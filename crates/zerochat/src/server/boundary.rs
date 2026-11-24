//! The HTTP boundary: which Host names zerochat answers (DNS-rebinding defence), which requests
//! may change state (same-origin JSON only), and the security headers on every answer.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::error::ApiError;
use super::state::AppState;

/// The Host header values zerochat answers (DNS-rebinding defence): exact `name[:port]` entries
/// and `*.suffix[:port]` patterns. No pattern matches everything; only `HostAllowlist::any()`
/// (`--any-host`, for when a logged-in proxy is the only way in) turns the check off.
#[derive(Clone, Debug, Default)]
pub struct HostAllowlist {
    patterns: Vec<HostPattern>,
    any: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HostPattern {
    /// Lower-case host name; for a wildcard, the suffix after `*.` (matched with a leading dot).
    name: String,
    wildcard: bool,
    /// None: any port (and no port).
    port: Option<String>,
}

impl HostAllowlist {
    pub fn new(entries: &[String]) -> Result<Self, String> {
        let patterns = entries
            .iter()
            .map(|e| HostPattern::parse(e))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            patterns,
            any: false,
        })
    }

    pub fn any() -> Self {
        Self {
            patterns: Vec::new(),
            any: true,
        }
    }

    /// The hosts a server listening on `listen` answers: `listen` itself and `localhost` on its
    /// port (plus `127.0.0.1` on its port when `listen` is the unspecified address), then the
    /// `extra` entries (`--allowed-host`). `any` (`--any-host`) turns the check off instead.
    pub fn for_listen(listen: SocketAddr, extra: &[String], any: bool) -> Result<Self, String> {
        if any {
            return Ok(Self::any());
        }
        let mut hosts = vec![listen.to_string(), format!("localhost:{}", listen.port())];
        if listen.ip().is_unspecified() {
            hosts.push(format!("127.0.0.1:{}", listen.port()));
        }
        hosts.extend(extra.iter().cloned());
        Self::new(&hosts)
    }

    pub fn allows(&self, host: &str) -> bool {
        if self.any {
            return true;
        }
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() || host.contains(['/', '@', ' ', '\\']) {
            return false;
        }
        let (name, port) = split_host_port(&host);
        self.patterns.iter().any(|p| {
            let port_ok = p.port.as_deref().is_none_or(|want| port == Some(want));
            let name_ok = if p.wildcard {
                name.len() > p.name.len() + 1 && name.ends_with(&format!(".{}", p.name))
            } else {
                name == p.name
            };
            port_ok && name_ok
        })
    }

    pub fn describe(&self) -> String {
        if self.any {
            return "any host".to_owned();
        }
        self.patterns
            .iter()
            .map(|p| {
                format!(
                    "{}{}{}",
                    if p.wildcard { "*." } else { "" },
                    p.name,
                    p.port
                        .as_deref()
                        .map(|port| format!(":{port}"))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `name:port`, `[v6]:port`, `name` or `[v6]`.
pub(crate) fn split_host_port(host: &str) -> (&str, Option<&str>) {
    if let Some(rest) = host.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((v6, tail)) => (v6, tail.strip_prefix(':')),
            None => (host, None),
        };
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') => (name, Some(port)),
        _ => (host, None),
    }
}

impl HostPattern {
    pub(crate) fn parse(entry: &str) -> Result<Self, String> {
        let entry = entry.trim().to_ascii_lowercase();
        let (name, port) = split_host_port(&entry);
        if let Some(port) = port
            && (port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()))
        {
            return Err(format!("--allowed-host {entry}: the port must be a number"));
        }
        let (wildcard, name) = match name.strip_prefix("*.") {
            Some(suffix) => (true, suffix),
            None => (false, name),
        };
        let label_ok = |l: &str| {
            !l.is_empty()
                && l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        let valid = !name.is_empty()
            && (name.split('.').all(label_ok)
                || (!wildcard && name.parse::<std::net::IpAddr>().is_ok()));
        if !valid || name.contains('*') {
            return Err(format!(
                "--allowed-host {entry}: give a host name (`zerochat:4317`) or a suffix pattern (`*.localhost:8090`); `*` alone would let any site reach zerochat (DNS rebinding)"
            ));
        }
        Ok(Self {
            name: name.to_owned(),
            wildcard,
            port: port.map(str::to_owned),
        })
    }
}

/// DNS-rebinding and cross-site protection: only this server's own host names, and state
/// changes only from same-origin JSON requests. Every answer carries the security headers.
pub(crate) async fn boundary(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let host = header_text(headers, header::HOST.as_str())
        .unwrap_or("")
        .to_owned();
    if !state.allowed_hosts.allows(&host) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "host_rejected",
            format!(
                "zerochat only answers requests for {}.",
                state.allowed_hosts.describe()
            ),
        )
        .into_response();
    }
    let changes_state = request.method() != Method::GET && request.method() != Method::HEAD;
    if changes_state && !same_origin(headers, &host) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "origin_rejected",
            "Cross-site requests are not allowed.",
        )
        .into_response();
    }
    if changes_state && !is_json(headers) {
        return ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "json_required",
            "Send application/json.",
        )
        .into_response();
    }
    let mut response = next.run(request).await;
    add_security_headers(response.headers_mut());
    response
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Whether a request comes from zerochat's own page (`host` is the request's Host header):
/// - `Sec-Fetch-Site`, when present, decides: the browser sets it, and it survives proxies that
///   rewrite Host;
/// - otherwise the `Origin` must name this host, or the proxy's `X-Forwarded-Host`;
/// - no `Origin` either: not a browser, so not a cross-site request.
fn same_origin(headers: &HeaderMap, host: &str) -> bool {
    if let Some(site) = header_text(headers, "sec-fetch-site") {
        return site == "same-origin" || site == "none";
    }
    let Some(origin) = header_text(headers, header::ORIGIN.as_str()) else {
        return true;
    };
    let Some((_, origin_host)) = origin.split_once("://") else {
        return false;
    };
    let forwarded_host = header_text(headers, "x-forwarded-host");
    origin_host.eq_ignore_ascii_case(host)
        || forwarded_host.is_some_and(|fwd| origin_host.eq_ignore_ascii_case(fwd))
}

fn is_json(headers: &HeaderMap) -> bool {
    header_text(headers, header::CONTENT_TYPE.as_str())
        .is_some_and(|v| v.starts_with("application/json"))
}

/// The Content-Security-Policy of the app shell (`index.html`, served by `assets`): only this
/// origin's scripts, styles, images, fonts and API; no plugins, frames, forms or `<base>`.
pub(crate) const INDEX_CSP: &str = concat!(
    "default-src 'none'; ",
    "script-src 'self'; ",
    "style-src 'self' 'unsafe-inline'; ",
    "img-src 'self' data:; ",
    "font-src 'self' data:; ",
    "connect-src 'self'; ",
    "base-uri 'none'; ",
    "form-action 'none'; ",
    "frame-ancestors 'none'",
);

fn add_security_headers(headers: &mut HeaderMap) {
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    axum::http::HeaderName::from_static(k),
                    HeaderValue::from_static(v),
                )
            })
            .collect()
    }

    #[test]
    fn same_origin_trusts_sec_fetch_site_first() {
        assert!(same_origin(
            &headers(&[
                ("sec-fetch-site", "same-origin"),
                ("origin", "http://evil.example")
            ]),
            "127.0.0.1:4317"
        ));
        assert!(same_origin(
            &headers(&[("sec-fetch-site", "none")]),
            "127.0.0.1:4317"
        ));
        assert!(!same_origin(
            &headers(&[
                ("sec-fetch-site", "cross-site"),
                ("origin", "http://127.0.0.1:4317")
            ]),
            "127.0.0.1:4317"
        ));
    }

    #[test]
    fn same_origin_otherwise_compares_origin_with_host_or_forwarded_host() {
        let host = "127.0.0.1:4317";
        assert!(same_origin(&headers(&[]), host), "no Origin: not a browser");
        assert!(same_origin(
            &headers(&[("origin", "http://127.0.0.1:4317")]),
            host
        ));
        assert!(same_origin(
            &headers(&[
                ("origin", "https://ZEROCHAT.localhost:8090"),
                ("x-forwarded-host", "zerochat.localhost:8090")
            ]),
            host
        ));
        assert!(!same_origin(
            &headers(&[("origin", "http://evil.example")]),
            host
        ));
        assert!(
            !same_origin(&headers(&[("origin", "null")]), host),
            "an opaque origin is not this host"
        );
    }

    #[test]
    fn host_allowlist_matches_names_ports_and_suffixes() {
        let hosts = HostAllowlist::new(&[
            "127.0.0.1:4317".to_owned(),
            "*.localhost:8090".to_owned(),
            "zerochat".to_owned(),
        ])
        .unwrap();
        assert!(hosts.allows("127.0.0.1:4317"));
        assert!(!hosts.allows("127.0.0.1:4318"));
        assert!(hosts.allows("a.localhost:8090"));
        assert!(
            !hosts.allows("localhost:8090"),
            "a suffix pattern needs a subdomain"
        );
        assert!(hosts.allows("zerochat:1234"), "no port: any port");
        assert!(!hosts.allows("evil.example"));
        assert!(HostAllowlist::new(&["*".to_owned()]).is_err());
        assert!(HostAllowlist::any().allows("anything"));
    }

    #[test]
    fn host_allowlist_for_listen_adds_localhost_and_the_unspecified_address() {
        let hosts = HostAllowlist::for_listen(
            "0.0.0.0:4317".parse().unwrap(),
            &["zerochat:4317".to_owned()],
            false,
        )
        .unwrap();
        assert_eq!(
            hosts.describe(),
            "0.0.0.0:4317, localhost:4317, 127.0.0.1:4317, zerochat:4317"
        );
        let loopback =
            HostAllowlist::for_listen("127.0.0.1:4317".parse().unwrap(), &[], false).unwrap();
        assert_eq!(loopback.describe(), "127.0.0.1:4317, localhost:4317");
        assert!(
            HostAllowlist::for_listen("127.0.0.1:4317".parse().unwrap(), &[], true)
                .unwrap()
                .allows("anything")
        );
        assert!(
            HostAllowlist::for_listen("127.0.0.1:4317".parse().unwrap(), &["*".to_owned()], false)
                .is_err()
        );
    }
}

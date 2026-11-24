//! Where zerochat listens: `127.0.0.1:4380` by default, else the next free port (up to ten
//! more). An address given with `--listen`/`ZEROCHAT_LISTEN`, or `--strict-port`, is used as it
//! is or not at all. A busy port that answers as zerochat is a zerochat already running: the
//! caller opens that one instead of starting a second.
//!
//! 4380, not 4317 (OTLP/gRPC), 4318 (OTLP/HTTP) or 4173 (`zeroshot ui`).

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::net::TcpListener;

/// The default listen address.
pub const DEFAULT_LISTEN: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 4380);
/// How many ports after a busy default are tried.
pub const FALLBACK_PORTS: u16 = 10;
/// How long a busy port has to answer `GET /api/setup?probe` to count as a running zerochat.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Whether another port may be tried when the asked one is busy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strictness {
    /// The default address: try the next ports.
    Fallback,
    /// `--strict-port` with the default address.
    StrictPort,
    /// `--listen` or `ZEROCHAT_LISTEN`.
    Explicit,
}

pub enum Bound {
    Listening(TcpListener),
    /// A zerochat already answers at this URL.
    AlreadyRunning(String),
}

/// The address a browser on this machine opens: the unspecified address (`0.0.0.0`) is not
/// one, so it becomes loopback on the same port.
pub fn browser_address(listen: SocketAddr) -> String {
    if listen.ip().is_unspecified() {
        format!("127.0.0.1:{}", listen.port())
    } else {
        listen.to_string()
    }
}

pub fn url(listen: SocketAddr) -> String {
    format!("http://{}/", browser_address(listen))
}

/// Binds `addr`, or with `Fallback` the first free port of the next `FALLBACK_PORTS`.
pub async fn bind(addr: SocketAddr, strictness: Strictness) -> Result<Bound> {
    let tries = if strictness == Strictness::Fallback {
        FALLBACK_PORTS
    } else {
        0
    };
    for step in 0..=tries {
        let Some(port) = addr.port().checked_add(step) else {
            break;
        };
        let candidate = SocketAddr::new(addr.ip(), port);
        match TcpListener::bind(candidate).await {
            Ok(listener) => return Ok(Bound::Listening(listener)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                if is_zerochat(candidate).await {
                    return Ok(Bound::AlreadyRunning(url(candidate)));
                }
            }
            Err(error) => return Err(anyhow!("cannot listen on {candidate}: {error}")),
        }
    }
    Err(anyhow!(busy_message(addr, strictness)))
}

fn busy_message(addr: SocketAddr, strictness: Strictness) -> String {
    let pick = format!(
        "pick another with --listen {}:<port> (or ZEROCHAT_LISTEN)",
        addr.ip()
    );
    match strictness {
        Strictness::Explicit => format!(
            "cannot listen on {addr}: port {} is in use by another program; {pick}",
            addr.port()
        ),
        Strictness::StrictPort => format!(
            "cannot listen on {addr}: port {} is in use by another program; drop --strict-port to try the next ports, or {pick}",
            addr.port()
        ),
        Strictness::Fallback => {
            let last = addr.port().saturating_add(FALLBACK_PORTS);
            format!(
                "cannot listen on {}: ports {}-{last} are all in use; {pick}",
                addr.ip(),
                addr.port()
            )
        }
    }
}

/// The program on `addr` answers `GET /api/setup?probe` with `app: "zerochat"`.
pub async fn is_zerochat(addr: SocketAddr) -> bool {
    let Ok(client) = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() else {
        return false;
    };
    let Ok(response) = client
        .get(format!("http://{}/api/setup?probe", browser_address(addr)))
        .send()
        .await
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .json::<serde_json::Value>()
        .await
        .is_ok_and(|v| v["app"] == crate::discovery::APP_NAME)
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::routing::get;

    use super::*;

    async fn taken() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        (listener, addr)
    }

    fn port_of(bound: Bound) -> u16 {
        match bound {
            Bound::Listening(l) => l.local_addr().unwrap().port(),
            Bound::AlreadyRunning(url) => panic!("not zerochat, yet: already running at {url}"),
        }
    }

    #[tokio::test]
    async fn a_busy_default_port_falls_back_to_the_next_free_one() {
        let (_held, addr) = taken().await;
        let port = port_of(bind(addr, Strictness::Fallback).await.unwrap());
        assert!(
            port > addr.port() && port <= addr.port() + FALLBACK_PORTS,
            "{port} after {}",
            addr.port()
        );
    }

    #[tokio::test]
    async fn a_free_port_is_used_as_it_is() {
        let (held, addr) = taken().await;
        drop(held);
        assert_eq!(
            port_of(bind(addr, Strictness::Explicit).await.unwrap()),
            addr.port()
        );
    }

    #[tokio::test]
    async fn strict_addresses_are_refused_naming_the_port_and_the_flag() {
        let (_held, addr) = taken().await;
        let Err(error) = bind(addr, Strictness::Explicit).await else {
            panic!("bound a busy port")
        };
        let message = error.to_string();
        assert!(
            message.contains(&format!("port {} is in use", addr.port()))
                && message.contains("--listen"),
            "{message}"
        );
        let Err(error) = bind(addr, Strictness::StrictPort).await else {
            panic!("bound a busy port")
        };
        assert!(error.to_string().contains("drop --strict-port"), "{error}");
    }

    #[test]
    fn says_when_every_fallback_port_is_taken() {
        let message = busy_message(DEFAULT_LISTEN, Strictness::Fallback);
        assert_eq!(
            message,
            "cannot listen on 127.0.0.1: ports 4380-4390 are all in use; pick another with --listen 127.0.0.1:<port> (or ZEROCHAT_LISTEN)"
        );
    }

    async fn stub(body: &'static str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let (listener, addr) = taken().await;
        let app = Router::new().route(
            "/api/setup",
            get(move || async move { ([("content-type", "application/json")], body) }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, task)
    }

    #[tokio::test]
    async fn a_running_zerochat_is_recognised_and_reused() {
        let (addr, task) = stub(r#"{"app":"zerochat","version":"0.1.0","ok":true}"#).await;
        match bind(addr, Strictness::Explicit).await.unwrap() {
            Bound::AlreadyRunning(url) => assert_eq!(url, format!("http://{addr}/")),
            Bound::Listening(_) => panic!("bound a busy port"),
        }
        task.abort();
    }

    #[tokio::test]
    async fn another_program_on_the_port_is_not_zerochat() {
        let (addr, task) = stub(r#"{"app":"something-else"}"#).await;
        assert!(!is_zerochat(addr).await);
        assert!(bind(addr, Strictness::Explicit).await.is_err());
        let port = port_of(bind(addr, Strictness::Fallback).await.unwrap());
        assert_ne!(port, addr.port());
        task.abort();
    }

    #[tokio::test]
    async fn a_port_that_never_answers_is_given_up_on() {
        let (_silent, addr) = taken().await; // listens, never accepts
        let started = std::time::Instant::now();
        assert!(!is_zerochat(addr).await);
        assert!(started.elapsed() < PROBE_TIMEOUT + Duration::from_secs(1));
    }

    #[test]
    fn the_default_is_not_an_otel_or_zeroshot_ui_port() {
        assert_eq!(DEFAULT_LISTEN.to_string(), "127.0.0.1:4380");
        for step in 0..=FALLBACK_PORTS {
            assert!(![4317, 4318, 4173].contains(&(DEFAULT_LISTEN.port() + step)));
        }
        assert_eq!(
            browser_address("0.0.0.0:4380".parse().unwrap()),
            "127.0.0.1:4380"
        );
    }
}

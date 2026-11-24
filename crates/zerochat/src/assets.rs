//! The web UI, embedded at build time (`web/dist`). `ZEROCHAT_WEB_DIR` serves a directory
//! from disk instead, for UI development. The security headers of every answer, including the
//! app shell's CSP (`INDEX_CSP`), are in `server::boundary`.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use include_dir::{Dir, include_dir};

use crate::server::{ApiError, INDEX_CSP};

static ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../web/dist");

pub async fn serve(request: Request) -> Response {
    let path = request.uri().path().trim_start_matches('/');
    if path.starts_with("api/") {
        return ApiError::not_found("not_found", "No such API route.").into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };
    if path.split('/').any(|p| p == "..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let (bytes, name) = match load(path) {
        Some(bytes) => (bytes, path.to_owned()),
        // Client-side routes fall back to the app shell.
        None if !path.contains('.') => match load("index.html") {
            Some(bytes) => (bytes, "index.html".to_owned()),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime(&name)));
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    };
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if name == "index.html" {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(INDEX_CSP),
        );
    }
    response
}

fn load(path: &str) -> Option<Vec<u8>> {
    if let Ok(dir) = std::env::var("ZEROCHAT_WEB_DIR") {
        return std::fs::read(std::path::Path::new(&dir).join(path)).ok();
    }
    ASSETS.get_file(path).map(|f| f.contents().to_vec())
}

fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    async fn get(path: &str) -> (StatusCode, String, String) {
        let response = serve(Request::get(path).body(Body::empty()).unwrap()).await;
        let status = response.status();
        let mime = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, mime, String::from_utf8_lossy(&body).into_owned())
    }

    /// The `src`/`href` values of `html` that point into `assets/`.
    fn asset_refs(html: &str) -> Vec<String> {
        ["src=\"", "href=\""]
            .iter()
            .flat_map(|attr| {
                html.split(attr)
                    .skip(1)
                    .filter_map(|rest| rest.split('"').next())
            })
            .filter(|url| url.contains("assets/"))
            .map(str::to_owned)
            .collect()
    }

    /// `url` as the browser resolves it on the page at `page` (a path).
    fn resolve(page: &str, url: &str) -> String {
        if url.starts_with('/') {
            return url.to_owned();
        }
        let dir = &page[..=page.rfind('/').unwrap()];
        format!("{dir}{}", url.trim_start_matches("./"))
    }

    #[test]
    fn resolves_like_a_browser() {
        assert_eq!(
            resolve("/runs/local/x", "./assets/a.js"),
            "/runs/local/assets/a.js"
        );
        assert_eq!(resolve("/runs/local/x", "/assets/a.js"), "/assets/a.js");
        assert_eq!(
            asset_refs(
                r#"<script src="/assets/a.js"></script><link href="/assets/b.css"><link href="data:,">"#
            ),
            ["/assets/a.js", "/assets/b.css"]
        );
    }

    /// A path that is not a hash route (`/runs/local/<id>`) gets the app shell, and every asset
    /// the shell names loads from there (they are absolute: vite `base: '/'`).
    #[tokio::test]
    async fn a_hash_less_path_serves_a_page_whose_assets_load() {
        let page = "/runs/local/01a20363-253a-76c1-8872-daa49cdaa360";
        let (status, mime, html) = get(page).await;
        assert_eq!(status, StatusCode::OK);
        assert!(mime.starts_with("text/html"), "{mime}");
        // Without a built UI (web/dist is the build's placeholder) there are no assets to load.
        for url in asset_refs(&html) {
            assert!(
                url.starts_with('/'),
                "relative asset URL {url} breaks on {page}"
            );
            let (status, _, _) = get(&resolve(page, &url)).await;
            assert_eq!(status, StatusCode::OK, "{url} from {page}");
        }
    }

    #[test]
    fn the_web_build_uses_absolute_asset_urls() {
        let config = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/vite.config.ts"
        ))
        .unwrap();
        assert!(
            config.contains("base: '/',"),
            "web/vite.config.ts must build with base '/' (absolute /assets/ URLs)"
        );
    }

    #[tokio::test]
    async fn a_missing_asset_is_still_a_404() {
        assert_eq!(
            get("/assets/no-such-file.js").await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get("/runs/local/assets/no-such-file.js").await.0,
            StatusCode::NOT_FOUND
        );
    }
}

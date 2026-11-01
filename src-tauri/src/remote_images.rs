//! Loads the remote images of a mail through the app instead of the webview.
//!
//! Some senders serve their images with `Cross-Origin-Resource-Policy: same-origin`, which keeps
//! a browser from showing them anywhere else. The sanitizer points every remote image at the
//! `rpimg` scheme, and this module fetches it, so the policy no longer applies. The request
//! carries no cookies and no referrer. Whether images load at all is still up to the reading
//! pane's CSP, which blocks the scheme until the user allows remote content.

use std::sync::OnceLock;
use std::time::Duration;

use tauri::http::{header, Request, Response, StatusCode};

pub const SCHEME: &str = "rpimg";

#[cfg(windows)]
const BASE: &str = "http://rpimg.localhost/?u=";
#[cfg(not(windows))]
const BASE: &str = "rpimg://localhost/?u=";

/// Larger answers are dropped, a mail image never needs that much.
const MAX_BYTES: usize = 15 * 1024 * 1024;

/// The remote URL in `value`, made absolute, if it is one.
fn remote(value: &str) -> Option<String> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(v.to_string())
    } else if lower.starts_with("//") {
        Some(format!("https:{v}"))
    } else {
        None
    }
}

/// The proxied address for a remote `src` or `background` value, `None` for anything else.
pub fn proxied(value: &str) -> Option<String> {
    let url = remote(value)?;
    let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
    Some(format!("{BASE}{encoded}"))
}

/// Points every remote `url(...)` in an inline style at the proxy.
pub fn rewrite_css(style: &str) -> String {
    let mut out = String::with_capacity(style.len());
    let mut rest = style;
    while let Some(pos) = rest.to_ascii_lowercase().find("url(") {
        out.push_str(&rest[..pos + 4]);
        rest = &rest[pos + 4..];
        let Some(end) = rest.find(')') else { break };
        let inner = rest[..end].trim().trim_matches(|c| c == '"' || c == '\'');
        match proxied(inner) {
            Some(p) => out.push_str(&p),
            None => out.push_str(&rest[..end]),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("redpanda/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client")
    })
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).body(Vec::new()).unwrap()
}

/// The target of a proxy request, only plain web addresses.
fn target(request: &Request<Vec<u8>>) -> Option<reqwest::Url> {
    let query = request.uri().query()?;
    let (_, value) = url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "u")?;
    let url = reqwest::Url::parse(&value).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// Answers one `rpimg` request with the image it names.
pub async fn serve(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some(url) = target(&request) else {
        return status(StatusCode::BAD_REQUEST);
    };
    let Ok(mut res) = client().get(url).header(header::ACCEPT, "image/*").send().await else {
        return status(StatusCode::BAD_GATEWAY);
    };
    if !res.status().is_success() {
        return status(StatusCode::BAD_GATEWAY);
    }
    let kind = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    // Only images go back into the mail, never a page or a script.
    if !(kind.starts_with("image/") || kind.starts_with("application/octet-stream")) {
        return status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    if res.content_length().unwrap_or(0) as usize > MAX_BYTES {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let mut body = Vec::new();
    loop {
        match res.chunk().await {
            Ok(Some(chunk)) => {
                body.extend_from_slice(&chunk);
                if body.len() > MAX_BYTES {
                    return status(StatusCode::PAYLOAD_TOO_LARGE);
                }
            }
            Ok(None) => break,
            Err(_) => return status(StatusCode::BAD_GATEWAY),
        }
    }
    Response::builder()
        .header(header::CONTENT_TYPE, kind)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "max-age=86400")
        .body(body)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unproxy(p: &str) -> String {
        let query = p.split_once('?').unwrap().1;
        let req = Request::builder()
            .uri(format!("http://rpimg.localhost/?{query}"))
            .body(Vec::new())
            .unwrap();
        target(&req).unwrap().to_string()
    }

    #[test]
    fn round_trips_remote_urls() {
        let url = "https://claude.ai/images/a b.png?x=1&y=+2";
        let p = proxied(url).unwrap();
        assert!(p.starts_with(BASE));
        assert!(!p[BASE.len()..].contains(['&', '(', ')', '"', '\'', ' ']));
        assert_eq!(unproxy(&p), reqwest::Url::parse(url).unwrap().to_string());
        assert_eq!(
            unproxy(&proxied("//cdn.example/x.gif").unwrap()),
            "https://cdn.example/x.gif"
        );
    }

    #[test]
    fn leaves_local_urls_alone() {
        assert_eq!(proxied("data:image/png;base64,AAAA"), None);
        assert_eq!(proxied("cid:abc"), None);
    }

    #[test]
    fn rejects_other_schemes() {
        let req = Request::builder()
            .uri("http://rpimg.localhost/?u=file%3A%2F%2F%2FC%3A%2Fx")
            .body(Vec::new())
            .unwrap();
        assert!(target(&req).is_none());
    }

    #[test]
    fn rewrites_css_urls() {
        let css = "color:red;background:URL('https://x.example/bg.png') no-repeat;list-style:url(data:image/png;base64,AA)";
        let out = rewrite_css(css);
        assert!(
            out.contains(&proxied("https://x.example/bg.png").unwrap()),
            "{out}"
        );
        assert!(out.contains("url(data:image/png;base64,AA)"), "{out}");
        assert!(out.starts_with("color:red;background:URL("));
        assert!(out.ends_with(" no-repeat;list-style:url(data:image/png;base64,AA)"));
    }
}

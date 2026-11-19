//! The browser page (backlog C2, ADR-0055): a phone or computer with no app opens a link or QR code and can
//! download what the user shared and upload to this device.
//!
//! It lives on the `LocalSend` receiver's HTTPS server: `/s/<token>/` is the page, `/s/<token>/list` the shared
//! files, `/s/<token>/f/<n>` one download. Uploads go through the page's script to the ordinary `LocalSend` v2
//! upload API, so they take the same path as any `LocalSend` sender: the user is asked, files are staged, every
//! limit applies. The token is 128 random bits and expires; a wrong or expired token gets a bare `404`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{HeaderValue, Method, Response, StatusCode, header};
use http_body_util::{Either, Full};

/// Responses of the whole server: small bodies, or a file streamed from disk.
pub type Body = Either<Full<Bytes>, http_body_util::channel::Channel<Bytes, std::io::Error>>;

/// Most files in one share, and most live shares.
pub const MAX_SHARED: usize = 1000;
pub const MAX_SHARES: usize = 16;
/// Longest a share may live.
pub const MAX_TTL: Duration = Duration::from_secs(24 * 3600);

/// One shared file.
#[derive(Debug, Clone)]
pub struct Shared {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

/// A live link.
#[derive(Debug, Clone)]
pub struct Share {
    pub files: Vec<Shared>,
    pub until: Instant,
}

/// Builds the file list of a share from paths the daemon chose: each must be a readable regular file.
pub fn collect(paths: &[PathBuf]) -> std::io::Result<Vec<Shared>> {
    if paths.len() > MAX_SHARED {
        return Err(std::io::Error::other("too many files"));
    }
    paths
        .iter()
        .map(|p| {
            let meta = std::fs::metadata(p)?;
            if !meta.is_file() {
                return Err(std::io::Error::other(format!(
                    "{} is not a file",
                    p.display()
                )));
            }
            let raw = p.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            Ok(Shared {
                name: vutric_core::filename::sanitize(raw),
                path: p.clone(),
                size: meta.len(),
            })
        })
        .collect()
}

/// What a `/s/...` request asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Route<'a> {
    Page(&'a str),
    List(&'a str),
    File(&'a str, usize),
}

/// Parses `/s/<token>/`, `/s/<token>/list` and `/s/<token>/f/<n>`; anything else is `None`.
#[must_use]
pub fn route(path: &str) -> Option<Route<'_>> {
    let rest = path.strip_prefix("/s/")?;
    let (token, tail) = rest.split_once('/').unwrap_or((rest, ""));
    if token.len() != 32
        || !token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    match tail {
        "" => Some(Route::Page(token)),
        "list" => Some(Route::List(token)),
        _ => {
            let n = tail.strip_prefix("f/")?;
            if n.is_empty() || n.len() > 4 || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some(Route::File(token, n.parse().ok()?))
        }
    }
}

fn base(status: StatusCode, body: Body) -> Response<Body> {
    let mut r = Response::new(body);
    *r.status_mut() = status;
    let h = r.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // The token is in the URL: never send it anywhere else, never keep it in a cache.
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    r
}

#[must_use]
pub fn not_found() -> Response<Body> {
    base(StatusCode::NOT_FOUND, Either::Left(Full::new(Bytes::new())))
}

/// The page, with a fresh script nonce.
#[must_use]
pub fn page(nonce: &str) -> Response<Body> {
    let html = PAGE.replace("{NONCE}", nonce);
    let mut r = base(StatusCode::OK, Either::Left(Full::new(Bytes::from(html))));
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; connect-src 'self'; \
         img-src 'self' data:; form-action 'none'; base-uri 'none'; frame-ancestors 'none'"
    )) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    r
}

#[derive(serde::Serialize)]
struct ListItem<'a> {
    name: &'a str,
    size: u64,
}

#[derive(serde::Serialize)]
struct List<'a> {
    alias: &'a str,
    files: Vec<ListItem<'a>>,
    /// Seconds until the link stops working.
    expires_in: u64,
}

#[must_use]
pub fn list(share: &Share, alias: &str) -> Response<Body> {
    let body = serde_json::to_vec(&List {
        alias,
        files: share
            .files
            .iter()
            .map(|f| ListItem {
                name: &f.name,
                size: f.size,
            })
            .collect(),
        expires_in: share
            .until
            .saturating_duration_since(Instant::now())
            .as_secs(),
    })
    .unwrap_or_default();
    let mut r = base(StatusCode::OK, Either::Left(Full::new(Bytes::from(body))));
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    r
}

/// `Content-Disposition` for a download: an ASCII fallback plus the exact name (RFC 6266, RFC 8187).
#[must_use]
pub fn disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut exact = String::with_capacity(name.len() * 3);
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            exact.push(char::from(b));
        } else {
            use std::fmt::Write as _;
            let _ = write!(exact, "%{b:02X}");
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{exact}")
}

/// Streams one shared file.
#[must_use]
pub fn file(f: &Shared) -> Response<Body> {
    let mut r = base(
        StatusCode::OK,
        Either::Right(crate::client::file_body(f.path.clone())),
    );
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(f.size));
    if let Ok(v) = HeaderValue::from_str(&disposition(&f.name)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    r
}

/// Only `GET` reaches the page routes.
#[must_use]
pub fn method_ok(m: &Method) -> bool {
    m == Method::GET
}

const PAGE: &str = include_str!("web_page.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        let t = "0123456789abcdef0123456789abcdef";
        assert_eq!(route(&format!("/s/{t}/")), Some(Route::Page(t)));
        assert_eq!(route(&format!("/s/{t}")), Some(Route::Page(t)));
        assert_eq!(route(&format!("/s/{t}/list")), Some(Route::List(t)));
        assert_eq!(route(&format!("/s/{t}/f/12")), Some(Route::File(t, 12)));
        for bad in [
            "/s/short/".to_owned(),
            format!("/s/{}/", t.to_uppercase()),
            format!("/s/{t}/f/"),
            format!("/s/{t}/f/99999"),
            format!("/s/{t}/f/-1"),
            format!("/s/{t}/../x"),
            "/api/localsend/v2/info".to_owned(),
        ] {
            assert_eq!(route(&bad), None, "{bad}");
        }
    }

    #[test]
    fn disposition_is_header_safe() {
        let d = disposition("ota\"ning rasmi\r\n.jpg");
        assert!(HeaderValue::from_str(&d).is_ok());
        assert!(d.contains("filename=\"ota_ning rasmi__.jpg\""));
        assert!(d.contains("filename*=UTF-8''ota%22ning%20rasmi%0D%0A.jpg"));
        let d = disposition("o\u{2018}g\u{2018}il.png");
        assert!(HeaderValue::from_str(&d).is_ok());
        assert!(d.contains("%E2%80%98"));
    }

    #[test]
    fn page_has_no_external_resources() {
        let r = page("abc");
        let csp = r.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(csp.contains("default-src 'none'"));
        assert!(!PAGE.contains("http://") && !PAGE.contains("https://"));
        assert!(PAGE.matches("{NONCE}").count() >= 2);
    }
}

//! Captive-portal answers (backlog R1a, ADR-0068): hotel, airport and guest Wi-Fi that answer every web request
//! with their own login page until the user signs in. On such a network both the LAN path and the relay fail, and
//! the right screen is "sign in to this Wi-Fi first", not "this network keeps devices apart". This is the pure,
//! fuzzed parser; the probe that sends the request is `vutric_net::captive::check`.
//!
//! The check is one plain-HTTP `GET` to a host that is known to answer `204 No Content`. A portal intercepts it
//! and answers with a redirect or a page. Which host to ask is the caller's choice (an owner question, ADR-0068):
//! nothing here names a third-party server.
//!
//! What the network says is untrusted: at most [`MAX_HEAD`] bytes are read, the login URL is kept only when it is
//! a short `http(s)` URL without control characters, and the whole check is bounded by the caller's `wait`.

/// The path a check host answers with `204 No Content` (the convention Android and Chrome OS use).
pub const CHECK_PATH: &str = "/generate_204";
/// Most bytes of the answer read: the status line and headers fit, a portal page is not needed.
pub const MAX_HEAD: usize = 8 * 1024;
/// Longest login URL passed on to the screen.
pub const MAX_LOGIN_URL: usize = 512;

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Portal {
    /// The check host answered itself: the internet is open.
    Open,
    /// Something answered in its place: a portal. `login` is where it sent us, when it said so and the URL is
    /// safe to show as a link.
    Captive { login: Option<String> },
    /// No answer at all (no route, DNS failed, timed out): not a portal question.
    Offline,
}

impl Portal {
    #[must_use]
    pub fn is_captive(&self) -> bool {
        matches!(self, Self::Captive { .. })
    }
}

/// Reads an HTTP/1.x answer head. `204`, or `200` with an empty body, is the check host; a redirect or any other
/// page is a portal; bytes that are not HTTP at all count as a portal too (something in the middle answered).
#[must_use]
pub fn classify(answer: &[u8]) -> Portal {
    let answer = &answer[..answer.len().min(MAX_HEAD)];
    let head_end = find(answer, b"\r\n\r\n").map_or(answer.len(), |i| i + 4);
    let Ok(head) = std::str::from_utf8(&answer[..head_end]) else {
        return Portal::Captive { login: None };
    };
    let mut lines = head.split("\r\n");
    let status = lines.next().and_then(|l| {
        let mut parts = l.splitn(3, ' ');
        let version = parts.next()?;
        if !version.starts_with("HTTP/1.") {
            return None;
        }
        parts.next()?.parse::<u16>().ok()
    });
    let mut location = None;
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("location") {
            location = Some(value);
        } else if name.eq_ignore_ascii_case("content-length") {
            length = value.parse::<u64>().ok();
        }
    }
    match status {
        Some(204) => Portal::Open,
        Some(200) if length == Some(0) => Portal::Open,
        Some(301 | 302 | 303 | 307 | 308) => Portal::Captive {
            login: location.and_then(login_url),
        },
        _ => Portal::Captive { login: None },
    }
}

/// A login URL fit to show as a link: `http://` or `https://`, short, printable ASCII only.
fn login_url(raw: &str) -> Option<String> {
    let ok = raw.len() <= MAX_LOGIN_URL
        && (raw.starts_with("http://") || raw.starts_with("https://"))
        && raw.len() > "https://".len()
        && raw.bytes().all(|b| b.is_ascii_graphic());
    ok.then(|| raw.to_owned())
}

/// Where `needle` first starts in `hay`.
#[must_use]
pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_classified() {
        assert_eq!(classify(b"HTTP/1.1 204 No Content\r\n\r\n"), Portal::Open);
        assert_eq!(
            classify(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n"),
            Portal::Open
        );
        assert_eq!(
            classify(b"HTTP/1.1 302 Found\r\nLocation: http://login.hotel.example/?u=1\r\n\r\n"),
            Portal::Captive {
                login: Some("http://login.hotel.example/?u=1".into())
            }
        );
        assert_eq!(
            classify(b"HTTP/1.1 200 OK\r\nContent-Length: 900\r\n\r\n<html>Sign in</html>"),
            Portal::Captive { login: None }
        );
        for unsafe_location in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://",
            "http://a b",
        ] {
            let a = format!("HTTP/1.1 302 Found\r\nLocation: {unsafe_location}\r\n\r\n");
            assert_eq!(classify(a.as_bytes()), Portal::Captive { login: None });
        }
        let long = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{}\r\n\r\n",
            "a".repeat(MAX_LOGIN_URL)
        );
        assert_eq!(classify(long.as_bytes()), Portal::Captive { login: None });
        assert_eq!(
            classify(b"\xff\xfe garbage"),
            Portal::Captive { login: None }
        );
        assert_eq!(classify(b""), Portal::Captive { login: None });
    }
}

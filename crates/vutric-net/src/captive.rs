//! The captive-portal probe (backlog R1a, ADR-0068); the answer is read by [`vutric_core::captive::classify`].

use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use vutric_core::captive::find;
pub use vutric_core::captive::{CHECK_PATH, MAX_HEAD, MAX_LOGIN_URL, Portal, classify};

/// Asks `host` (name, or `name:port`; port 80 when absent) for [`CHECK_PATH`] over plain HTTP, within `wait`.
pub async fn check(host: &str, wait: Duration) -> Portal {
    let (name, addr) = match host.rsplit_once(':') {
        Some((n, p)) if p.parse::<u16>().is_ok() && (!n.contains(':') || n.ends_with(']')) => {
            (n, host.to_owned())
        }
        _ => (host, format!("{host}:80")),
    };
    let ask = async {
        let mut s = TcpStream::connect(addr).await.ok()?;
        let request = format!(
            "GET {CHECK_PATH} HTTP/1.1\r\nHost: {name}\r\nUser-Agent: Vutric\r\nConnection: close\r\n\r\n"
        );
        s.write_all(request.as_bytes()).await.ok()?;
        let mut buf = vec![0u8; MAX_HEAD];
        let mut got = 0;
        while got < buf.len() {
            match s.read(&mut buf[got..]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => got += n,
            }
            if find(&buf[..got], b"\r\n\r\n").is_some() {
                break;
            }
        }
        (got > 0).then(|| classify(&buf[..got]))
    };
    tokio::time::timeout(wait, ask)
        .await
        .ok()
        .flatten()
        .unwrap_or(Portal::Offline)
}

/// The check host for one of our relays (ADR-0068): `iroh-relay` answers [`CHECK_PATH`] over plain HTTP, on
/// port 80 when the relay uses TLS (`https://domain/`) and on its own port otherwise (`http://host:port/`). So no
/// third-party server is asked. `None` for anything else.
#[must_use]
pub fn host_for_relay(relay_url: &str) -> Option<String> {
    let (https, rest) = if let Some(r) = relay_url.strip_prefix("https://") {
        (true, r)
    } else {
        (false, relay_url.strip_prefix("http://")?)
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    if https {
        // TLS relays serve the check on plain port 80, whatever port their HTTPS uses.
        let host = match authority.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => h,
            _ => authority,
        };
        Some(host.to_owned())
    } else {
        Some(authority.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_urls_give_the_check_host() {
        assert_eq!(
            host_for_relay("https://relay.vutric.example/").as_deref(),
            Some("relay.vutric.example")
        );
        assert_eq!(
            host_for_relay("https://relay.vutric.example:8443/").as_deref(),
            Some("relay.vutric.example")
        );
        assert_eq!(
            host_for_relay("http://127.0.0.1:3340/").as_deref(),
            Some("127.0.0.1:3340")
        );
        for bad in ["ftp://x/", "https:///", "http://u@evil/", "relay.example"] {
            assert_eq!(host_for_relay(bad), None, "{bad}");
        }
    }

    async fn serve(answer: &'static [u8]) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let mut req = [0u8; 1024];
                let n = s.read(&mut req).await.unwrap_or(0);
                assert!(req[..n].starts_with(b"GET /generate_204 HTTP/1.1\r\n"));
                let _ = s.write_all(answer).await;
            }
        });
        format!("127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn check_against_a_local_server() {
        let wait = Duration::from_secs(5);
        let open = serve(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").await;
        assert_eq!(check(&open, wait).await, Portal::Open);
        let portal =
            serve(b"HTTP/1.1 302 Found\r\nLocation: https://wifi.example/login\r\n\r\n").await;
        assert_eq!(
            check(&portal, wait).await,
            Portal::Captive {
                login: Some("https://wifi.example/login".into())
            }
        );
        let closed = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let a = l.local_addr().unwrap();
            drop(l);
            a.to_string()
        };
        assert_eq!(check(&closed, wait).await, Portal::Offline);
    }

    #[tokio::test]
    async fn a_silent_server_is_offline_within_the_wait() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a = l.local_addr().unwrap().to_string();
        let keep = tokio::spawn(async move {
            let mut held = vec![];
            while let Ok((s, _)) = l.accept().await {
                held.push(s);
            }
        });
        let t = std::time::Instant::now();
        assert_eq!(check(&a, Duration::from_millis(300)).await, Portal::Offline);
        assert!(t.elapsed() < Duration::from_secs(2));
        keep.abort();
    }
}

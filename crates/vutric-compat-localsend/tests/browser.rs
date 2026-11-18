//! The browser page (C2) over real HTTPS on loopback: page and headers, list, download, wrong and closed tokens,
//! and an upload offer shaped exactly like the page's script sends it.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use vutric_compat_localsend::proto::DeviceType;
use vutric_compat_localsend::server::{self, Decision, Event, Receiver};
use vutric_compat_localsend::tls::Identity;

const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

struct Answer {
    status: u16,
    head: String,
    body: Vec<u8>,
}

/// One HTTP/1.1 request over TLS, read to the end (`Connection: close`).
async fn get(addr: SocketAddr, method: &str, path: &str, body: &[u8]) -> Answer {
    let id = Identity::generate().unwrap();
    let conn = tokio_rustls::TlsConnector::from(Arc::new(id.client_config().unwrap()));
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let name = rustls::pki_types::ServerName::try_from("vutric").unwrap();
    let mut s = conn.connect(name, tcp).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    s.write_all(body).await.unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw).await;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let status = head[9..12].parse().unwrap();
    let mut body = raw[split + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        body = dechunk(&body);
    }
    Answer { status, head, body }
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n").unwrap();
        let n = usize::from_str_radix(std::str::from_utf8(&b[..eol]).unwrap().trim(), 16).unwrap();
        if n == 0 {
            return out;
        }
        out.extend_from_slice(&b[eol + 2..eol + 2 + n]);
        b = &b[eol + 4 + n..];
    }
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("vutric-web-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn receiver(dir: &std::path::Path) -> (Receiver, tokio::sync::mpsc::Receiver<Event>) {
    Receiver::start(
        server::Config {
            alias: "Ota kompyuteri".into(),
            device_type: DeviceType::Desktop,
            port: 0,
            bind: LOCAL,
            staging: dir.join("staging"),
            pin: None,
            decide_timeout: Duration::from_secs(10),
        },
        &Identity::generate().unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn page_list_download_and_closed_links() {
    let dir = tmp("dl");
    let f = dir.join("o\u{2018}g\u{2018}il rasmi.jpg");
    let data: Vec<u8> = (0..700_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&f, &data).unwrap();
    let (rx, mut events) = receiver(&dir).await;
    let token = rx.share(&[f], Duration::from_secs(600)).unwrap();
    let a = rx.addr();

    let page = get(a, "GET", &format!("/s/{token}/"), b"").await;
    assert_eq!(page.status, 200);
    assert!(
        page.head
            .contains("content-security-policy: default-src 'none'")
    );
    assert!(page.head.contains("referrer-policy: no-referrer"));
    assert!(page.head.contains("cache-control: no-store"));
    let html = String::from_utf8(page.body).unwrap();
    assert!(html.contains("/api/localsend/v2/prepare-upload"));
    assert!(!html.contains("{NONCE}"));

    let list = get(a, "GET", &format!("/s/{token}/list"), b"").await;
    let j: serde_json::Value = serde_json::from_slice(&list.body).unwrap();
    assert_eq!(j["alias"], "Ota kompyuteri");
    assert_eq!(j["files"][0]["size"], 700_000);
    assert!(j["expires_in"].as_u64().unwrap() > 590);

    let file = get(a, "GET", &format!("/s/{token}/f/0"), b"").await;
    assert_eq!(file.status, 200);
    assert!(file.head.contains("content-disposition: attachment;"));
    assert!(
        file.head
            .contains("filename*=utf-8''o%e2%80%98g%e2%80%98il")
    );
    assert_eq!(file.body, data);
    match events.recv().await {
        Some(Event::Downloaded { token: t, .. }) => assert_eq!(t, token),
        other => panic!("{other:?}"),
    }

    for bad in [
        format!("/s/{token}/f/1"),
        "/s/00000000000000000000000000000000/".to_owned(),
        format!("/s/{token}/../list"),
    ] {
        assert_eq!(get(a, "GET", &bad, b"").await.status, 404, "{bad}");
    }
    assert_eq!(
        get(a, "POST", &format!("/s/{token}/list"), b"")
            .await
            .status,
        404
    );
    rx.unshare(&token);
    assert_eq!(
        get(a, "GET", &format!("/s/{token}/"), b"").await.status,
        404
    );
    let short = rx.share(&[], Duration::from_millis(1)).unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        get(a, "GET", &format!("/s/{short}/list"), b"").await.status,
        404
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_offer_from_the_page() {
    let dir = tmp("up");
    let (rx, mut events) = receiver(&dir).await;
    let user = tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            match ev {
                Event::Offer { offer, reply } => {
                    assert_eq!(offer.from.alias, "Safari (web)");
                    assert_eq!(offer.from.device_type, Some(DeviceType::Web));
                    reply.send(Decision::Accept(vec!["f0".into()])).unwrap();
                }
                Event::Received { path, .. } => return std::fs::read(path).unwrap(),
                _ => {}
            }
        }
        Vec::new()
    });
    // Exactly what web_page.html sends.
    let prep = format!(
        r#"{{"info":{{"alias":"Safari (web)","version":"2.2","deviceModel":"Safari","deviceType":"web","fingerprint":"{}","download":false}},"files":{{"f0":{{"id":"f0","fileName":"IMG_0001.HEIC","size":5,"fileType":""}}}}}}"#,
        "ab".repeat(32)
    );
    let a = rx.addr();
    let r = get(
        a,
        "POST",
        "/api/localsend/v2/prepare-upload",
        prep.as_bytes(),
    )
    .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let ans: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    let q = format!(
        "/api/localsend/v2/upload?sessionId={}&fileId=f0&token={}",
        ans["sessionId"].as_str().unwrap(),
        ans["files"]["f0"].as_str().unwrap()
    );
    assert_eq!(get(a, "POST", &q, b"hello").await.status, 200);
    assert_eq!(user.await.unwrap(), b"hello");
    let _ = std::fs::remove_dir_all(dir);
}

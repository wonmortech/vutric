//! R8 cloud handoff (ADR-0072) against an in-process fake of an S3 bucket (Docker/MinIO is not on the build
//! machines): presigned PUT / GET / DELETE, the storage only ever sees ciphertext, the receiver gets the file back
//! with its name, a damaged object leaves no file behind, and the handoff refuses to run while it is off.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::TcpListener;
use vutric_core::cloud::{CloudSettings, Credentials};
use vutric_net::cloud;

type Bucket = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// One request per connection, `Connection: close`. Accepts a `Content-Length` or a chunked body.
async fn fake_s3() -> (String, Bucket) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let bucket: Bucket = Arc::default();
    let b = bucket.clone();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                return;
            };
            let b = b.clone();
            tokio::spawn(async move {
                let mut r = BufReader::new(sock);
                let mut line = String::new();
                if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_owned();
                let target = parts.next().unwrap_or_default().to_owned();
                let mut len = 0usize;
                let mut chunked = false;
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).await.unwrap();
                    let h = h.trim_end();
                    if h.is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                        chunked = true;
                    }
                }
                let mut body = Vec::new();
                if chunked {
                    loop {
                        let mut size = String::new();
                        r.read_line(&mut size).await.unwrap();
                        let n = usize::from_str_radix(size.trim(), 16).unwrap();
                        let mut piece = vec![0u8; n + 2];
                        r.read_exact(&mut piece).await.unwrap();
                        if n == 0 {
                            break;
                        }
                        body.extend_from_slice(&piece[..n]);
                    }
                } else {
                    body.resize(len, 0);
                    r.read_exact(&mut body).await.unwrap();
                }
                let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                let signed = query.contains("X-Amz-Signature=")
                    && query.contains("X-Amz-Expires=")
                    && query.contains("X-Amz-Credential=AKTEST%2F");
                let (status, out) = if !signed {
                    ("403 Forbidden", Vec::new())
                } else {
                    let mut map = b.lock().unwrap();
                    match method.as_str() {
                        "PUT" => {
                            map.insert(path.to_owned(), body);
                            ("200 OK", Vec::new())
                        }
                        "GET" => match map.get(path) {
                            Some(v) => ("200 OK", v.clone()),
                            None => ("404 Not Found", Vec::new()),
                        },
                        "DELETE" => {
                            map.remove(path);
                            ("204 No Content", Vec::new())
                        }
                        _ => ("405 Method Not Allowed", Vec::new()),
                    }
                };
                let mut sock = r.into_inner();
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    out.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&out).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (url, bucket)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vutric-cloud-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn settings(endpoint: &str) -> CloudSettings {
    CloudSettings {
        enabled: true,
        endpoint: endpoint.into(),
        bucket: "vutric-test".into(),
        path_style: true,
        ..CloudSettings::default()
    }
}

fn creds() -> Credentials {
    Credentials {
        access_key: "AKTEST".into(),
        secret_key: "not-a-real-secret".into(),
    }
}

const NOW: u64 = 1_795_000_000;

#[tokio::test(flavor = "multi_thread")]
async fn upload_download_delete_through_a_bucket_that_sees_only_ciphertext() {
    let (endpoint, bucket) = fake_s3().await;
    let http = cloud::client().unwrap();
    let from = scratch("from");
    let to = scratch("to");
    let data: Vec<u8> = (0..200_000u32)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    let path = from.join("hisobot-2026.bin");
    std::fs::write(&path, &data).unwrap();

    let up = cloud::upload(
        &http,
        &settings(&endpoint),
        &creds(),
        &path,
        "application/octet-stream",
        NOW,
    )
    .await
    .expect("upload");
    assert_eq!(up.link.expires, NOW + 24 * 3600);
    {
        let map = bucket.lock().unwrap();
        assert_eq!(map.len(), 1);
        let stored = map.values().next().unwrap();
        assert_eq!(stored.len() as u64, up.link.size);
        assert!(
            !stored.windows(32).any(|w| w == &data[1000..1032]),
            "plaintext in the bucket"
        );
        assert!(
            !stored.windows(9).any(|w| w == b"hisobot-2"),
            "file name in the bucket"
        );
        assert!(
            map.keys()
                .next()
                .unwrap()
                .starts_with("/vutric-test/vutric/")
        );
    }

    let uri = up.link.to_uri();
    let got = cloud::download(&http, &uri, &to, 1 << 30, NOW + 60)
        .await
        .expect("download");
    assert_eq!(got.file_name().unwrap(), "hisobot-2026.bin");
    assert_eq!(std::fs::read(&got).unwrap(), data);

    assert!(
        cloud::download(&http, &uri, &to, 1 << 30, NOW + 25 * 3600)
            .await
            .is_err(),
        "an expired link is refused before any request"
    );
    assert!(
        cloud::download(&http, &uri, &to, 100, NOW + 60)
            .await
            .is_err(),
        "bigger than the receiver allows"
    );

    cloud::delete(&http, &settings(&endpoint), &creds(), &up.object, NOW + 120)
        .await
        .expect("delete");
    assert!(bucket.lock().unwrap().is_empty());
    assert!(
        cloud::download(&http, &uri, &to, 1 << 30, NOW + 180)
            .await
            .is_err()
    );

    let _ = std::fs::remove_dir_all(&from);
    let _ = std::fs::remove_dir_all(&to);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_object_leaves_no_file_and_off_means_off() {
    let (endpoint, bucket) = fake_s3().await;
    let http = cloud::client().unwrap();
    let from = scratch("dmg-from");
    let to = scratch("dmg-to");
    let path = from.join("a.txt");
    std::fs::write(&path, vec![7u8; 70_000]).unwrap();

    let off = CloudSettings {
        enabled: false,
        ..settings(&endpoint)
    };
    assert!(
        cloud::upload(&http, &off, &creds(), &path, "text/plain", NOW)
            .await
            .is_err()
    );
    assert!(
        bucket.lock().unwrap().is_empty(),
        "nothing uploaded while off"
    );

    let small = CloudSettings {
        max_bytes: 1000,
        ..settings(&endpoint)
    };
    assert!(
        cloud::upload(&http, &small, &creds(), &path, "text/plain", NOW)
            .await
            .is_err()
    );

    let up = cloud::upload(
        &http,
        &settings(&endpoint),
        &creds(),
        &path,
        "text/plain",
        NOW,
    )
    .await
    .unwrap();
    for v in bucket.lock().unwrap().values_mut() {
        let at = v.len() - 5;
        v[at] ^= 0x40;
    }
    assert!(
        cloud::download(&http, &up.link.to_uri(), &to, 1 << 30, NOW + 1)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_dir(&to).unwrap().count(),
        0,
        "no file and no part file left"
    );

    let _ = std::fs::remove_dir_all(&from);
    let _ = std::fs::remove_dir_all(&to);
}

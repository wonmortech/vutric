//! Encrypted cloud handoff, opt-in (backlog R8, ADR-0072): upload a file sealed on this device to the user's
//! S3-compatible storage and return a [`CloudLink`]; download and open one. The format, the link and the request
//! signing are `vutric_core::cloud`; this is only HTTP and files. Off unless `Settings.cloud.enabled`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result, bail, ensure};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use vutric_core::cloud::{
    CHUNK_BYTES, CloudLink, CloudSettings, ContentKey, Credentials, Meta, Object, Opener, Sealer,
    amz_date, presign, sealed_len,
};

/// An HTTP client that checks certificates with the OS (ring for the crypto, as iroh). No global state.
pub fn client() -> Result<reqwest::Client> {
    use rustls_platform_verifier::BuilderVerifierExt as _;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .user_agent("vutric")
        .build()?)
}

fn random32() -> [u8; 32] {
    iroh::SecretKey::generate().to_bytes()
}

/// What an upload left in the bucket, so the sender can delete it early.
#[derive(Debug, Clone)]
pub struct Uploaded {
    pub link: CloudLink,
    /// The object's key in the bucket (random; says nothing about the file).
    pub object: String,
}

fn object<'a>(s: &'a CloudSettings, key: &'a str) -> Object<'a> {
    Object {
        endpoint: s.endpoint.trim_end_matches('/'),
        region: &s.region,
        bucket: &s.bucket,
        key,
        path_style: s.path_style,
    }
}

/// The upload body: the sealed header first, then each sealed chunk as the file is read.
struct Body {
    head: Option<Vec<u8>>,
    file: tokio::fs::File,
    sealer: Sealer,
    left: u64,
}

async fn next_piece(mut b: Body) -> Option<(std::io::Result<Vec<u8>>, Body)> {
    if let Some(h) = b.head.take() {
        return Some((Ok(h), b));
    }
    if b.sealer.is_done() {
        return None;
    }
    let n = usize::try_from(b.left.min(CHUNK_BYTES as u64)).expect("<= 64 KiB");
    let mut buf = vec![0u8; n];
    if let Err(e) = b.file.read_exact(&mut buf).await {
        return Some((Err(e), b));
    }
    b.left -= n as u64;
    let out = b.sealer.chunk(&buf).map_err(std::io::Error::other);
    Some((out, b))
}

/// Seals `path` and uploads it with one presigned `PUT`, then returns a link whose download URL and expiry are
/// `settings.expiry_hours` after `now` (Unix seconds). Refuses when the handoff is off or the file is too big.
pub async fn upload(
    http: &reqwest::Client,
    settings: &CloudSettings,
    creds: &Credentials,
    path: &Path,
    mime: &str,
    now: u64,
) -> Result<Uploaded> {
    let settings = settings.clone().clamped();
    ensure!(settings.ready(), "cloud handoff is off or not set up");
    let file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?;
    let len = file.metadata().await?.len();
    ensure!(
        len <= settings.max_bytes,
        "file is bigger than the cloud limit"
    );
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let key = ContentKey(random32());
    let (sealer, head) = Sealer::start(
        key.clone(),
        len,
        &Meta {
            name,
            mime: mime.into(),
        },
    )?;
    let object_key = format!("vutric/{}.vtc", &blake3::hash(&random32()).to_hex()[..32]);
    let obj = object(&settings, &object_key);
    let date = amz_date(now);
    let size = sealed_len(len);
    let body = n0_future::stream::unfold(
        Body {
            head: Some(head),
            file,
            sealer,
            left: len,
        },
        next_piece,
    );
    let resp = http
        .put(presign("PUT", &obj, creds, &date, 900))
        .header(reqwest::header::CONTENT_LENGTH, size)
        .body(reqwest::Body::wrap_stream(body))
        .send()
        .await
        .context("upload")?;
    ensure!(
        resp.status().is_success(),
        "storage refused the upload: {}",
        resp.status()
    );
    let expires_secs = u64::from(settings.expiry_hours) * 3600;
    Ok(Uploaded {
        link: CloudLink {
            url: presign("GET", &obj, creds, &date, expires_secs),
            expires: now + expires_secs,
            size,
            key,
        },
        object: object_key,
    })
}

/// Deletes an uploaded object before it expires (after the receiver said it has the file, or on cancel).
pub async fn delete(
    http: &reqwest::Client,
    settings: &CloudSettings,
    creds: &Credentials,
    object_key: &str,
    now: u64,
) -> Result<()> {
    let url = presign(
        "DELETE",
        &object(settings, object_key),
        creds,
        &amz_date(now),
        900,
    );
    let resp = http.delete(url).send().await.context("delete")?;
    ensure!(
        resp.status().is_success(),
        "storage refused the delete: {}",
        resp.status()
    );
    Ok(())
}

/// Downloads a cloud link into `dir`, decrypting as it goes. The name comes from inside the ciphertext and is
/// sanitized; the file appears only once every chunk is verified (until then it is `<name>.vutric-part`).
/// `limit` is the largest file accepted; `now` refuses an expired link.
pub async fn download(
    http: &reqwest::Client,
    uri: &str,
    dir: &Path,
    limit: u64,
    now: u64,
) -> Result<PathBuf> {
    let link = CloudLink::parse(uri, now)?;
    ensure!(
        link.size <= sealed_len(limit),
        "file is bigger than allowed"
    );
    let mut resp = http.get(&link.url).send().await.context("download")?;
    ensure!(
        resp.status().is_success(),
        "storage refused the download: {}",
        resp.status()
    );
    let mut opener = Opener::new(link.key.clone(), limit);
    let part = dir.join(format!(
        "{}.vutric-part",
        &blake3::hash(link.url.as_bytes()).to_hex()[..16]
    ));
    let mut out = tokio::fs::File::create(&part).await?;
    let mut got = 0u64;
    let result: Result<()> = async {
        while let Some(bytes) = resp.chunk().await.context("download")? {
            got += bytes.len() as u64;
            ensure!(got <= link.size, "more bytes than the link says");
            let clear = opener.push(&bytes)?;
            out.write_all(&clear).await?;
        }
        ensure!(opener.is_done(), "download ended early");
        out.flush().await?;
        Ok(())
    }
    .await;
    drop(out);
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    let Some(meta) = opener.meta() else {
        bail!("no metadata");
    };
    let name = vutric_core::filename::unique(&vutric_core::filename::sanitize(&meta.name), |n| {
        dir.join(n).exists()
    });
    let dest = dir.join(name);
    tokio::fs::rename(&part, &dest).await?;
    Ok(dest)
}

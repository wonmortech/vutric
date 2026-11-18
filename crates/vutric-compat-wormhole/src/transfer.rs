//! The `wormhole send` / `wormhole receive` application protocol (file-transfer-protocol.md): a text, a file, or
//! a directory sent as a zip, which we keep as a `.zip` file and never unpack.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2_10::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::mailbox::hex;
use crate::transit::{self, CHUNK, Hints, Records, Role, Transit};
use crate::wormhole::Wormhole;

/// The application id of the reference `wormhole` tool; using it is what makes us interoperable.
pub const APPID: &str = "lothar.com/wormhole/text-or-file-xfer";
/// The public mailbox server of the reference tool.
pub const DEFAULT_MAILBOX: &str = "ws://relay.magic-wormhole.io:4000/v1";
/// The public transit relay of the reference tool.
pub const DEFAULT_RELAY: &str = "tcp:transit.magic-wormhole.io:4001";
/// Longest text kept from an offer.
pub const MAX_TEXT: usize = 64 * 1024;
/// How long the other side may take to answer an offer (its user is deciding).
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(600);

/// Where the servers are and which address the transit listens on.
#[derive(Debug, Clone)]
pub struct Config {
    pub mailbox: String,
    /// `tcp:host:port`, or `None` for direct connections only.
    pub relay: Option<String>,
    pub bind: IpAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mailbox: DEFAULT_MAILBOX.into(),
            relay: Some(DEFAULT_RELAY.into()),
            bind: IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
        }
    }
}

impl Config {
    fn relay(&self) -> Option<(String, u16)> {
        self.relay.as_deref().and_then(crate::transit::parse_relay)
    }
}

/// What the other side offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Text(String),
    /// A file, or a directory as one zip (`name` then ends in `.zip`). `name` is already sanitized.
    File {
        name: String,
        size: u64,
    },
}

/// Parses an `offer` value. Names go through [`vutric_core::filename::sanitize`]; texts are cut to
/// [`MAX_TEXT`] bytes on a character boundary.
#[must_use]
pub fn parse_offer(offer: &Value) -> Option<Offer> {
    if let Some(m) = offer.get("message").and_then(Value::as_str) {
        let mut end = m.len().min(MAX_TEXT);
        while !m.is_char_boundary(end) {
            end -= 1;
        }
        return Some(Offer::Text(m[..end].to_owned()));
    }
    if let Some(f) = offer.get("file") {
        let name = f.get("filename")?.as_str()?;
        let size = f.get("filesize")?.as_u64()?;
        return Some(Offer::File {
            name: vutric_core::filename::sanitize(name),
            size,
        });
    }
    if let Some(d) = offer.get("directory") {
        if d.get("mode")?.as_str()? != "zipfile/deflated" {
            return None;
        }
        let name = d.get("dirname")?.as_str()?;
        let size = d.get("zipsize")?.as_u64()?;
        return Some(Offer::File {
            name: vutric_core::filename::sanitize(&format!("{name}.zip")),
            size,
        });
    }
    None
}

/// Waits for the message holding `key`, keeping the peer's transit hints when they come first. An `error`
/// message from the peer ends the wait with that error.
async fn wait_for(
    w: &mut Wormhole,
    key: &str,
    hints: &mut Option<Hints>,
    wait: Duration,
) -> Result<Value> {
    loop {
        let msg = w.recv(wait).await?;
        if let Some(e) = msg.get("error") {
            bail!(
                "the other side refused: {}",
                e.as_str()
                    .unwrap_or("error")
                    .chars()
                    .take(200)
                    .collect::<String>()
            );
        }
        if let Some(t) = msg.get("transit") {
            *hints = Some(Hints::parse(t).ok_or_else(|| anyhow!("malformed transit hints"))?);
        }
        if let Some(v) = msg.get(key) {
            return Ok(v.clone());
        }
    }
}

/// Sends a text through an open wormhole and waits for the other side's acknowledgement.
pub async fn send_text(w: &mut Wormhole, text: &str) -> Result<()> {
    w.send(&json!({"offer": {"message": text}})).await?;
    let mut hints = None;
    let answer = wait_for(w, "answer", &mut hints, ANSWER_TIMEOUT).await?;
    if answer.get("message_ack").and_then(Value::as_str) != Some("ok") {
        bail!("the other side did not take the text");
    }
    Ok(())
}

/// Sends one file through an open wormhole: offer, wait for `file_ack`, transit, records, then the receiver's
/// SHA-256 acknowledgement. Calls `on(sent_bytes)` as it goes.
pub async fn send_file(
    w: &mut Wormhole,
    path: &Path,
    cfg: &Config,
    on: &(dyn Fn(u64) + Sync),
) -> Result<u64> {
    let size = tokio::fs::metadata(path).await?.len();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("file name"))?
        .to_owned();
    let transit = Transit::new(
        w.transit_key(),
        Role::Sender,
        &transit::transit_side(),
        cfg.bind,
        cfg.relay(),
    )
    .await?;
    w.send(&json!({"transit": transit.ours.to_json()})).await?;
    w.send(&json!({"offer": {"file": {"filename": name, "filesize": size}}}))
        .await?;
    let mut hints = None;
    let answer = wait_for(w, "answer", &mut hints, ANSWER_TIMEOUT).await?;
    if answer.get("file_ack").and_then(Value::as_str) != Some("ok") {
        bail!("the other side did not accept the file");
    }
    let mut rec = transit.connect(&hints.unwrap_or_default()).await?;
    let mut f = tokio::fs::File::open(path).await?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut sent = 0u64;
    while sent < size {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            bail!("the file got shorter while sending");
        }
        hash.update(&buf[..n]);
        rec.send(&buf[..n]).await?;
        sent += n as u64;
        on(sent);
    }
    let ack: Value =
        serde_json::from_slice(&tokio::time::timeout(Duration::from_secs(60), rec.recv()).await??)
            .map_err(|_| anyhow!("malformed acknowledgement"))?;
    rec.close().await;
    if ack.get("ack").and_then(Value::as_str) != Some("ok") {
        bail!("the other side did not confirm the file");
    }
    if let Some(theirs) = ack.get("sha256").and_then(Value::as_str)
        && !theirs.eq_ignore_ascii_case(&hex(&hash.finalize()))
    {
        bail!("the other side got different bytes (sha256 mismatch)");
    }
    Ok(sent)
}

/// Reads the other side's offer (keeping its transit hints).
pub async fn receive_offer(w: &mut Wormhole) -> Result<(Offer, Option<Hints>)> {
    let mut hints = None;
    let offer = wait_for(w, "offer", &mut hints, Duration::from_secs(120)).await?;
    let offer = parse_offer(&offer).ok_or_else(|| anyhow!("unsupported offer"))?;
    Ok((offer, hints))
}

/// Acknowledges a text offer.
pub async fn accept_text(w: &mut Wormhole) -> Result<()> {
    w.send(&json!({"answer": {"message_ack": "ok"}})).await
}

/// Refuses an offer.
pub async fn reject(w: &mut Wormhole) -> Result<()> {
    w.send(&json!({"error": "transfer rejected"})).await
}

/// Accepts a file offer and receives it into `part` (truncated first). Returns the bytes written; the caller
/// renames the part file into place. `on(received)` is called as bytes arrive.
pub async fn accept_file(
    w: &mut Wormhole,
    size: u64,
    peer: Option<Hints>,
    part: &Path,
    cfg: &Config,
    on: &(dyn Fn(u64) + Sync),
) -> Result<u64> {
    let transit = Transit::new(
        w.transit_key(),
        Role::Receiver,
        &transit::transit_side(),
        cfg.bind,
        cfg.relay(),
    )
    .await?;
    w.send(&json!({"transit": transit.ours.to_json()})).await?;
    w.send(&json!({"answer": {"file_ack": "ok"}})).await?;
    let mut rec: Records = transit.connect(&peer.unwrap_or_default()).await?;
    let mut f = tokio::fs::File::create(part)
        .await
        .with_context(|| format!("create {}", part.display()))?;
    let mut hash = Sha256::new();
    let mut got = 0u64;
    while got < size {
        let r = tokio::time::timeout(Duration::from_secs(60), rec.recv())
            .await
            .map_err(|_| anyhow!("the sender went quiet"))??;
        if got + r.len() as u64 > size {
            bail!("the sender sent more than it offered");
        }
        hash.update(&r);
        f.write_all(&r).await?;
        got += r.len() as u64;
        on(got);
    }
    f.flush().await?;
    drop(f);
    let ack = json!({"ack": "ok", "sha256": hex(&hash.finalize())});
    rec.send(&serde_json::to_vec(&ack)?).await?;
    rec.close().await;
    Ok(got)
}

/// A file name for `name` inside `dir` that does not exist yet (`photo (2).jpg`).
#[must_use]
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    dir.join(vutric_core::filename::unique(name, |n| {
        dir.join(n).exists()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_are_parsed_strictly() {
        assert_eq!(
            parse_offer(&json!({"message": "salom"})),
            Some(Offer::Text("salom".into()))
        );
        assert_eq!(
            parse_offer(&json!({"file": {"filename": "../../etc/passwd", "filesize": 5}})),
            Some(Offer::File {
                name: vutric_core::filename::sanitize("../../etc/passwd"),
                size: 5
            })
        );
        assert_eq!(
            parse_offer(
                &json!({"directory": {"mode": "zipfile/deflated", "dirname": "Trip", "zipsize": 9, "numbytes": 20, "numfiles": 2}})
            ),
            Some(Offer::File {
                name: "Trip.zip".into(),
                size: 9
            })
        );
        assert_eq!(
            parse_offer(&json!({"directory": {"mode": "tar", "dirname": "x", "zipsize": 1}})),
            None
        );
        assert_eq!(
            parse_offer(&json!({"file": {"filename": "a", "filesize": -1}})),
            None
        );
        assert_eq!(parse_offer(&json!({"nothing": 1})), None);
        let long = "ʻ".repeat(MAX_TEXT);
        match parse_offer(&json!({"message": long})) {
            Some(Offer::Text(t)) => assert!(t.len() <= MAX_TEXT),
            other => panic!("{other:?}"),
        }
    }
}

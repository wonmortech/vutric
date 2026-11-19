//! The mailbox server connection (server-protocol.md): one WebSocket, JSON messages in binary frames.
//!
//! Everything the server sends is untrusted: frames are capped before they are buffered ([`MAX_FRAME`]), only
//! the keys we use are read, every string is bounded, and bodies must be hex of bounded length. Unknown message
//! types are ignored, as the spec asks.

use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Largest WebSocket message read from the server.
pub const MAX_FRAME: usize = 1 << 20;
/// Largest decoded message body (a hex body is twice this).
pub const MAX_BODY: usize = 256 * 1024;
/// Longest side, phase, nameplate or mailbox id kept.
pub const MAX_ID: usize = 128;
/// Longest wait for any single server answer.
pub const SERVER_TIMEOUT: Duration = Duration::from_secs(20);

/// One message from the server, reduced to what a client uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMsg {
    Welcome {
        motd: Option<String>,
        error: Option<String>,
        /// The server lets clients in without a proof of work.
        open: bool,
    },
    Allocated {
        nameplate: String,
    },
    Claimed {
        mailbox: String,
    },
    Released,
    Message {
        side: String,
        phase: String,
        body: Vec<u8>,
    },
    Closed,
    Error {
        error: String,
    },
    /// `ack`, `pong`, `nameplates` and unknown types.
    Other,
}

fn bounded(v: &Value, key: &str, max: usize) -> Option<String> {
    let s = v.get(key)?.as_str()?;
    (s.len() <= max && !s.chars().any(char::is_control)).then(|| s.to_owned())
}

fn hex_body(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || s.len() > MAX_BODY * 2 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Parses one server message. `None` for anything malformed (the caller drops the connection).
#[must_use]
pub fn parse(frame: &[u8]) -> Option<ServerMsg> {
    if frame.len() > MAX_FRAME {
        return None;
    }
    let v: Value = serde_json::from_slice(frame).ok()?;
    let ty = v.get("type")?.as_str()?;
    Some(match ty {
        "welcome" => {
            let w = v.get("welcome").filter(|w| w.is_object())?;
            let open = match w.get("permission-required") {
                None => true,
                Some(p) => p.get("none").is_some(),
            };
            ServerMsg::Welcome {
                motd: bounded(w, "motd", 1024),
                error: bounded(w, "error", 1024),
                open,
            }
        }
        "allocated" => ServerMsg::Allocated {
            nameplate: bounded(&v, "nameplate", MAX_ID)
                .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))?,
        },
        "claimed" => ServerMsg::Claimed {
            mailbox: bounded(&v, "mailbox", MAX_ID).filter(|m| !m.is_empty())?,
        },
        "released" => ServerMsg::Released,
        "message" => ServerMsg::Message {
            side: bounded(&v, "side", MAX_ID)?,
            phase: match v.get("phase")? {
                Value::String(s) if s.len() <= MAX_ID => s.clone(),
                Value::Number(n) => n.as_u64()?.to_string(),
                _ => return None,
            },
            body: hex_body(v.get("body")?.as_str()?)?,
        },
        "closed" => ServerMsg::Closed,
        "error" => ServerMsg::Error {
            error: bounded(&v, "error", 1024).unwrap_or_else(|| "unknown error".into()),
        },
        _ => ServerMsg::Other,
    })
}

/// A connection to the mailbox server, bound to one application id and one side.
#[derive(Debug)]
pub struct Mailbox {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub side: String,
    /// The server's message of the day, to show the user.
    pub motd: Option<String>,
}

impl Mailbox {
    /// Connects to `url` (`ws://…/v1`), reads `welcome` and binds `appid` with a fresh random side.
    pub async fn connect(url: &str, appid: &str) -> Result<Self> {
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_FRAME))
            .max_frame_size(Some(MAX_FRAME));
        let (ws, _) = tokio::time::timeout(
            SERVER_TIMEOUT,
            tokio_tungstenite::connect_async_with_config(url, Some(config), false),
        )
        .await
        .map_err(|_| anyhow!("mailbox server did not answer"))?
        .context("connect to the mailbox server")?;
        let mut me = Self {
            ws,
            side: random_side(),
            motd: None,
        };
        match me.next().await? {
            ServerMsg::Welcome { error: Some(e), .. } => bail!("mailbox server refused: {e}"),
            ServerMsg::Welcome { open: false, .. } => {
                bail!("mailbox server asks for a proof of work, which is not supported")
            }
            ServerMsg::Welcome { motd, .. } => me.motd = motd,
            other => bail!("expected welcome, got {other:?}"),
        }
        let side = me.side.clone();
        me.send(json!({"type": "bind", "appid": appid, "side": side}))
            .await?;
        Ok(me)
    }

    pub async fn send(&mut self, mut msg: Value) -> Result<()> {
        msg["id"] = json!(random_side());
        self.ws
            .send(Ws::Binary(serde_json::to_vec(&msg)?.into()))
            .await
            .context("mailbox send")
    }

    /// The next server message that is not an `ack`/`pong`/unknown one, within [`SERVER_TIMEOUT`] when
    /// `bounded` is true. A server `error` becomes an error here.
    pub async fn next_within(&mut self, wait: Option<Duration>) -> Result<ServerMsg> {
        loop {
            let frame = match wait {
                Some(w) => tokio::time::timeout(w, self.ws.next())
                    .await
                    .map_err(|_| anyhow!("mailbox server went quiet"))?,
                None => self.ws.next().await,
            };
            let data = match frame.ok_or_else(|| anyhow!("mailbox server closed"))?? {
                Ws::Binary(b) => b.to_vec(),
                Ws::Text(t) => t.as_bytes().to_vec(),
                Ws::Close(_) => bail!("mailbox server closed"),
                _ => continue,
            };
            match parse(&data)
                .ok_or_else(|| anyhow!("malformed message from the mailbox server"))?
            {
                ServerMsg::Other => {}
                ServerMsg::Error { error } => bail!("mailbox server error: {error}"),
                m => return Ok(m),
            }
        }
    }

    pub async fn next(&mut self) -> Result<ServerMsg> {
        self.next_within(Some(SERVER_TIMEOUT)).await
    }

    pub async fn allocate(&mut self) -> Result<String> {
        self.send(json!({"type": "allocate"})).await?;
        loop {
            if let ServerMsg::Allocated { nameplate } = self.next().await? {
                return Ok(nameplate);
            }
        }
    }

    pub async fn claim(&mut self, nameplate: &str) -> Result<String> {
        self.send(json!({"type": "claim", "nameplate": nameplate}))
            .await?;
        loop {
            if let ServerMsg::Claimed { mailbox } = self.next().await? {
                return Ok(mailbox);
            }
        }
    }

    pub async fn open(&mut self, mailbox: &str) -> Result<()> {
        self.send(json!({"type": "open", "mailbox": mailbox})).await
    }

    pub async fn add(&mut self, phase: &str, body: &[u8]) -> Result<()> {
        self.send(json!({"type": "add", "phase": phase, "body": hex(body)}))
            .await
    }

    /// Releases the nameplate without waiting for the answer (it may arrive interleaved with messages).
    pub async fn release(&mut self, nameplate: &str) -> Result<()> {
        self.send(json!({"type": "release", "nameplate": nameplate}))
            .await
    }

    /// Closes the mailbox with a mood (`happy`, `lonely`, `scary`, `errory`) and the WebSocket.
    pub async fn close(mut self, mailbox: &str, mood: &str) {
        let _ = self
            .send(json!({"type": "close", "mailbox": mailbox, "mood": mood}))
            .await;
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while let Ok(m) = self.next().await {
                if m == ServerMsg::Closed {
                    break;
                }
            }
        })
        .await;
        let _ = self.ws.close(None).await;
    }
}

/// Five random bytes as hex, the length the reference client uses for sides.
#[must_use]
pub fn random_side() -> String {
    use ring::rand::SecureRandom as _;
    let mut b = [0u8; 5];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .expect("OS random generator");
    hex(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_we_use_and_refuses_the_rest() {
        assert_eq!(
            parse(br#"{"type":"welcome","welcome":{"motd":"hi"}}"#),
            Some(ServerMsg::Welcome {
                motd: Some("hi".into()),
                error: None,
                open: true
            })
        );
        assert!(matches!(
            parse(br#"{"type":"welcome","welcome":{"permission-required":{"hashcash":{}}}}"#),
            Some(ServerMsg::Welcome { open: false, .. })
        ));
        assert_eq!(
            parse(br#"{"type":"message","side":"ab","phase":"pake","body":"00ff","id":"x"}"#),
            Some(ServerMsg::Message {
                side: "ab".into(),
                phase: "pake".into(),
                body: vec![0, 255]
            })
        );
        assert_eq!(
            parse(br#"{"type":"message","side":"ab","phase":3,"body":""}"#),
            Some(ServerMsg::Message {
                side: "ab".into(),
                phase: "3".into(),
                body: vec![]
            })
        );
        assert_eq!(parse(br#"{"type":"ack"}"#), Some(ServerMsg::Other));
        for bad in [
            &br#"{"type":"message","side":"ab","phase":"p","body":"0"}"#[..],
            br#"{"type":"message","side":"ab","phase":"p","body":"zz"}"#,
            br#"{"type":"allocated","nameplate":"1a"}"#,
            br#"{"type":"claimed","mailbox":""}"#,
            br#"{"type":"welcome"}"#,
            br"[]",
            b"not json",
        ] {
            assert_eq!(parse(bad), None, "{}", String::from_utf8_lossy(bad));
        }
        let long = format!(
            r#"{{"type":"message","side":"a","phase":"p","body":"{}"}}"#,
            "00".repeat(MAX_BODY + 1)
        );
        assert_eq!(parse(long.as_bytes()), None);
    }

    #[test]
    fn sides_are_hex_and_differ() {
        let (a, b) = (random_side(), random_side());
        assert_eq!(a.len(), 10);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}

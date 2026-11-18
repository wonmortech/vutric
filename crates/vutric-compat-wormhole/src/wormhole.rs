//! The client-to-client protocol over the mailbox (client-protocol.md): `pake`, `version`, then numbered,
//! encrypted application messages delivered in order, once each.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::code::Code;
use crate::crypto::{self, Key32, Pake};
use crate::mailbox::{Mailbox, ServerMsg};

/// Longest wait for the other side to show up after the code is known.
pub const PEER_TIMEOUT: Duration = Duration::from_secs(600);
/// Most out-of-order application messages held.
const MAX_HELD: usize = 64;

/// A code that is being shown to the user while we wait for the other side ([`Wormhole::create`]).
#[derive(Debug)]
pub struct Pending {
    mb: Mailbox,
    appid: String,
    pub code: Code,
}

/// An open wormhole: both sides proved they know the code.
#[derive(Debug)]
pub struct Wormhole {
    mb: Mailbox,
    mailbox: String,
    key: Key32,
    peer_side: String,
    next_out: u64,
    next_in: u64,
    held: BTreeMap<u64, Vec<u8>>,
    appid: String,
}

impl Pending {
    /// Waits for the other side and opens the wormhole.
    pub async fn open(self) -> Result<Wormhole> {
        Wormhole::finish(self.mb, &self.code, &self.appid).await
    }
}

impl Wormhole {
    /// Allocates a nameplate and makes a code to show (`7-4821-0937`).
    pub async fn create(url: &str, appid: &str) -> Result<Pending> {
        let mut mb = Mailbox::connect(url, appid).await?;
        let nameplate = mb.allocate().await?;
        Ok(Pending {
            mb,
            appid: appid.to_owned(),
            code: Code::generate(&nameplate),
        })
    }

    /// Joins with a code the user typed.
    pub async fn join(url: &str, appid: &str, code: &Code) -> Result<Self> {
        let mb = Mailbox::connect(url, appid).await?;
        Self::finish(mb, code, appid).await
    }

    async fn finish(mut mb: Mailbox, code: &Code, appid: &str) -> Result<Self> {
        let nameplate = code.nameplate().to_owned();
        let mailbox = mb.claim(&nameplate).await?;
        mb.open(&mailbox).await?;
        let (pake, msg) = Pake::start(code.as_str(), appid);
        let body = serde_json::to_vec(&json!({"pake_v1": crate::mailbox::hex(&msg)}))?;
        mb.add("pake", &body).await?;

        let mut early = Vec::new();
        let (peer_side, peer_pake) = loop {
            match mb.next_within(Some(PEER_TIMEOUT)).await? {
                ServerMsg::Message { side, phase, body } if side != mb.side => {
                    if phase == "pake" {
                        break (side, body);
                    }
                    early.push((side, phase, body));
                    if early.len() > MAX_HELD {
                        bail!("the other side sent too much before its key exchange");
                    }
                }
                _ => {}
            }
        };
        let peer_msg = pake_message(&peer_pake).ok_or_else(|| anyhow!("malformed pake message"))?;
        let Some(key) = pake.finish(&peer_msg) else {
            mb.close(&mailbox, "scary").await;
            bail!("malformed pake message");
        };
        mb.release(&nameplate).await?;
        let version = serde_json::to_vec(&json!({"abilities": [], "app_versions": {}}))?;
        let side = mb.side.clone();
        mb.add(
            "version",
            &crypto::seal(&crypto::phase_key(&key, &side, "version"), &version),
        )
        .await?;

        let mut me = Self {
            mb,
            mailbox,
            key,
            peer_side,
            next_out: 0,
            next_in: 0,
            held: BTreeMap::new(),
            appid: appid.to_owned(),
        };
        // The peer's version is the first proof that it knows the code.
        let mut saw_version = false;
        for (s, phase, body) in early {
            me.take(&s, &phase, &body, &mut saw_version).await?;
        }
        while !saw_version {
            let ServerMsg::Message { side, phase, body } =
                me.mb.next_within(Some(PEER_TIMEOUT)).await?
            else {
                continue;
            };
            me.take(&side, &phase, &body, &mut saw_version).await?;
        }
        Ok(me)
    }

    /// Handles one mailbox message from the peer: checks `version`, holds numbered phases for [`Self::recv`].
    async fn take(
        &mut self,
        side: &str,
        phase: &str,
        body: &[u8],
        saw_version: &mut bool,
    ) -> Result<()> {
        if side == self.mb.side || side != self.peer_side {
            return Ok(());
        }
        let numbered = !phase.is_empty() && phase.bytes().all(|b| b.is_ascii_digit());
        if phase != "version" && !numbered {
            return Ok(());
        }
        let Some((_, plain)) = crypto::open(&crypto::phase_key(&self.key, side, phase), body)
        else {
            let _ = self
                .mb
                .send(json!({"type": "close", "mailbox": self.mailbox, "mood": "scary"}))
                .await;
            bail!(
                "a message from the other side did not decrypt: wrong code, or someone guessed at it"
            );
        };
        if phase == "version" {
            *saw_version = true;
            return Ok(());
        }
        let n: u64 = phase.parse().map_err(|_| anyhow!("phase out of range"))?;
        if n >= self.next_in && !self.held.contains_key(&n) {
            if self.held.len() >= MAX_HELD {
                bail!("too many messages out of order");
            }
            self.held.insert(n, plain);
        }
        Ok(())
    }

    /// Sends one application message (a JSON value).
    pub async fn send(&mut self, msg: &Value) -> Result<()> {
        let phase = self.next_out.to_string();
        self.next_out += 1;
        let side = self.mb.side.clone();
        let boxed = crypto::seal(
            &crypto::phase_key(&self.key, &side, &phase),
            &serde_json::to_vec(msg)?,
        );
        self.mb.add(&phase, &boxed).await
    }

    /// The next application message, in order, as JSON. Waits up to `wait`.
    pub async fn recv(&mut self, wait: Duration) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(plain) = self.held.remove(&self.next_in) {
                self.next_in += 1;
                return serde_json::from_slice(&plain)
                    .map_err(|_| anyhow!("the other side sent malformed JSON"));
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                bail!("the other side went quiet");
            }
            if let ServerMsg::Message { side, phase, body } =
                self.mb.next_within(Some(left)).await?
            {
                let mut v = true;
                self.take(&side, &phase, &body, &mut v).await?;
            }
        }
    }

    /// A subkey of the shared key, e.g. the transit key (`<appid>/transit-key`).
    #[must_use]
    pub fn derive(&self, purpose: &str) -> Key32 {
        crypto::derive(&self.key, purpose.as_bytes())
    }

    /// The transit key for file transfers.
    #[must_use]
    pub fn transit_key(&self) -> Key32 {
        self.derive(&format!("{}/transit-key", self.appid))
    }

    /// The verifier, hex, for users who want to compare it.
    #[must_use]
    pub fn verifier(&self) -> String {
        crate::mailbox::hex(&crypto::verifier(&self.key))
    }

    /// Our side id (also the transit relay side).
    #[must_use]
    pub fn side(&self) -> &str {
        &self.mb.side
    }

    pub async fn close(self, mood: &str) {
        self.mb.close(&self.mailbox, mood).await;
    }
}

/// The SPAKE2 bytes in a `pake` body `{"pake_v1": "<hex>"}`.
#[must_use]
pub fn pake_message(body: &[u8]) -> Option<Vec<u8>> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let h = v.get("pake_v1")?.as_str()?;
    if h.len() > 256 || !h.len().is_multiple_of(2) {
        return None;
    }
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok())
        .collect()
}

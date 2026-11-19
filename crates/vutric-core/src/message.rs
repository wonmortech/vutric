use serde::{Deserialize, Serialize};

use crate::{EffectId, Error, FlingId, PROTOCOL_VERSION, Result};

/// Largest preview the desktop accepts. The sender targets ~30 KB (longest side ≤ 512 px).
pub const MAX_PREVIEW_BYTES: usize = 96 * 1024;

/// Longest file name accepted on the wire, in bytes. The receiver still sanitizes it.
pub const MAX_FILE_NAME_BYTES: usize = 512;

/// Most entries in one [`Message::Offer`]; a bigger selection goes as several offers of one transfer.
pub const MAX_OFFER_ENTRIES: usize = 256;

/// Longest relative path of an offered file (`/`-separated, folders kept), in bytes.
pub const MAX_PATH_BYTES: usize = 1024;

/// Longest MIME type on the wire, in bytes.
pub const MAX_MIME_BYTES: usize = 128;

/// One file of an offer. The bytes follow on a data stream of their own (see `docs/protocol.md`).
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferEntry {
    /// Relative path, `/`-separated: `IMG_2041.jpg` or `Trip/day 1/IMG_2041.jpg`. Hostile until sanitized.
    pub path: String,
    pub size: u64,
    pub mime: String,
}

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    Android,
    Ios,
    Windows,
    Linux,
    MacOs,
}

/// How the item left the phone. The desktop uses it so the entry matches the throw: a hard, angled flick enters
/// fast and tilted; a soft one drifts in.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gesture {
    /// Release speed in logical pixels per second on the phone, always ≥ 0.
    pub speed: f32,
    /// Direction of travel in radians, 0 = straight up, positive = clockwise (towards the right).
    pub angle: f32,
    /// Horizontal release position as a fraction of the phone width, 0.0 (left) ..= 1.0 (right).
    pub exit_x: f32,
}

impl Gesture {
    fn validate(&self) -> Result<()> {
        let finite = self.speed.is_finite() && self.angle.is_finite() && self.exit_x.is_finite();
        if !finite || self.speed < 0.0 || !(0.0..=1.0).contains(&self.exit_x) {
            return Err(Error::Malformed("gesture out of range".into()));
        }
        Ok(())
    }
}

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewFormat {
    Jpeg,
    Webp,
    Png,
}

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub format: PreviewFormat,
    pub width: u16,
    pub height: u16,
    #[serde(with = "serde_bytes_compat")]
    pub bytes: Vec<u8>,
}

/// Opens a fling. Sent on the control stream together with the preview, before any file byte, so the desktop
/// can start the effect immediately.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlingStart {
    pub id: FlingId,
    pub file_name: String,
    pub mime: String,
    pub size: u64,
    /// BLAKE3 of the full file; the receiver verifies it before the file becomes visible in the save folder.
    pub blake3: [u8; 32],
    pub gesture: Gesture,
    pub effect: EffectId,
    pub preview: Option<Preview>,
}

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Message {
    /// First message on every connection, in both directions.
    Hello {
        protocol: u16,
        device_name: String,
        platform: Platform,
    },
    FlingStart(FlingStart),
    /// Receiver → sender: the effect has started (first frame presented). Carries the receiver's monotonic
    /// timestamp so the latency probe can be computed without synchronized clocks.
    EffectStarted {
        id: FlingId,
        receiver_micros: u64,
    },
    /// Receiver → sender: the file arrived and its hash matched.
    FlingComplete {
        id: FlingId,
    },
    /// Either direction: the fling was abandoned.
    FlingFailed {
        id: FlingId,
        reason: FailReason,
    },
    /// Sender → receiver: files on offer. A transfer of more than [`MAX_OFFER_ENTRIES`] files is several
    /// offers with the same `id`; `first` is the transfer-wide index of `entries[0]`, so the receiver asks the
    /// user once (at `first == 0`) and the totals cover the whole transfer.
    Offer {
        id: FlingId,
        first: u32,
        entries: Vec<OfferEntry>,
        total_files: u32,
        total_bytes: u64,
    },
    /// Receiver → sender. `have[i]` is how many leading bytes of `entries[i]` the receiver already holds from an
    /// interrupted transfer; the sender resumes there (and still proves the whole file with its hash).
    OfferReply {
        id: FlingId,
        accept: bool,
        have: Vec<u64>,
    },
    /// Receiver → sender, after the last data stream of one offer: how many of its files arrived whole and
    /// verified, and which (transfer-wide indices) did not.
    OfferDone {
        id: FlingId,
        received: u32,
        failed: Vec<u32>,
    },
    /// Pairing without a QR code (`crate::sas`), dialer → listener: commitment to the dialer's nonce.
    PairCommit {
        commit: [u8; 32],
    },
    /// Listener → dialer: the listener's nonce, sent only after the commitment arrived.
    PairNonce {
        nonce: [u8; 32],
    },
    /// Dialer → listener: the nonce the commitment was made to.
    PairReveal {
        nonce: [u8; 32],
    },
    /// Both ways: whether this side's user confirmed that the six digits match. Trust is stored only when both
    /// sides said yes.
    PairResult {
        confirmed: bool,
    },
    /// Dialer → listener, right after `Hello`, when the dialer used a pairing link with a secret (ADR-0064):
    /// `crate::sas::link_proof` over this session. The listener then trusts the key as `ticket`.
    PairProof {
        proof: [u8; 32],
    },
    /// A phone notification mirrored to a paired device (ADR-0064, backlog A12/F13). Shown, never acted on.
    Notice(Notice),
    /// Text the user chose to send to the other device's clipboard. The receiver offers it ("Nusxalash"), it never
    /// writes the clipboard by itself.
    Clipboard {
        text: String,
    },
    /// "Ring my phone" (ADR-0069, backlog F14): start or stop a loud sound on the other device. Taken only from a
    /// paired device and rate-limited (`crate::ring`).
    Ring(crate::ring::Ring),
    /// To a paired device only, after pairing: this device's key for BLE rotating ids (ADR-0071), so the other
    /// side can recognise it nearby. Taken only from a trusted device (`crate::ble::accepts_key`).
    BleKey {
        key: crate::ble::BleKey,
    },
    /// Phone as mouse and keyboard (ADR-0075, backlog F15): one input event for the desktop. Taken only from a
    /// paired device while the desktop user allows it, rate-limited (`crate::pointer`).
    Pointer(crate::pointer::Pointer),
}

/// Most characters of [`Notice::app`] and [`Notice::title`].
pub const MAX_NOTICE_LINE_CHARS: usize = 80;

/// Most characters of [`Notice::text`].
pub const MAX_NOTICE_TEXT_CHARS: usize = 400;

/// Longest [`Message::Clipboard`] text, in bytes.
pub const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;

/// One mirrored notification. All text is the other device's and hostile until passed through
/// `crate::companion::shown`.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// The app's label on the phone ("Telegram").
    pub app: String,
    pub title: String,
    pub text: String,
    /// When the phone posted it, Unix milliseconds of the phone's clock (display only).
    pub at_ms: u64,
}

impl Message {
    /// The variant's name, for errors and logs (never the payload, which may be large or peer-chosen).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "Hello",
            Self::FlingStart(_) => "FlingStart",
            Self::EffectStarted { .. } => "EffectStarted",
            Self::FlingComplete { .. } => "FlingComplete",
            Self::FlingFailed { .. } => "FlingFailed",
            Self::Offer { .. } => "Offer",
            Self::OfferReply { .. } => "OfferReply",
            Self::OfferDone { .. } => "OfferDone",
            Self::PairCommit { .. } => "PairCommit",
            Self::PairNonce { .. } => "PairNonce",
            Self::PairReveal { .. } => "PairReveal",
            Self::PairResult { .. } => "PairResult",
            Self::PairProof { .. } => "PairProof",
            Self::Notice(_) => "Notice",
            Self::Clipboard { .. } => "Clipboard",
            Self::Ring(_) => "Ring",
            Self::BleKey { .. } => "BleKey",
            Self::Pointer(_) => "Pointer",
        }
    }
}

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailReason {
    HashMismatch,
    DiskFull,
    Rejected,
    Cancelled,
    Paused,
}

impl Message {
    #[must_use]
    pub fn hello(device_name: impl Into<String>, platform: Platform) -> Self {
        Self::Hello {
            protocol: PROTOCOL_VERSION,
            device_name: device_name.into(),
            platform,
        }
    }

    /// Checks invariants serde cannot express. Called by [`crate::frame::decode`] on every inbound message.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Hello { protocol, .. } if *protocol != PROTOCOL_VERSION => {
                Err(Error::UnsupportedVersion(*protocol))
            }
            Self::FlingStart(start) => {
                if start.file_name.is_empty() || start.file_name.len() > MAX_FILE_NAME_BYTES {
                    return Err(Error::Malformed("file name length".into()));
                }
                if let Some(preview) = &start.preview
                    && preview.bytes.len() > MAX_PREVIEW_BYTES
                {
                    return Err(Error::PreviewTooLarge {
                        len: preview.bytes.len(),
                        max: MAX_PREVIEW_BYTES,
                    });
                }
                start.gesture.validate()
            }
            Self::Offer {
                first,
                entries,
                total_files,
                ..
            } => {
                let end = u64::from(*first) + entries.len() as u64;
                if entries.is_empty()
                    || entries.len() > MAX_OFFER_ENTRIES
                    || end > u64::from(*total_files)
                {
                    return Err(Error::Malformed("offer entry count".into()));
                }
                for e in entries {
                    if e.path.is_empty() || e.path.len() > MAX_PATH_BYTES {
                        return Err(Error::Malformed("offer path length".into()));
                    }
                    if e.mime.len() > MAX_MIME_BYTES {
                        return Err(Error::Malformed("offer mime length".into()));
                    }
                }
                Ok(())
            }
            Self::OfferReply { have, .. } if have.len() > MAX_OFFER_ENTRIES => {
                Err(Error::Malformed("offer reply length".into()))
            }
            Self::OfferDone { failed, .. } if failed.len() > MAX_OFFER_ENTRIES => {
                Err(Error::Malformed("offer done length".into()))
            }
            Self::Notice(n) => {
                let over = |s: &str, max| s.chars().nth(max).is_some();
                if n.app.is_empty()
                    || over(&n.app, MAX_NOTICE_LINE_CHARS)
                    || over(&n.title, MAX_NOTICE_LINE_CHARS)
                    || over(&n.text, MAX_NOTICE_TEXT_CHARS)
                {
                    return Err(Error::Malformed("notice length".into()));
                }
                Ok(())
            }
            Self::Clipboard { text } if text.is_empty() || text.len() > MAX_CLIPBOARD_BYTES => {
                Err(Error::Malformed("clipboard length".into()))
            }
            Self::Ring(r) if !r.is_valid() => Err(Error::Malformed("ring seconds".into())),
            Self::Pointer(p) if !p.is_valid() => Err(Error::Malformed("pointer input".into())),
            _ => Ok(()),
        }
    }
}

mod serde_bytes_compat {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        <&'de [u8]>::deserialize(d)
            .map(<[u8]>::to_vec)
            .map_err(|_| serde::de::Error::custom("expected bytes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample_start() -> FlingStart {
        FlingStart {
            id: FlingId([7; 16]),
            file_name: "IMG_2041.jpg".into(),
            mime: "image/jpeg".into(),
            size: 3_481_220,
            blake3: *blake3::hash(b"file").as_bytes(),
            gesture: Gesture {
                speed: 2400.0,
                angle: 0.2,
                exit_x: 0.6,
            },
            effect: EffectId::Halqa,
            preview: Some(Preview {
                format: PreviewFormat::Webp,
                width: 512,
                height: 384,
                bytes: vec![1, 2, 3],
            }),
        }
    }

    #[test]
    fn valid_start_passes() {
        Message::FlingStart(sample_start()).validate().unwrap();
    }

    #[test]
    fn rejects_oversized_preview() {
        let mut start = sample_start();
        start.preview.as_mut().unwrap().bytes = vec![0; MAX_PREVIEW_BYTES + 1];
        assert!(matches!(
            Message::FlingStart(start).validate(),
            Err(Error::PreviewTooLarge { .. })
        ));
    }

    #[test]
    fn rejects_nonsense_gesture() {
        for gesture in [
            Gesture {
                speed: f32::NAN,
                angle: 0.0,
                exit_x: 0.5,
            },
            Gesture {
                speed: -1.0,
                angle: 0.0,
                exit_x: 0.5,
            },
            Gesture {
                speed: 1.0,
                angle: 0.0,
                exit_x: 1.5,
            },
        ] {
            let mut start = sample_start();
            start.gesture = gesture;
            assert!(Message::FlingStart(start).validate().is_err());
        }
    }

    fn entry(path: &str) -> OfferEntry {
        OfferEntry {
            path: path.into(),
            size: 10,
            mime: "image/jpeg".into(),
        }
    }

    #[test]
    fn offer_bounds() {
        let offer = |first, entries: Vec<OfferEntry>, total_files| Message::Offer {
            id: FlingId([1; 16]),
            first,
            entries,
            total_files,
            total_bytes: 0,
        };
        offer(0, vec![entry("a.jpg")], 1).validate().unwrap();
        offer(256, vec![entry("a.jpg")], 257).validate().unwrap();
        assert!(offer(0, vec![], 0).validate().is_err());
        assert!(offer(1, vec![entry("a.jpg")], 1).validate().is_err());
        assert!(offer(0, vec![entry("")], 1).validate().is_err());
        let long = "x".repeat(MAX_PATH_BYTES + 1);
        assert!(offer(0, vec![entry(&long)], 1).validate().is_err());
        let many = vec![entry("a"); MAX_OFFER_ENTRIES + 1];
        assert!(offer(0, many, 1000).validate().is_err());
    }

    #[test]
    fn companion_bounds() {
        let notice = |app: &str, title: &str, text: &str| {
            Message::Notice(Notice {
                app: app.into(),
                title: title.into(),
                text: text.into(),
                at_ms: 1,
            })
        };
        notice("Telegram", "", "").validate().unwrap();
        let line = "ʻ".repeat(MAX_NOTICE_LINE_CHARS);
        let text = "ʻ".repeat(MAX_NOTICE_TEXT_CHARS);
        notice(&line, &line, &text).validate().unwrap();
        assert!(notice("", "t", "x").validate().is_err());
        assert!(notice(&format!("{line}a"), "", "").validate().is_err());
        assert!(notice("a", &format!("{line}a"), "").validate().is_err());
        assert!(notice("a", "", &format!("{text}a")).validate().is_err());
        let clip = |n| Message::Clipboard {
            text: "x".repeat(n),
        };
        clip(1).validate().unwrap();
        clip(MAX_CLIPBOARD_BYTES).validate().unwrap();
        assert!(clip(0).validate().is_err());
        assert!(clip(MAX_CLIPBOARD_BYTES + 1).validate().is_err());
    }

    #[test]
    fn appended_variants_keep_old_wire_indices() {
        // postcard writes the variant index first; the variants added later must not move the older ones.
        let wire = crate::frame::encode(&Message::PairResult { confirmed: true }).unwrap();
        assert_eq!(wire[4], 11);
        let wire = crate::frame::encode(&Message::PairProof { proof: [0; 32] }).unwrap();
        assert_eq!(wire[4], 12);
        let wire = crate::frame::encode(&Message::Clipboard { text: "a".into() }).unwrap();
        assert_eq!(wire[4], 14);
        let wire = crate::frame::encode(&Message::Ring(crate::ring::Ring::Stop)).unwrap();
        assert_eq!(wire[4], 15);
        let wire = crate::frame::encode(&Message::BleKey {
            key: crate::ble::BleKey([0; 16]),
        })
        .unwrap();
        assert_eq!(wire[4], 16);
        let wire =
            crate::frame::encode(&Message::Pointer(crate::pointer::Pointer::ReleaseAll)).unwrap();
        assert_eq!(wire[4], 17);
        assert!(
            Message::Pointer(crate::pointer::Pointer::Key {
                usage: 0x01,
                down: true
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn ring_bounds() {
        use crate::ring::{MAX_RING_SECS, Ring};
        Message::Ring(Ring::Stop).validate().unwrap();
        Message::Ring(Ring::Start {
            secs: MAX_RING_SECS,
        })
        .validate()
        .unwrap();
        assert!(Message::Ring(Ring::Start { secs: 0 }).validate().is_err());
        assert!(
            Message::Ring(Ring::Start {
                secs: MAX_RING_SECS + 1
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_other_protocol_versions() {
        let msg = Message::Hello {
            protocol: 99,
            device_name: "Pixel 8".into(),
            platform: Platform::Android,
        };
        assert_eq!(msg.validate(), Err(Error::UnsupportedVersion(99)));
    }
}

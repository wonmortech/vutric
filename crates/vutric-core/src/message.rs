use serde::{Deserialize, Serialize};

use crate::{EffectId, Error, FlingId, PROTOCOL_VERSION, Result};

/// Largest preview the desktop accepts. The sender targets ~30 KB (longest side ≤ 512 px).
pub const MAX_PREVIEW_BYTES: usize = 96 * 1024;

/// Longest file name accepted on the wire, in bytes. The receiver still sanitizes it.
pub const MAX_FILE_NAME_BYTES: usize = 512;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewFormat {
    Jpeg,
    Webp,
    Png,
}

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
        }
    }
}

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

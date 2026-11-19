//! What the iOS Share Extension and App Intents call through `UniFFI` (ADR-0009 rule 2, ADR-0015): pick a paired
//! device and stream files to it, with no Flutter engine in the process.
//!
//! The extension runs in its own process with its own memory limit, so this side keeps its own small peer: it is
//! bound for one send and closed afterwards, and it never touches the app's session slot in [`crate::api`]. The
//! trust store and settings are the same files the app uses (the App Group folder, set with [`share_open`]), so a
//! device paired in the app is a target here and nothing is trusted that the app did not trust.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use tokio::sync::watch;
use vutric_core::EndpointId;
use vutric_core::code::ShortCode;
use vutric_net::{Peer, Reach, Route, transfer};

use crate::api;

/// A paired device the Share sheet offers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ShareDevice {
    /// The endpoint id, 64 hex.
    pub id: String,
    pub name: String,
    /// `android`, `ios`, `windows`, `linux`, `macos` or "".
    pub platform: String,
    /// `ABCD-EFGH`, to tell two devices with one name apart.
    pub code: String,
    pub last_seen: u64,
}

/// A Bonjour result the extension resolved itself (`NWBrowser` on `_vutric._udp`; Rust cannot multicast on iOS
/// without the multicast entitlement): the service instance name and the addresses it resolved to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NearbyHint {
    /// iroh's mDNS instance name: the endpoint id in lowercase base32 (RFC 4648, no padding).
    pub instance: String,
    /// `ip:port`, as many as resolved.
    pub addrs: Vec<String>,
}

/// Where a send is.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ShareProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u32,
    pub files_total: u32,
    pub current: String,
    pub bytes_per_sec: f64,
}

/// How a send ended.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ShareResult {
    pub files: u32,
    pub bytes: u64,
    /// Names that did not arrive whole.
    pub failed: Vec<String>,
    pub seconds: f64,
    /// `direct`, `relay` or "".
    pub route: String,
}

/// Why a send did not happen. Each case is one line in the extension's String Catalog (`share.error.*`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum ShareError {
    /// [`share_open`] was not called, or the App Group folder is missing.
    NotSetUp,
    /// The id is not a device this phone paired with.
    NotPaired,
    /// The device did not answer (asleep, another network, local network permission off).
    Unreachable { detail: String },
    /// The other side said no.
    Declined,
    /// [`share_cancel`] was called.
    Cancelled,
    /// Over the relay limit (ADR-0068): use a hotspot or a cable.
    TooBig,
    /// A file could not be read, or anything else; `detail` is for the log, not the screen.
    Failed { detail: String },
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSetUp => f.write_str("share: not set up"),
            Self::NotPaired => f.write_str("share: not a paired device"),
            Self::Unreachable { detail } => write!(f, "share: unreachable: {detail}"),
            Self::Declined => f.write_str("share: declined"),
            Self::Cancelled => f.write_str("share: cancelled"),
            Self::TooBig => f.write_str("share: too big for the relay"),
            Self::Failed { detail } => write!(f, "share: {detail}"),
        }
    }
}

impl std::error::Error for ShareError {}

/// Progress for the extension's screen and the Live Activity; implemented in Swift.
#[uniffi::export(with_foreign)]
pub trait ShareListener: Send + Sync {
    fn on_progress(&self, progress: ShareProgress);
}

struct Identity {
    key: [u8; 32],
    name: String,
}

fn identity_slot() -> &'static Mutex<Option<Identity>> {
    static ID: Mutex<Option<Identity>> = Mutex::new(None);
    &ID
}

/// Moves on at every [`share_cancel`]; a send started before returns [`ShareError::Cancelled`].
fn cancel_epoch() -> &'static watch::Sender<u64> {
    static EPOCH: OnceLock<watch::Sender<u64>> = OnceLock::new();
    EPOCH.get_or_init(|| watch::channel(0).0)
}

/// Sets the folder with the app's settings and trust files (the App Group container) and this phone's identity
/// (from the shared Keychain item) and name. Call once before anything else; calling again replaces both.
#[uniffi::export]
pub fn share_open(dir: String, identity: Vec<u8>, name: String) -> Result<(), ShareError> {
    let key: [u8; 32] = identity
        .as_slice()
        .try_into()
        .map_err(|_| ShareError::NotSetUp)?;
    if dir.trim().is_empty() || !std::path::Path::new(&dir).is_dir() {
        return Err(ShareError::NotSetUp);
    }
    api::set_settings_dir(dir);
    let name = if name.trim().is_empty() {
        "iPhone".to_owned()
    } else {
        name
    };
    *identity_slot().lock().expect("identity lock") = Some(Identity { key, name });
    Ok(())
}

/// The paired devices, the one seen most recently first. Empty before [`share_open`].
#[uniffi::export]
#[must_use]
pub fn share_devices() -> Vec<ShareDevice> {
    if identity_slot().lock().expect("identity lock").is_none() {
        return Vec::new();
    }
    let mut out: Vec<ShareDevice> = api::load_trust()
        .iter()
        .filter(|(_, d)| d.method != vutric_net::trust::PairMethod::Pinned)
        .map(|(id, d)| ShareDevice {
            id: id.to_string(),
            name: d.name.clone(),
            platform: d
                .platform
                .map(|p| format!("{p:?}").to_ascii_lowercase())
                .unwrap_or_default(),
            code: ShortCode::of(&id).to_string(),
            last_seen: d.last_seen,
        })
        .collect();
    out.sort_by_key(|d| std::cmp::Reverse(d.last_seen));
    out
}

/// The endpoint id in an iroh mDNS instance name (lowercase base32, RFC 4648, no padding); `None` for any other
/// service on the network.
#[must_use]
pub fn endpoint_of_instance(instance: &str) -> Option<EndpointId> {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let s = instance.trim().to_ascii_lowercase();
    // 32 bytes = 256 bits = 52 symbols, the last one carrying 1 bit plus 4 zero bits.
    if s.len() != 52 {
        return None;
    }
    let mut out = [0u8; 32];
    let (mut acc, mut bits, mut n) = (0u32, 0u32, 0usize);
    for c in s.bytes() {
        let v = ALPHABET.iter().position(|&a| a == c)?;
        acc = (acc << 5) | u32::try_from(v).ok()?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            if n == out.len() {
                return None;
            }
            out[n] = (acc >> bits) as u8;
            n += 1;
            acc &= (1 << bits) - 1;
        }
    }
    (n == out.len() && acc == 0).then_some(EndpointId(out))
}

/// The addresses among `nearby` that belong to `id`.
#[must_use]
pub fn hints_for(id: EndpointId, nearby: &[NearbyHint]) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = nearby
        .iter()
        .filter(|h| endpoint_of_instance(&h.instance) == Some(id))
        .flat_map(|h| h.addrs.iter().filter_map(|a| a.trim().parse().ok()))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn share_error(e: &anyhow::Error) -> ShareError {
    if e.downcast_ref::<transfer::Declined>().is_some() {
        ShareError::Declined
    } else if e.downcast_ref::<transfer::TooBigForRelay>().is_some() {
        ShareError::TooBig
    } else {
        ShareError::Failed {
            detail: format!("{e:#}"),
        }
    }
}

/// Sends `paths` (named `names[i]` on the other side; empty keeps the file's own name) to the paired device `id`,
/// reporting to `listener` about every 150 ms. Blocks until done; call it off the main thread. The files are read
/// as they go and never loaded whole, so a 4 GB video fits the extension's memory.
#[uniffi::export]
pub fn share_send(
    device_id: String,
    nearby: Vec<NearbyHint>,
    paths: Vec<String>,
    names: Vec<String>,
    listener: Arc<dyn ShareListener>,
) -> Result<ShareResult, ShareError> {
    let (key, name) = {
        let slot = identity_slot().lock().expect("identity lock");
        let id = slot.as_ref().ok_or(ShareError::NotSetUp)?;
        (id.key, id.name.clone())
    };
    let id: EndpointId = device_id.parse().map_err(|_| ShareError::NotPaired)?;
    if !api::load_trust().is_trusted(&id) {
        return Err(ShareError::NotPaired);
    }
    let files = paths
        .iter()
        .enumerate()
        .map(|(i, p)| transfer::Outgoing::file(p, names.get(i).cloned()))
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(|e| share_error(&e))?;
    if files.is_empty() {
        return Err(ShareError::Failed {
            detail: "nothing to send".into(),
        });
    }
    let addrs = hints_for(id, &nearby);
    let reach = Reach::for_picker(api::load_settings().route);
    let mut cancelled = cancel_epoch().subscribe();
    cancelled.mark_unchanged();
    let on: transfer::OnProgress = Arc::new(move |p: transfer::Progress| {
        listener.on_progress(ShareProgress {
            bytes_done: p.bytes_done,
            bytes_total: p.bytes_total,
            files_done: p.files_done,
            files_total: p.files_total,
            current: p.current,
            bytes_per_sec: p.bytes_per_sec,
        });
    });
    api::runtime().block_on(async move {
        let job = async {
            let peer = Peer::bind(key, &name, api::PLATFORM, reach)
                .await
                .map_err(|e| ShareError::Unreachable {
                    detail: format!("{e:#}"),
                })?;
            let t0 = Instant::now();
            let r = async {
                let mut s =
                    peer.connect(id, &addrs)
                        .await
                        .map_err(|e| ShareError::Unreachable {
                            detail: format!("{e:#}"),
                        })?;
                let route = match s.route().0 {
                    Route::Direct => "direct",
                    Route::Relay => "relay",
                    Route::Unknown => "",
                };
                let sum = transfer::send(&mut s, files, on)
                    .await
                    .map_err(|e| share_error(&e))?;
                s.close();
                Ok(ShareResult {
                    files: sum.files,
                    bytes: sum.bytes,
                    failed: sum.failed,
                    seconds: t0.elapsed().as_secs_f64(),
                    route: route.into(),
                })
            }
            .await;
            peer.close().await;
            r
        };
        tokio::select! {
            r = job => r,
            _ = cancelled.changed() => Err(ShareError::Cancelled),
        }
    })
}

/// Stops a [`share_send`] in flight (the extension's Cancel button, or the system ending the extension).
#[uniffi::export]
pub fn share_cancel() {
    cancel_epoch().send_modify(|e| *e += 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base32(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
        let (mut out, mut acc, mut bits) = (String::new(), 0u32, 0u32);
        for &b in bytes {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
            }
            acc &= (1 << bits) - 1;
        }
        if bits > 0 {
            out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
        }
        out
    }

    #[test]
    fn instance_names_decode_to_the_endpoint() {
        for seed in [0u8, 1, 0x7f, 0xff] {
            let id = EndpointId([seed; 32]);
            let name = base32(&id.0);
            assert_eq!(name.len(), 52);
            assert_eq!(endpoint_of_instance(&name), Some(id));
            assert_eq!(endpoint_of_instance(&name.to_ascii_uppercase()), Some(id));
        }
        let mut id = [0u8; 32];
        for (i, b) in id.iter_mut().enumerate() {
            *b = u8::try_from(i * 7).unwrap();
        }
        assert_eq!(endpoint_of_instance(&base32(&id)), Some(EndpointId(id)));
    }

    #[test]
    fn other_services_are_not_endpoints() {
        assert_eq!(endpoint_of_instance(""), None);
        assert_eq!(endpoint_of_instance("Living room TV"), None);
        // Right length, a symbol outside the alphabet.
        assert_eq!(endpoint_of_instance(&"1".repeat(52)), None);
        // Right length, but the padding bits are not zero.
        let mut name = base32(&[0u8; 32]);
        name.pop();
        name.push('b');
        assert_eq!(endpoint_of_instance(&name), None);
        assert_eq!(endpoint_of_instance(&"a".repeat(53)), None);
    }

    #[test]
    fn hints_keep_only_the_target_and_parse_addresses() {
        let a = EndpointId([1; 32]);
        let b = EndpointId([2; 32]);
        let nearby = vec![
            NearbyHint {
                instance: base32(&a.0),
                addrs: vec![
                    "192.168.1.24:51000".into(),
                    "garbage".into(),
                    "[fe80::1]:51000".into(),
                ],
            },
            NearbyHint {
                instance: base32(&b.0),
                addrs: vec!["192.168.1.30:51000".into()],
            },
            NearbyHint {
                instance: base32(&a.0),
                addrs: vec!["192.168.1.24:51000".into()],
            },
        ];
        let got = hints_for(a, &nearby);
        assert_eq!(
            got,
            vec![
                "192.168.1.24:51000".parse::<SocketAddr>().unwrap(),
                "[fe80::1]:51000".parse().unwrap(),
            ]
        );
        assert!(hints_for(EndpointId([3; 32]), &nearby).is_empty());
    }

    #[test]
    fn send_before_open_says_not_set_up() {
        struct Quiet;
        impl ShareListener for Quiet {
            fn on_progress(&self, _: ShareProgress) {}
        }
        // The slot is process-wide; only this test reads it before any open.
        if identity_slot().lock().unwrap().is_some() {
            return;
        }
        assert!(share_devices().is_empty());
        let r = share_send(
            EndpointId([9; 32]).to_string(),
            Vec::new(),
            vec!["nope".into()],
            Vec::new(),
            Arc::new(Quiet),
        );
        assert_eq!(r, Err(ShareError::NotSetUp));
    }

    #[test]
    fn open_refuses_a_bad_identity_or_folder() {
        assert_eq!(
            share_open(".".into(), vec![0; 31], "x".into()),
            Err(ShareError::NotSetUp)
        );
        assert_eq!(
            share_open("/no/such/vutric/folder".into(), vec![0; 32], "x".into()),
            Err(ShareError::NotSetUp)
        );
    }
}

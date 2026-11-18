//! A USB cable between the phone and the computer (backlog R5, ADR-0070). Two ways, both found without the user
//! choosing anything:
//!
//! - **USB tethering** (every Android phone; an iPhone's "Personal Hotspot" over USB). The phone becomes a small
//!   router on the cable: the computer gets an interface (Windows `Remote NDIS based Internet Sharing Device` or
//!   `UsbNcm Host Device`, Linux `usb0` / `enx…` with the `rndis_host` / `cdc_ncm` driver, `ipheth` for an
//!   iPhone) whose gateway is the phone. [`usb_links`] lists them; [`find_phone`] asks the gateway's beacon
//!   who it is, so the computer can dial the phone, and the phone dials the computer's cable address from the QR.
//!   [`crate::lan::Medium::Usb`] ranks that address first and the badge says "Kabel".
//! - **ADB reverse** ([`adb`]), for a phone with USB debugging on: `adb reverse` makes the phone's
//!   `127.0.0.1:<port>` reach the computer's. `adb` carries TCP only and iroh speaks QUIC over UDP, so the
//!   computer runs a relay on loopback (`vutric-relay`) and both ends meet through it; packets stay end-to-end
//!   encrypted, and a session through a loopback relay counts as a cable ([`crate::Session::badge`]), not as
//!   "relayed" (no big-file limit).
//!
//! An iPhone over `usbmuxd` (the peertalk approach) is researched only; see ADR-0070.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use crate::beacon::BeaconService;
use crate::find::{self, Found, Target};

/// Which kind of phone link a USB interface is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbKind {
    /// Android USB tethering (RNDIS or NCM gadget).
    Android,
    /// An iPhone's or iPad's USB network link (`ipheth`, "Apple Mobile Device Ethernet").
    Iphone,
}

/// One network interface as the OS describes it, the input of [`links_from`] (so tests can describe a cable that
/// is not there).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IfaceInfo {
    pub index: u32,
    /// The OS name (`usb0`, `{GUID}`), then the friendly name and the description, if any.
    pub names: Vec<String>,
    /// The kernel driver (Linux), when known.
    pub driver: Option<String>,
    pub mac: Option<[u8; 6]>,
    pub up: bool,
    pub addrs: Vec<IpAddr>,
    /// The default gateway on this interface: on a tethering link, the phone.
    pub gateways: Vec<IpAddr>,
}

/// A USB link to a phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbLink {
    pub iface: u32,
    /// For the screen ("Kabel · Pixel 8" needs the phone's name from its beacon; this is the interface).
    pub name: String,
    pub kind: UsbKind,
    /// Our addresses on the link (these go into the QR first).
    pub local: Vec<IpAddr>,
    /// Where the phone probably is: the gateway, then the classic Android address `192.168.42.129`.
    pub phone: Vec<IpAddr>,
}

/// Drivers of USB network gadgets. A USB Ethernet adapter may use the same drivers, so on Linux a match also
/// needs a locally administered MAC address, which the phone's gadget always has and a real adapter does not.
const GADGET_DRIVERS: [&str; 4] = ["rndis_host", "cdc_ncm", "cdc_ether", "cdc_eem"];

/// Whether an interface is a USB link to a phone. Pure, so it is tested without a phone.
#[must_use]
pub fn tether_kind(names: &[&str], driver: Option<&str>, mac: Option<[u8; 6]>) -> Option<UsbKind> {
    let lower: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let has = |h: &str| lower.iter().any(|n| n.contains(h));
    let driver = driver.map(str::to_ascii_lowercase);
    if driver.as_deref() == Some("ipheth") || has("apple mobile device ethernet") {
        return Some(UsbKind::Iphone);
    }
    if has("remote ndis") || has("usbncm") || has("usb ncm") {
        return Some(UsbKind::Android);
    }
    if lower
        .first()
        .is_some_and(|n| n == "usb0" || n == "usb1" || n == "rndis0")
    {
        return Some(UsbKind::Android);
    }
    let local_mac = mac.is_some_and(|m| m[0] & 0x02 != 0 && m[0] & 0x01 == 0);
    if driver
        .as_deref()
        .is_some_and(|d| GADGET_DRIVERS.contains(&d))
        && local_mac
    {
        return Some(UsbKind::Android);
    }
    None
}

/// The USB links among `ifaces`: up, a phone link by [`tether_kind`], with at least one address.
#[must_use]
pub fn links_from(ifaces: &[IfaceInfo]) -> Vec<UsbLink> {
    ifaces
        .iter()
        .filter(|i| i.up && !i.addrs.is_empty())
        .filter_map(|i| {
            let names: Vec<&str> = i.names.iter().map(String::as_str).collect();
            let kind = tether_kind(&names, i.driver.as_deref(), i.mac)?;
            let mut phone: Vec<IpAddr> = i.gateways.clone();
            let classic = IpAddr::V4(Ipv4Addr::new(192, 168, 42, 129));
            let on_classic = i
                .addrs
                .iter()
                .any(|a| matches!(a, IpAddr::V4(v4) if v4.octets()[..3] == [192, 168, 42]));
            if kind == UsbKind::Android && on_classic && !phone.contains(&classic) {
                phone.push(classic);
            }
            phone.retain(|p| !i.addrs.contains(p));
            Some(UsbLink {
                iface: i.index,
                name: i
                    .names
                    .get(1)
                    .or(i.names.first())
                    .cloned()
                    .unwrap_or_default(),
                kind,
                local: i.addrs.clone(),
                phone,
            })
        })
        .collect()
}

/// The kernel driver of interface `name` (Linux: `/sys/class/net/<name>/device/driver`); `None` elsewhere.
#[must_use]
pub fn driver_of(name: &str) -> Option<String> {
    if !cfg!(target_os = "linux") || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    let link = std::fs::read_link(format!("/sys/class/net/{name}/device/driver")).ok()?;
    Some(link.file_name()?.to_string_lossy().into_owned())
}

/// This machine's interfaces as [`IfaceInfo`].
#[must_use]
pub fn ifaces() -> Vec<IfaceInfo> {
    netdev::get_interfaces()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .map(|i| {
            let names = [
                Some(i.name.clone()),
                i.friendly_name.clone(),
                i.description.clone(),
            ]
            .into_iter()
            .flatten()
            .collect();
            let gateways = i
                .gateway
                .as_ref()
                .map(|g| {
                    g.ipv4
                        .iter()
                        .map(|a| IpAddr::V4(*a))
                        .chain(g.ipv6.iter().map(|a| IpAddr::V6(*a)))
                        .collect()
                })
                .unwrap_or_default();
            IfaceInfo {
                index: i.index,
                driver: driver_of(&i.name),
                mac: i.mac_addr.map(|m| m.octets()),
                up: i.is_up(),
                addrs: i
                    .ipv4
                    .iter()
                    .map(|n| IpAddr::V4(n.addr()))
                    .chain(i.ipv6.iter().map(|n| IpAddr::V6(n.addr())))
                    .collect(),
                gateways,
                names,
            }
        })
        .collect()
}

/// The USB links to phones on this machine right now.
#[must_use]
pub fn usb_links() -> Vec<UsbLink> {
    links_from(&ifaces())
}

/// What changed between two looks at the USB links: plugged in, and unplugged (by interface index).
#[must_use]
pub fn changes(before: &[UsbLink], now: &[UsbLink]) -> (Vec<UsbLink>, Vec<u32>) {
    let plugged = now
        .iter()
        .filter(|l| !before.iter().any(|b| b.iface == l.iface))
        .cloned()
        .collect();
    let unplugged = before
        .iter()
        .filter(|b| !now.iter().any(|l| l.iface == b.iface))
        .map(|b| b.iface)
        .collect();
    (plugged, unplugged)
}

/// How often [`watch`] looks at the interfaces. Cheap (one OS call); a cable shows up within this time.
pub const WATCH_EVERY: Duration = Duration::from_secs(2);

/// A cable came or went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbEvent {
    Plugged(UsbLink),
    Unplugged(u32),
}

/// Watches for USB phone links until the receiver is dropped; the links present at the start come as
/// [`UsbEvent::Plugged`] first.
#[must_use]
pub fn watch() -> tokio::sync::mpsc::Receiver<UsbEvent> {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        let mut seen = Vec::new();
        loop {
            let now = tokio::task::spawn_blocking(usb_links)
                .await
                .unwrap_or_default();
            let (plugged, unplugged) = changes(&seen, &now);
            for l in plugged {
                if tx.send(UsbEvent::Plugged(l)).await.is_err() {
                    return;
                }
            }
            for i in unplugged {
                if tx.send(UsbEvent::Unplugged(i)).await.is_err() {
                    return;
                }
            }
            seen = now;
            tokio::time::sleep(WATCH_EVERY).await;
        }
    });
    rx
}

/// Asks each phone address of `link` who it is (unicast beacon query on `beacon_port`), within `wait` in total.
/// The answer is unauthenticated like every beacon answer: connect, then pair with the six digits, unless the
/// phone is already trusted.
pub async fn find_phone(
    beacon: &BeaconService,
    link: &UsbLink,
    beacon_port: u16,
    wait: Duration,
) -> Option<Found> {
    let ips: Vec<IpAddr> = link.phone.iter().copied().filter(IpAddr::is_ipv4).collect();
    if ips.is_empty() {
        return None;
    }
    let each = wait / u32::try_from(ips.len()).unwrap_or(1);
    for ip in ips {
        if let Ok(found) =
            find::resolve(beacon, Target::Address(ip), beacon_port, each, false).await
        {
            return Some(found);
        }
    }
    None
}

pub mod adb {
    //! Android Debug Bridge, for a phone with USB debugging on (ADR-0070). We never ship `adb`; we use the one the
    //! user installed (`PATH`, `ANDROID_HOME`, `ANDROID_SDK_ROOT`, the Android Studio default folder).

    use std::path::{Path, PathBuf};
    use std::process::Command;

    use anyhow::{Context as _, Result, bail, ensure};

    pub use vutric_core::adb::{
        Device, MAX_DEVICES, MAX_OUTPUT_BYTES, State, parse_devices, token_ok,
    };

    /// The `adb` the user has, and how to run it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Adb {
        path: PathBuf,
    }

    impl Adb {
        /// `adb` at a known path (tests use a fake one).
        #[must_use]
        pub fn at(path: impl Into<PathBuf>) -> Self {
            Self { path: path.into() }
        }

        /// Finds `adb`: `PATH`, then `$ANDROID_HOME`/`$ANDROID_SDK_ROOT`/`platform-tools`, then the Android
        /// Studio default (`%LOCALAPPDATA%\Android\Sdk`, `~/Android/Sdk`, `~/Library/Android/sdk`).
        #[must_use]
        pub fn find() -> Option<Self> {
            let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
            let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default();
            for k in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
                if let Some(v) = std::env::var_os(k) {
                    dirs.push(Path::new(&v).join("platform-tools"));
                }
            }
            if let Some(v) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(Path::new(&v).join("Android/Sdk/platform-tools"));
            }
            if let Some(h) = std::env::var_os("HOME") {
                dirs.push(Path::new(&h).join("Android/Sdk/platform-tools"));
                dirs.push(Path::new(&h).join("Library/Android/sdk/platform-tools"));
            }
            dirs.into_iter()
                .map(|d| d.join(exe))
                .find(|p| p.is_file())
                .map(Self::at)
        }

        fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
            let out = Command::new(&self.path)
                .args(args)
                .output()
                .with_context(|| format!("run {}", self.path.display()))?;
            ensure!(
                out.status.success(),
                "adb {} failed: {}",
                args.first().copied().unwrap_or_default(),
                String::from_utf8_lossy(&out.stderr[..out.stderr.len().min(512)]).trim()
            );
            Ok(out.stdout)
        }

        /// The phones `adb` sees.
        pub fn devices(&self) -> Result<Vec<Device>> {
            Ok(parse_devices(&self.run(&["devices", "-l"])?))
        }

        /// Makes the phone's `127.0.0.1:port` reach this computer's `127.0.0.1:port` over the cable.
        pub fn reverse(&self, serial: &str, port: u16) -> Result<()> {
            ensure!(token_ok(serial), "bad adb serial");
            let spec = format!("tcp:{port}");
            self.run(&["-s", serial, "reverse", &spec, &spec]).map(drop)
        }

        /// Undoes [`Self::reverse`] (the phone forgets it by itself when the cable goes).
        pub fn remove_reverse(&self, serial: &str, port: u16) -> Result<()> {
            ensure!(token_ok(serial), "bad adb serial");
            self.run(&["-s", serial, "reverse", "--remove", &format!("tcp:{port}")])
                .map(drop)
        }

        /// Opens our pairing link in the phone's Vutric app (`vutric://` intent), so the user does not have to
        /// scan anything. The link is ours: plain ASCII, no quote, no space.
        pub fn open_link(&self, serial: &str, link: &str) -> Result<()> {
            ensure!(token_ok(serial), "bad adb serial");
            if !link.starts_with("vutric://")
                || link
                    .bytes()
                    .any(|b| !b.is_ascii_graphic() || b == b'\'' || b == b'"')
            {
                bail!("not a pairing link");
            }
            // `adb shell` hands the rest to the phone's shell; the single quotes keep `&` in the link literal.
            let cmd = format!("am start -a android.intent.action.VIEW -d '{link}'");
            self.run(&["-s", serial, "shell", &cmd]).map(drop)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn refuses_what_could_escape_the_phone_shell() {
            let adb = Adb::at("/nonexistent/adb");
            assert!(adb.open_link("R5CT", "vutric://pair?v=2&n=a'b").is_err());
            assert!(adb.open_link("R5CT", "https://example.com").is_err());
            assert!(adb.open_link("R5 CT", "vutric://pair?v=2").is_err());
            assert!(adb.reverse("", 1).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL_MAC: [u8; 6] = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f];
    const VENDOR_MAC: [u8; 6] = [0x00, 0xe0, 0x4c, 0x68, 0x00, 0x01];

    #[test]
    fn phone_links_by_name_driver_and_mac() {
        let k = tether_kind;
        assert_eq!(
            k(
                &[
                    "{GUID}",
                    "Ethernet 3",
                    "Remote NDIS based Internet Sharing Device"
                ],
                None,
                None
            ),
            Some(UsbKind::Android)
        );
        assert_eq!(
            k(&["{GUID}", "Ethernet 4", "UsbNcm Host Device"], None, None),
            Some(UsbKind::Android)
        );
        assert_eq!(
            k(&["Ethernet 5", "Apple Mobile Device Ethernet"], None, None),
            Some(UsbKind::Iphone)
        );
        assert_eq!(k(&["usb0"], None, None), Some(UsbKind::Android));
        assert_eq!(
            k(&["enx0a1b2c3d4e5f"], Some("cdc_ncm"), Some(LOCAL_MAC)),
            Some(UsbKind::Android)
        );
        assert_eq!(k(&["eth1"], Some("ipheth"), None), Some(UsbKind::Iphone));
        // A USB Ethernet adapter on the same driver, but with a vendor MAC: a LAN, not a phone.
        assert_eq!(
            k(&["enx00e04c680001"], Some("cdc_ncm"), Some(VENDOR_MAC)),
            None
        );
        assert_eq!(
            k(&["Wi-Fi", "Intel Wi-Fi 6 AX201"], None, Some(LOCAL_MAC)),
            None
        );
        assert_eq!(
            k(&["vEthernet (WSL)"], Some("hv_netvsc"), Some(LOCAL_MAC)),
            None
        );
    }

    fn fake_cable() -> IfaceInfo {
        IfaceInfo {
            index: 17,
            names: vec!["enx0a1b2c3d4e5f".into()],
            driver: Some("rndis_host".into()),
            mac: Some(LOCAL_MAC),
            up: true,
            addrs: vec![
                "192.168.42.17".parse().unwrap(),
                "fe80::81b:2cff:fe3d:4e5f".parse().unwrap(),
            ],
            gateways: vec![],
        }
    }

    #[test]
    fn a_fake_cable_becomes_a_link_with_the_phone_behind_it() {
        let wifi = IfaceInfo {
            index: 3,
            names: vec!["wlan0".into()],
            up: true,
            addrs: vec!["192.168.1.20".parse().unwrap()],
            gateways: vec!["192.168.1.1".parse().unwrap()],
            ..IfaceInfo::default()
        };
        let mut down = fake_cable();
        down.index = 18;
        down.up = false;
        let links = links_from(&[wifi.clone(), fake_cable(), down]);
        assert_eq!(
            links.len(),
            1,
            "Wi-Fi and a cable that is down are not phone links"
        );
        let l = &links[0];
        assert_eq!((l.iface, l.kind), (17, UsbKind::Android));
        assert_eq!(l.phone, vec!["192.168.42.129".parse::<IpAddr>().unwrap()]);

        // Android 11+ picks a random subnet and hands out the gateway by DHCP.
        let mut modern = fake_cable();
        modern.addrs = vec!["10.112.63.5".parse().unwrap()];
        modern.gateways = vec!["10.112.63.191".parse().unwrap()];
        assert_eq!(
            links_from(&[modern])[0].phone,
            vec!["10.112.63.191".parse::<IpAddr>().unwrap()]
        );

        // A self-assigned link only (no DHCP yet): a link without a phone address to ask.
        let mut bare = fake_cable();
        bare.addrs = vec!["169.254.3.4".parse().unwrap()];
        assert!(links_from(&[bare])[0].phone.is_empty());
    }

    #[test]
    fn plugging_and_unplugging_are_seen() {
        let a = links_from(&[fake_cable()]);
        let (plugged, unplugged) = changes(&[], &a);
        assert_eq!((plugged.len(), unplugged.len()), (1, 0));
        assert_eq!(changes(&a, &a), (vec![], vec![]));
        assert_eq!(changes(&a, &[]), (vec![], vec![17]));
    }

    #[tokio::test]
    async fn the_phone_behind_a_fake_cable_answers_its_beacon() {
        let phone =
            BeaconService::start_on(0, vutric_core::EndpointId([7; 32]), 4107, true).unwrap();
        let desktop =
            BeaconService::start_on(0, vutric_core::EndpointId([8; 32]), 4108, false).unwrap();
        // The cable's "gateway" is the phone; on this machine it is loopback.
        let mut cable = fake_cable();
        cable.gateways = vec!["127.0.0.1".parse().unwrap()];
        let link = links_from(&[cable]).remove(0);
        let found = find_phone(&desktop, &link, phone.port(), Duration::from_secs(3))
            .await
            .expect("phone answers");
        assert_eq!(found.id, vutric_core::EndpointId([7; 32]));
        assert!(found.phone);
        assert_eq!(found.addr.port(), 4107);
    }
}

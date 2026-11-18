//! Reading `adb devices -l` (backlog R5, ADR-0070). Pure: the host runs `adb` and passes its output. The text
//! comes from another program, so lines that do not fit are skipped, never guessed at; fuzzed (`adb_devices`).

/// Longest `adb devices -l` output we read; more is cut (a real one is a few hundred bytes).
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Most devices taken from one listing.
pub const MAX_DEVICES: usize = 32;

/// A device's state as `adb devices` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Authorized: `reverse` works.
    Device,
    /// The phone shows "Allow USB debugging?" and the user has not said yes.
    Unauthorized,
    Offline,
    /// Recovery, sideload, bootloader, or a state a newer `adb` adds.
    Other,
}

/// One line of `adb devices -l`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The serial `adb -s` takes. Printable ASCII without spaces, at most 64 bytes.
    pub serial: String,
    pub state: State,
    /// `model:` without the underscores (`Pixel 8`), at most 64 bytes.
    pub model: Option<String>,
    /// Connected over USB (`usb:` present), not over Wi-Fi debugging.
    pub usb: bool,
}

/// A serial or `adb` token we accept: printable ASCII without spaces, 1 to 64 bytes.
#[must_use]
pub fn token_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// Reads `adb devices -l`. Pure and fuzzed (`adb_devices`): the text comes from another program, so lines
/// that do not fit are skipped, never guessed at.
#[must_use]
pub fn parse_devices(out: &[u8]) -> Vec<Device> {
    let out = &out[..out.len().min(MAX_OUTPUT_BYTES)];
    let text = String::from_utf8_lossy(out);
    let mut devices = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(serial), Some(state)) = (parts.next(), parts.next()) else {
            continue;
        };
        if line.starts_with('*') || line.starts_with("List of") || !token_ok(serial) {
            continue;
        }
        let state = match state {
            "device" => State::Device,
            "unauthorized" => State::Unauthorized,
            "offline" => State::Offline,
            s if token_ok(s) => State::Other,
            _ => continue,
        };
        let mut model = None;
        let mut usb = false;
        for p in parts {
            if let Some(m) = p.strip_prefix("model:") {
                model = Some(m.replace('_', " ")).filter(|m| token_ok(&m.replace(' ', "_")));
            } else if p.starts_with("usb:") {
                usb = true;
            }
        }
        devices.push(Device {
            serial: serial.to_owned(),
            state,
            model,
            usb,
        });
        if devices.len() == MAX_DEVICES {
            break;
        }
    }
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_real_listing() {
        let out = b"* daemon not running; starting now at tcp:5037\n\
* daemon started successfully\n\
List of devices attached\n\
R5CT1234ABC            device usb:1-1 product:o1sxxx model:SM_G991B device:o1s transport_id:1\n\
emulator-5554          offline transport_id:2\n\
0A1B2C3D               unauthorized usb:2-3 transport_id:3\n\
192.168.1.9:5555       device product:panther model:Pixel_7 device:panther transport_id:4\n\
weird                  sideload\n\
\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 5);
        assert_eq!(
            d[0],
            Device {
                serial: "R5CT1234ABC".into(),
                state: State::Device,
                model: Some("SM G991B".into()),
                usb: true
            }
        );
        assert_eq!((d[1].state, d[1].usb), (State::Offline, false));
        assert_eq!((d[2].state, d[2].usb), (State::Unauthorized, true));
        assert_eq!((d[3].model.as_deref(), d[3].usb), (Some("Pixel 7"), false));
        assert_eq!(d[4].state, State::Other);
    }

    #[test]
    fn hostile_listings_stay_bounded() {
        assert!(parse_devices(b"").is_empty());
        assert!(parse_devices(b"\xff\xfe device\n").is_empty());
        let long = format!("{} device\n", "x".repeat(65));
        assert!(parse_devices(long.as_bytes()).is_empty());
        let many = "a device\n".repeat(1000);
        assert_eq!(parse_devices(many.as_bytes()).len(), MAX_DEVICES);
    }
}

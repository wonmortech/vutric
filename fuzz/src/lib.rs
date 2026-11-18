//! Fuzz targets for every parser that sees input from a peer or from a file another program may have written
//! (backlog S4, ADR-0007 part 2, ADR-0010 rule 4).
//!
//! Each target is a plain function over bytes, so the same checks run three ways:
//! - `cargo +nightly fuzz run <target>` with libFuzzer (CI job `fuzz`, see `.github/workflows/fuzz.yml`);
//! - `cargo test --no-default-features` on stable: a deterministic smoke run of random and mutated-valid inputs;
//! - replaying a crash file: `cargo +nightly fuzz run <target> artifacts/<target>/crash-…`.
//!
//! Every target asserts properties, not just "no panic": round trips, size caps, paths that stay inside the save
//! folder, names that are valid on every desktop OS.

use std::path::{Component, Path, PathBuf};

use arbitrary::{Arbitrary, Unstructured};
use vutric_core::frame::{self, Decoder, MAX_FRAME_BYTES};
use vutric_core::storage::{Arrival, Kind, SaveRules, Settings, Subfolder};
use vutric_core::trust::TrustStore;
use vutric_core::{Message, PairingTicket, filename};

const HEADER: usize = 4;

fn round_trips(msg: &Message) {
    let wire = frame::encode(msg).expect("a decoded message re-encodes");
    let again = frame::decode(&wire[HEADER..]).expect("and decodes again");
    assert_eq!(&again, msg, "round trip changed the message");
}

/// A control stream from a hostile peer: the first byte picks how the rest is split into reads. The decoder must
/// never panic, never buffer much more than one frame, and every message it yields must round-trip.
pub fn frame_stream(data: &[u8]) {
    let Some((&step, rest)) = data.split_first() else {
        return;
    };
    let step = usize::from(step).max(1);
    let mut d = Decoder::new();
    for chunk in rest.chunks(step) {
        d.push(chunk);
        loop {
            match d.next_message() {
                Ok(Some(msg)) => round_trips(&msg),
                Ok(None) => break,
                Err(_) => return,
            }
        }
        assert!(
            d.buffered() <= HEADER + MAX_FRAME_BYTES + step,
            "decoder holds {} bytes",
            d.buffered()
        );
    }
}

/// Structure-aware: an arbitrary `Message`. What the sender's validation accepts must arrive unchanged; what it
/// rejects must also be rejected by the receiver.
pub fn message(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let Ok(msg) = Message::arbitrary(&mut u) else {
        return;
    };
    let valid = msg.validate().is_ok();
    match frame::encode(&msg) {
        Ok(wire) => {
            let len = u32::from_le_bytes(wire[..HEADER].try_into().unwrap()) as usize;
            assert_eq!(len, wire.len() - HEADER);
            assert!(len <= MAX_FRAME_BYTES);
            let got = frame::decode(&wire[HEADER..]);
            assert_eq!(got.is_ok(), valid, "receiver and sender disagree");
            if let Ok(got) = got {
                assert_eq!(got, msg);
            }
        }
        Err(vutric_core::Error::FrameTooLarge { .. }) => {}
        Err(e) => panic!("encode failed: {e}"),
    }
}

/// A scanned QR code or a pasted link.
pub fn pairing_ticket(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(t) = PairingTicket::parse(text) else {
        return;
    };
    assert!(t.device_name.chars().count() <= 64);
    assert!(!t.device_name.chars().any(char::is_control));
    assert!(t.addrs.len() <= 8);
    if let Some(r) = &t.relay {
        assert!(r.starts_with("https://") && r.len() <= 200);
    }
    let uri = t.to_uri();
    assert!(uri.is_ascii(), "QR payload must stay ASCII");
    assert_eq!(PairingTicket::parse(&uri).expect("own link parses"), t);
}

#[derive(Debug, Arbitrary)]
struct TargetInput {
    path: String,
    mime: String,
    device: String,
    date: String,
    split: bool,
    keep_tree: bool,
    subfolder: u8,
    type_folder: Option<String>,
}

/// Where an offered file lands: the peer chooses the path, the MIME type and its own name. The result must stay
/// under the save folder, with no `..`, root or drive component, and no component the OS would mangle.
pub fn save_target(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let Ok(i) = TargetInput::arbitrary(&mut u) else {
        return;
    };
    let base = Path::new("save");
    let mut rules = SaveRules {
        split_by_type: i.split,
        keep_tree: i.keep_tree,
        subfolder: match i.subfolder % 4 {
            0 => Subfolder::None,
            1 => Subfolder::Device,
            2 => Subfolder::Date,
            _ => Subfolder::DeviceDate,
        },
        ..SaveRules::default()
    };
    if let Some(tf) = i.type_folder {
        // A relative type folder is typed by the user and cleaned like peer input; an absolute one is the user's
        // explicit choice and is not under the save folder by design.
        let tf = PathBuf::from(tf);
        if tf.is_absolute() || tf.has_root() {
            return;
        }
        rules.type_folders.insert(Kind::of(&i.path, &i.mime), tf);
    }
    let out = rules.target(
        base,
        Arrival {
            path: &i.path,
            mime: &i.mime,
            device: &i.device,
            date: &i.date,
        },
    );
    let rel = out
        .strip_prefix(base)
        .unwrap_or_else(|_| panic!("{} left the save folder", out.display()));
    assert!(rel.components().count() >= 1, "no file name");
    for c in rel.components() {
        let Component::Normal(name) = c else {
            panic!("{c:?} in {}", out.display())
        };
        check_name(&name.to_string_lossy());
    }
}

/// Whether `name` is one safe component on Windows, Linux and macOS.
fn check_name(name: &str) {
    assert!(!name.is_empty());
    assert!(
        !name.contains(['/', '\\', '<', '>', ':', '"', '|', '?', '*']),
        "{name:?}"
    );
    assert!(!name.chars().any(char::is_control), "{name:?}");
    assert!(
        !name.ends_with(['.', ' ']),
        "Windows drops a trailing dot or space: {name:?}"
    );
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
    assert!(!reserved.contains(&device.as_str()), "{name:?}");
    let numbered = (device.starts_with("COM") || device.starts_with("LPT"))
        && device.chars().count() == 4
        && device
            .chars()
            .nth(3)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '\u{b9}' | '\u{b2}' | '\u{b3}'));
    assert!(!numbered, "{name:?}");
}

/// A peer-supplied file name.
pub fn filename(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let out = filename::sanitize(&text);
    check_name(&out);
    assert!(out.chars().count() <= 120 + 1 + 16, "{out:?}");
    let unique = filename::unique(&out, |c| c == out);
    check_name(&unique);
}

/// The settings file: written by us, but it may be edited, damaged or replaced by another program.
pub fn settings_json(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(s) = serde_json::from_str::<Settings>(text) else {
        return;
    };
    let s = s.migrate();
    assert!((1..=30).contains(&s.parts_keep_days));
    assert!(!s.save.album.trim().is_empty() && s.save.album.chars().count() <= 64);
    let again: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(again.migrate(), s);
}

/// The trust file: a damaged or edited one must never yield an entry that is not a well-formed key.
pub fn trust_json(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(s) = serde_json::from_str::<TrustStore>(text) else {
        return;
    };
    let s = s.migrate();
    assert!(s.len() <= vutric_core::trust::MAX_DEVICES);
    assert_eq!(s.iter().count(), s.len(), "every kept key parses");
    for (_, d) in s.iter() {
        assert!(d.name.chars().count() <= 64);
        assert!(!d.name.chars().any(char::is_control));
    }
    let again: TrustStore = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(again.migrate(), s);
}

#[cfg(test)]
mod smoke {
    //! Deterministic stand-in for libFuzzer on stable: random inputs plus valid frames with bytes flipped.

    use vutric_core::message::OfferEntry;
    use vutric_core::{FlingId, Platform};

    use super::*;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn bytes(&mut self, max: usize) -> Vec<u8> {
            let n = (self.next() as usize) % (max + 1);
            (0..n).map(|_| self.next() as u8).collect()
        }
    }

    const RUNS: usize = 20_000;

    fn run(target: fn(&[u8]), max: usize, seed: u64) {
        let mut r = Rng(seed);
        for _ in 0..RUNS {
            target(&r.bytes(max));
        }
    }

    fn valid_wire() -> Vec<u8> {
        let msgs = [
            Message::hello("Pixel 8", Platform::Android),
            Message::Offer {
                id: FlingId([3; 16]),
                first: 0,
                entries: vec![OfferEntry {
                    path: "Trip/day 1/IMG_2041.jpg".into(),
                    size: 4_200_000,
                    mime: "image/jpeg".into(),
                }],
                total_files: 1,
                total_bytes: 4_200_000,
            },
            Message::OfferReply {
                id: FlingId([3; 16]),
                accept: true,
                have: vec![0],
            },
            Message::PairCommit { commit: [9; 32] },
            Message::PairResult { confirmed: true },
        ];
        msgs.iter()
            .flat_map(|m| frame::encode(m).unwrap())
            .collect()
    }

    #[test]
    fn frame_stream_random() {
        run(frame_stream, 4096, 0x9E37_79B9_7F4A_7C15);
    }

    #[test]
    fn frame_stream_mutated_valid() {
        let wire = valid_wire();
        let mut r = Rng(42);
        for _ in 0..RUNS {
            let mut v = wire.clone();
            for _ in 0..1 + r.next() % 4 {
                let i = (r.next() as usize) % v.len();
                v[i] ^= 1 << (r.next() % 8);
            }
            let mut input = vec![(r.next() % 64) as u8];
            input.extend_from_slice(&v);
            frame_stream(&input);
        }
    }

    #[test]
    fn frame_stream_whole_valid() {
        let mut input = vec![7u8];
        input.extend_from_slice(&valid_wire());
        frame_stream(&input);
    }

    #[test]
    fn message_structured() {
        run(message, 2048, 7);
    }

    #[test]
    fn pairing_ticket_random_and_near_valid() {
        run(pairing_ticket, 512, 11);
        let ok = "vutric://pair?v=2&id=5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a&n=PC&a=192.168.1.2:4711&r=https%3A%2F%2Fr.example%2F";
        pairing_ticket(ok.as_bytes());
        let mut r = Rng(5);
        for _ in 0..RUNS {
            let mut v = ok.as_bytes().to_vec();
            let i = (r.next() as usize) % v.len();
            let pool = b"%&=,:.[]0aZ\x00\xff";
            v[i] = pool[(r.next() as usize) % pool.len()];
            pairing_ticket(&v);
        }
    }

    #[test]
    fn save_target_structured() {
        run(save_target, 1024, 13);
    }

    #[test]
    fn filename_random() {
        run(filename, 600, 17);
    }

    #[test]
    fn filename_edge_cases() {
        filename(format!("a.b{}c", " ".repeat(20)).as_bytes());
        filename(format!("{}x.png", " .".repeat(80)).as_bytes());
        filename(b"CON .txt");
        filename(b"com1 .tar.gz");
        filename(format!("{} .jpg", "a".repeat(119)).as_bytes());
    }

    #[test]
    fn settings_and_trust_json() {
        run(settings_json, 512, 19);
        run(trust_json, 512, 23);
        let bidi = char::from_u32(0x202E).unwrap();
        settings_json(
            format!(r#"{{"version":0,"parts_keep_days":0,"save":{{"album":"{bidi}  "}}}}"#)
                .as_bytes(),
        );
        trust_json(br#"{"devices":{"zz":{"name":"x","platform":null,"method":"ticket","paired_at":0,"last_seen":0}}}"#);
    }
}

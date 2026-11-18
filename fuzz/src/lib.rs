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
use vutric_core::code::ShortCode;
use vutric_core::frame::{self, Decoder, MAX_FRAME_BYTES};
use vutric_core::hotspot::{HotspotTicket, MAX_SSID_BYTES, Security, Wifi};
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

/// A scanned QR code, a pasted link or a typed short code.
pub fn pairing_ticket(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(c) = ShortCode::parse(text) {
        assert_eq!(
            ShortCode::parse(&c.to_string()),
            Some(c),
            "typed code round-trips"
        );
    }
    let wifi = Wifi::in_link(text).ok().flatten();
    if let Some(w) = &wifi {
        assert!((1..=MAX_SSID_BYTES).contains(&w.ssid().len()));
        assert!(!w.ssid().chars().any(char::is_control));
        assert_eq!(w.password().is_none(), w.security() == Security::Open);
        let qr = w.to_wifi_qr();
        assert!(qr.starts_with("WIFI:T:") && qr.ends_with(";;"));
    }
    if let Ok(Some(secret)) = vutric_core::pairing::secret_in(text) {
        let again = vutric_core::pairing::with_secret("vutric://pair?v=2", &secret);
        assert_eq!(
            vutric_core::pairing::secret_in(&again),
            Ok(Some(secret)),
            "link secret round-trips"
        );
    }
    let Ok(t) = PairingTicket::parse(text) else {
        return;
    };
    if let Some(wifi) = wifi {
        let h = HotspotTicket {
            pair: t.clone(),
            wifi,
        };
        let uri = h.to_uri();
        assert!(uri.is_ascii(), "hotspot QR payload must stay ASCII");
        assert_eq!(
            HotspotTicket::parse(&uri).expect("own hotspot link parses"),
            h
        );
        assert_eq!(PairingTicket::parse(&uri).expect("old parser"), t);
    }
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

/// Companion messages from a paired phone (ADR-0064, issue #12): a notice or clipboard text. What validation
/// accepts must round-trip and stay inside the caps; what `companion::shown` returns must be safe to print.
pub fn companion(data: &[u8]) {
    use vutric_core::companion::shown;
    use vutric_core::message::{MAX_CLIPBOARD_BYTES, MAX_NOTICE_LINE_CHARS, MAX_NOTICE_TEXT_CHARS};
    let Some((&pick, rest)) = data.split_first() else {
        return;
    };
    let mut u = Unstructured::new(rest);
    let msg = if pick & 1 == 0 {
        let Ok(n) = vutric_core::Notice::arbitrary(&mut u) else {
            return;
        };
        Message::Notice(n)
    } else {
        let Ok(text) = String::arbitrary(&mut u) else {
            return;
        };
        Message::Clipboard { text }
    };
    let Ok(wire) = frame::encode(&msg) else {
        return;
    };
    let Ok(got) = frame::decode(&wire[HEADER..]) else {
        assert!(msg.validate().is_err(), "valid companion message refused");
        return;
    };
    assert_eq!(got, msg);
    let bad = |s: &str| {
        s.chars().any(|c| {
            (c.is_control() && c != '\n')
                || matches!(c, '\u{200E}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
    };
    match got {
        Message::Notice(n) => {
            assert!(n.app.chars().count() <= MAX_NOTICE_LINE_CHARS);
            assert!(n.title.chars().count() <= MAX_NOTICE_LINE_CHARS);
            assert!(n.text.chars().count() <= MAX_NOTICE_TEXT_CHARS);
            for (line, max) in [
                (&n.app, MAX_NOTICE_LINE_CHARS),
                (&n.title, MAX_NOTICE_LINE_CHARS),
            ] {
                let shown = shown(line, max, false);
                assert!(!bad(&shown) && !shown.contains('\n'));
            }
            assert!(!bad(&shown(&n.text, MAX_NOTICE_TEXT_CHARS, true)));
        }
        Message::Clipboard { text } => {
            assert!(!text.is_empty() && text.len() <= MAX_CLIPBOARD_BYTES);
            let preview = shown(&text, 200, true);
            assert!(preview.chars().count() <= 200 && !bad(&preview));
        }
        other => panic!("decoded into {}", other.kind()),
    }
}

/// Everything a magic-wormhole peer or server sends us before and after the key exchange (C7, ADR-0067): mailbox
/// server frames, the `pake` body, transit hints and file-transfer offers. Bounded output, no panic.
pub fn wormhole_messages(data: &[u8]) {
    use vutric_compat_wormhole::{mailbox, transfer, transit, wormhole};
    if let Some(m) = mailbox::parse(data) {
        if let mailbox::ServerMsg::Message { side, phase, body } = &m {
            assert!(side.len() <= mailbox::MAX_ID && phase.len() <= mailbox::MAX_ID);
            assert!(body.len() <= mailbox::MAX_BODY);
        }
        if let mailbox::ServerMsg::Allocated { nameplate } = &m {
            assert!(nameplate.bytes().all(|b| b.is_ascii_digit()));
        }
    }
    if let Some(p) = wormhole::pake_message(data) {
        assert!(p.len() <= 128);
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    if let Some(h) = transit::Hints::parse(&v) {
        assert!(h.direct.len() <= transit::MAX_HINTS && h.relays.len() <= transit::MAX_RELAYS);
        assert_eq!(
            transit::Hints::parse(&h.to_json()),
            Some(h),
            "hints round-trip"
        );
    }
    match transfer::parse_offer(&v) {
        Some(transfer::Offer::Text(t)) => assert!(t.len() <= transfer::MAX_TEXT),
        Some(transfer::Offer::File { name, .. }) => {
            assert_eq!(filename::sanitize(&name), name, "offered name is sanitized");
        }
        None => {}
    }
    if let Some(code) = data
        .get(..data.len().min(200))
        .and_then(|d| std::str::from_utf8(d).ok())
        && let Some(c) = vutric_compat_wormhole::code::Code::parse(code)
    {
        assert_eq!(
            vutric_compat_wormhole::code::Code::parse(c.as_str()),
            Some(c.clone())
        );
        assert!(c.nameplate().bytes().all(|b| b.is_ascii_digit()));
    }
}

/// A captive-portal check answer from an untrusted network (R1a, ADR-0068): never panics, and a login URL it
/// passes on is short, `http(s)` and printable.
pub fn captive_answer(data: &[u8]) {
    use vutric_core::captive::{MAX_LOGIN_URL, Portal, classify};
    if let Portal::Captive { login: Some(url) } = classify(data) {
        assert!(url.len() <= MAX_LOGIN_URL);
        assert!(url.starts_with("http://") || url.starts_with("https://"));
        assert!(url.bytes().all(|b| b.is_ascii_graphic()));
    }
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

/// The LAN beacon: anything but one exact beacon is `None`; a decoded beacon re-encodes to the same bytes, up to
/// the reserved flag bits it must carry through unchanged.
pub fn beacon(data: &[u8]) {
    use vutric_core::beacon::{Beacon, LEN};
    let Some(b) = Beacon::decode(data) else {
        return;
    };
    assert_eq!(data.len(), LEN);
    assert_ne!(b.port, 0);
    assert_eq!(&b.encode()[..], data);
}

/// `LocalSend` `prepare-upload` from a foreign sender (ADR-0054): bounded, every kept name is already safe to
/// save, ids match their keys, checksums are 64 lower-case hex.
pub fn localsend_prepare(data: &[u8]) {
    use vutric_compat_localsend::proto;
    let Ok(p) = proto::parse_prepare_upload(data) else {
        return;
    };
    assert!(!p.files.is_empty() && p.files.len() <= proto::MAX_FILES);
    assert!(p.info.alias.chars().count() <= 64 && !p.info.alias.is_empty());
    for (k, f) in &p.files {
        assert_eq!(k, &f.id);
        assert!(k.chars().count() <= proto::MAX_TEXT);
        assert_eq!(
            filename::sanitize(&f.file_name),
            f.file_name,
            "name stays sanitized"
        );
        check_name(&f.file_name);
        if let Some(h) = &f.sha256 {
            assert!(
                h.len() == 64
                    && h.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
        }
        assert!(
            f.preview
                .as_ref()
                .is_none_or(|p| p.len() <= proto::MAX_PREVIEW)
        );
    }
    let again = proto::parse_prepare_upload(&serde_json::to_vec(&p).unwrap()).expect("re-parses");
    assert_eq!(again, p, "parsing is idempotent");
}

/// `LocalSend` multicast announcements, `register` bodies and query strings.
pub fn localsend_announce(data: &[u8]) {
    use vutric_compat_localsend::proto;
    for info in [proto::parse_announcement(data), proto::parse_register(data)]
        .into_iter()
        .flatten()
    {
        assert!(info.version.starts_with('2'));
        assert!(!info.alias.chars().any(char::is_control));
        assert_ne!(info.port, Some(0));
    }
    if let Ok(q) = std::str::from_utf8(data) {
        if let Some(r) = vutric_compat_localsend::web::route(q) {
            use vutric_compat_localsend::web::Route;
            let (Route::Page(t) | Route::List(t) | Route::File(t, _)) = r;
            assert!(t.len() == 32 && t.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(q.starts_with("/s/") && !q.contains(".."));
        }
        let m = proto::query(q);
        assert!(m.len() <= 16);
        assert!(m.values().all(|v| v.chars().count() <= proto::MAX_TEXT));
    }
}

/// What the daemon reads from the adapter process, which is untrusted after it has parsed foreign bytes.
pub fn localsend_ipc(data: &[u8]) {
    use vutric_compat_localsend::ipc;
    if let Some(m) = ipc::parse::<ipc::FromAdapter>(data) {
        let line = ipc::line(&m);
        assert_eq!(
            ipc::parse::<ipc::FromAdapter>(&line[..line.len() - 1]),
            Some(m)
        );
    }
    let _ = ipc::parse::<ipc::ToAdapter>(data);
}

/// Every KDE Connect parser a peer reaches (ADR-0062): a packet line before and after TLS, the identity, pairing
/// and share bodies, and the certificate reader behind the verification key.
pub fn kdeconnect_packet(data: &[u8]) {
    use vutric_compat_kdeconnect::{proto, tls};
    let _ = tls::spki(data);
    let small = proto::Packet::parse(data, proto::MAX_PREAUTH_LINE);
    assert!(small.is_none() || data.len() <= proto::MAX_PREAUTH_LINE);
    let Some(p) = proto::Packet::parse(data, proto::MAX_LINE) else {
        return;
    };
    assert!(p.kind.starts_with("kdeconnect.") && p.kind.len() <= 64);
    if let Some(id) = proto::Identity::from_packet(&p) {
        assert!(proto::valid_device_id(&id.device_id));
        let n = id.device_name.chars().count();
        assert!((1..=proto::MAX_NAME).contains(&n));
        assert!(
            !id.device_name
                .chars()
                .any(|c| c.is_control() || "\"',;:.!?()[]<>".contains(c))
        );
        assert!(id.incoming.len() <= proto::MAX_CAPS && id.outgoing.len() <= proto::MAX_CAPS);
    }
    match proto::Share::from_packet(&p) {
        Some(proto::Share::File { name, port, .. }) => {
            assert_eq!(
                vutric_core::filename::sanitize(&name),
                name,
                "file name is already clean"
            );
            assert_ne!(port, 0);
        }
        Some(proto::Share::Text(t) | proto::Share::Url(t)) => {
            assert!(t.chars().count() <= proto::MAX_TEXT);
        }
        None => {}
    }
    let _ = proto::Pair::from_packet(&p);
}

/// Every Quick Share parser a peer reaches before or after the handshake (ADR-0056): offline and sharing frames,
/// UKEY2 messages, keys, endpoint info, and payload reassembly. Kept values stay within their limits.
pub fn quickshare_frames(data: &[u8]) {
    use vutric_compat_quickshare::{conn, endpoint, msg};
    if let Some(f) = msg::parse_offline(data) {
        let mut b = conn::Bytes::default();
        for _ in 0..3 {
            if let conn::Piece::Message(_, m) = b.feed(&f) {
                assert!(m.len() <= conn::MAX_BYTES_PAYLOAD);
            }
        }
    }
    if let Some(msg::Share::Introduction { files, texts }) = msg::parse_share(data) {
        assert!(files.len() + texts.len() <= msg::MAX_FILES);
        for f in &files {
            assert!(f.size >= 0 && f.name.chars().count() <= msg::MAX_NAME);
        }
    }
    if let Some((_, inner)) = msg::parse_ukey(data) {
        let _ = msg::parse_client_init(inner);
        let _ = msg::parse_server_init(inner);
    }
    let _ = msg::parse_p256_key(data);
    let _ = msg::parse_secure_message(data);
    let _ = msg::parse_header_and_body(data);
    if let Some(p) = endpoint::parse(data)
        && let Some(n) = p.name
    {
        assert!(!n.is_empty() && !n.chars().any(char::is_control));
    }
}

/// A hostile `ClientInit`, and hostile secure messages on an established channel: refused, never a panic.
pub fn quickshare_secure(data: &[u8]) {
    use vutric_compat_quickshare::ukey2;
    if let Ok((server, _)) = ukey2::Server::start(data) {
        // Only a ClientFinished matching the commitment could pass; random bytes never do.
        assert!(server.finish(data).is_err());
    }
    let (client, init) = ukey2::Client::start();
    let (server, server_init) = ukey2::Server::start(&init).expect("own init");
    let (_, finished) = client.finish(&server_init).expect("own server init");
    let keys = server.finish(&finished).expect("own finish");
    let mut ch = ukey2::Channel::new(&keys, true);
    assert!(ch.open(data).is_err(), "unsigned bytes never open");
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
            Message::PairProof { proof: [4; 32] },
            Message::Notice(vutric_core::Notice {
                app: "Telegram".into(),
                title: "Ali".into(),
                text: "salom".into(),
                at_ms: 1,
            }),
            Message::Clipboard {
                text: "nusxa".into(),
            },
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
        let hotspot = format!("{ok}&ws=Vutric%20AB&wk=wpa2&wp=k%26y%3D12345&wh=1");
        let mut r = Rng(5);
        for seed in [ok, hotspot.as_str()] {
            pairing_ticket(seed.as_bytes());
            for _ in 0..RUNS {
                let mut v = seed.as_bytes().to_vec();
                let i = (r.next() as usize) % v.len();
                let pool = b"%&=,:.[]0aZ\x00\xff";
                v[i] = pool[(r.next() as usize) % pool.len()];
                pairing_ticket(&v);
            }
        }
    }

    #[test]
    fn companion_structured() {
        run(companion, 1024, 23);
        companion(b"\x00\x08Telegram\x03Ali\x05salom\x2a");
        companion(b"\x01\x04\xe2\x80\xaeab");
    }

    #[test]
    fn wormhole_random_and_near_valid() {
        run(wormhole_messages, 1024, 29);
        let seeds = [
            r#"{"type":"message","side":"0a1b2c3d4e","phase":"pake","body":"7b7d","id":"x"}"#,
            r#"{"type":"welcome","welcome":{"motd":"hi","permission-required":{"none":{}}}}"#,
            r#"{"pake_v1":"00112233"}"#,
            r#"{"hints-v1":[{"type":"direct-tcp-v1","hostname":"10.0.0.2","port":4000},{"type":"relay-v1","hints":[{"type":"direct-tcp-v1","hostname":"r.example","port":4001}]}]}"#,
            r#"{"file":{"filename":"../a.txt","filesize":12}}"#,
            r#"{"directory":{"mode":"zipfile/deflated","dirname":"CON","zipsize":9}}"#,
            "7-guitarist-revenge",
        ];
        let mut r = Rng(31);
        for seed in seeds {
            wormhole_messages(seed.as_bytes());
            for _ in 0..RUNS {
                let mut v = seed.as_bytes().to_vec();
                let i = (r.next() as usize) % v.len();
                let pool = b"\"{}[]:,0-9aZ\\\x00\xff";
                v[i] = pool[(r.next() as usize) % pool.len()];
                wormhole_messages(&v);
            }
        }
    }

    #[test]
    fn captive_random_and_near_valid() {
        run(captive_answer, 2048, 37);
        let seeds = [
            "HTTP/1.1 204 No Content

",
            "HTTP/1.1 302 Found
Location: https://wifi.example/login?a=1

",
            "HTTP/1.0 200 OK
Content-Length: 0

",
        ];
        let mut r = Rng(41);
        for seed in seeds {
            for _ in 0..RUNS {
                let mut v = seed.as_bytes().to_vec();
                let i = (r.next() as usize) % v.len();
                v[i] = (r.next() & 0xff) as u8;
                captive_answer(&v);
            }
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
        // libFuzzer, CI run 44214498931: a device name cut to the stem limit, then prefixed with `_`.
        filename(include_bytes!(
            "../regressions/filename-device-at-limit.bin"
        ));
        filename(format!("{} .jpg", "a".repeat(119)).as_bytes());
    }

    #[test]
    fn beacon_random_and_near_valid() {
        use vutric_core::beacon::{Beacon, Kind};
        run(beacon, 64, 23);
        let ok = Beacon {
            kind: Kind::Query,
            id: vutric_core::EndpointId([9; 32]),
            port: 53_319,
            flags: 0,
        }
        .encode();
        let mut r = Rng(29);
        for _ in 0..RUNS {
            let mut v = ok.to_vec();
            let i = (r.next() as usize) % v.len();
            v[i] = r.next() as u8;
            beacon(&v);
        }
    }

    #[test]
    fn localsend_random_and_near_valid() {
        run(localsend_announce, 512, 31);
        run(localsend_ipc, 256, 37);
        run(localsend_prepare, 1024, 41);
        let ok = br#"{"info":{"alias":"A","version":"2.2","deviceType":"mobile","port":53317,"protocol":"https"},"files":{"a":{"id":"a","fileName":"CON .txt","size":5,"fileType":"text/plain","sha256":"00000000000000000000000000000000000000000000000000000000000000AA"}}}"#;
        localsend_prepare(ok);
        localsend_announce(br#"{"alias":"A","version":"2.0","announce":true}"#);
        let mut r = Rng(43);
        for _ in 0..RUNS {
            let mut v = ok.to_vec();
            let i = (r.next() as usize) % v.len();
            let pool = br#"{}[]":,.0a\ "#;
            v[i] = pool[(r.next() as usize) % pool.len()];
            localsend_prepare(&v);
            localsend_announce(&v);
        }
    }

    #[test]
    fn quickshare_random() {
        run(quickshare_frames, 512, 47);
        for i in 0..64u8 {
            quickshare_secure(&[i; 40]);
        }
    }

    #[test]
    fn kdeconnect_random_and_near_valid() {
        run(kdeconnect_packet, 512, 53);
        let seeds = [
            &br#"{"id":0,"type":"kdeconnect.identity","body":{"deviceId":"740bd4b9b4184ee497d6caf1da8151be","protocolVersion":8,"deviceName":"Pixel","deviceType":"phone","incomingCapabilities":["kdeconnect.share.request"],"tcpPort":1716}}"#[..],
            br#"{"id":0,"type":"kdeconnect.share.request","body":{"filename":"../a:b.txt","numberOfFiles":1},"payloadSize":10,"payloadTransferInfo":{"port":1739}}"#,
            br#"{"id":0,"type":"kdeconnect.pair","body":{"pair":true,"timestamp":642247200}}"#,
        ];
        let mut r = Rng(29);
        for seed in seeds {
            kdeconnect_packet(seed);
            for _ in 0..RUNS {
                let mut v = seed.to_vec();
                let i = (r.next() as usize) % v.len();
                let pool = b"\"{}[]:,.0-9az\\\x00\xff";
                v[i] = pool[(r.next() as usize) % pool.len()];
                kdeconnect_packet(&v);
            }
        }
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

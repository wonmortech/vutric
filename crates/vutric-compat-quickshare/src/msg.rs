//! The Quick Share messages Vutric reads and writes, with field numbers from the protocol's `.proto` files
//! (Chromium, BSD-3; listed in `NearDrop`'s `PROTOCOL.md`). Every decoder is strict about what it needs and bounded
//! in what it keeps; anything else is `None`.

use crate::pb::{Reader, Writer};

/// Most files in one introduction.
pub const MAX_FILES: usize = 1000;
/// Longest name or MIME type kept.
pub const MAX_NAME: usize = 1024;

fn fields(b: &[u8]) -> Option<Vec<(u32, crate::pb::Value<'_>)>> {
    let mut r = Reader::new(b);
    let v: Vec<_> = r.by_ref().take(4096).collect();
    (r.ok() && v.len() < 4096).then_some(v)
}

// ---------------------------------------------------------------- UKEY2

pub const UKEY_ALERT: i64 = 1;
pub const UKEY_CLIENT_INIT: i64 = 2;
pub const UKEY_SERVER_INIT: i64 = 3;
pub const UKEY_CLIENT_FINISH: i64 = 4;
pub const P256_SHA512: i64 = 100;
pub const NEXT_PROTOCOL: &str = "AES_256_CBC-HMAC_SHA256";

/// `Ukey2Message`: a type and the inner message.
#[must_use]
pub fn ukey(kind: i64, data: &[u8]) -> Vec<u8> {
    Writer::new().int(1, kind).bytes(2, data).finish()
}

#[must_use]
pub fn parse_ukey(b: &[u8]) -> Option<(i64, &[u8])> {
    let (mut kind, mut data) = (None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => kind = v.int(),
            2 => data = v.bytes(),
            _ => {}
        }
    }
    Some((kind?, data?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInit<'a> {
    pub version: i64,
    pub random: &'a [u8],
    /// The P-256 commitment, if offered.
    pub p256_commitment: Option<&'a [u8]>,
    pub next_protocol: &'a str,
}

#[must_use]
pub fn parse_client_init(b: &[u8]) -> Option<ClientInit<'_>> {
    let mut c = ClientInit {
        version: 0,
        random: &[],
        p256_commitment: None,
        next_protocol: "",
    };
    for (n, v) in fields(b)? {
        match n {
            1 => c.version = v.int()?,
            2 => c.random = v.bytes()?,
            3 => {
                let (mut cipher, mut commitment) = (None, None);
                for (m, w) in fields(v.bytes()?)? {
                    match m {
                        1 => cipher = w.int(),
                        2 => commitment = w.bytes(),
                        _ => {}
                    }
                }
                if cipher == Some(P256_SHA512) {
                    c.p256_commitment = commitment;
                }
            }
            4 => c.next_protocol = v.string()?,
            _ => {}
        }
    }
    Some(c)
}

#[must_use]
pub fn client_init(random: &[u8], commitment: &[u8]) -> Vec<u8> {
    let cc = Writer::new()
        .int(1, P256_SHA512)
        .bytes(2, commitment)
        .finish();
    Writer::new()
        .int(1, 1)
        .bytes(2, random)
        .bytes(3, &cc)
        .string(4, NEXT_PROTOCOL)
        .finish()
}

#[must_use]
pub fn server_init(random: &[u8], public_key: &[u8]) -> Vec<u8> {
    Writer::new()
        .int(1, 1)
        .bytes(2, random)
        .int(3, P256_SHA512)
        .bytes(4, public_key)
        .finish()
}

/// `(version, random, cipher, public key)` of a `Ukey2ServerInit`.
#[must_use]
pub fn parse_server_init(b: &[u8]) -> Option<(i64, &[u8], i64, &[u8])> {
    let (mut ver, mut rnd, mut cip, mut key) = (None, None, None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => ver = v.int(),
            2 => rnd = v.bytes(),
            3 => cip = v.int(),
            4 => key = v.bytes(),
            _ => {}
        }
    }
    Some((ver?, rnd?, cip?, key?))
}

#[must_use]
pub fn client_finished(public_key: &[u8]) -> Vec<u8> {
    Writer::new().bytes(1, public_key).finish()
}

#[must_use]
pub fn parse_client_finished(b: &[u8]) -> Option<&[u8]> {
    fields(b)?
        .into_iter()
        .find(|(n, _)| *n == 1)
        .and_then(|(_, v)| v.bytes())
}

/// `Ukey2Alert` with the given type (1 bad message … 200 internal error).
#[must_use]
pub fn alert(kind: i64) -> Vec<u8> {
    ukey(UKEY_ALERT, &Writer::new().int(1, kind).finish())
}

/// `GenericPublicKey { type: EC_P256, ec_p256_public_key { x, y } }`. Coordinates are written as protobuf's
/// two's-complement big-endian integers, so a leading `0x00` is added when the top bit is set.
#[must_use]
pub fn p256_key(x: &[u8; 32], y: &[u8; 32]) -> Vec<u8> {
    let signed = |c: &[u8; 32]| {
        let mut v = Vec::with_capacity(33);
        if c[0] & 0x80 != 0 {
            v.push(0);
        }
        v.extend_from_slice(c);
        v
    };
    let point = Writer::new()
        .bytes(1, &signed(x))
        .bytes(2, &signed(y))
        .finish();
    Writer::new().int(1, 1).bytes(2, &point).finish()
}

/// The 32-byte coordinates of a P-256 `GenericPublicKey`.
#[must_use]
pub fn parse_p256_key(b: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    let coord = |v: &[u8]| -> Option<[u8; 32]> {
        let v = match v {
            [0, rest @ ..] if rest.len() == 32 => rest,
            _ => v,
        };
        (v.len() <= 32).then(|| {
            let mut out = [0u8; 32];
            out[32 - v.len()..].copy_from_slice(v);
            out
        })
    };
    let (mut kind, mut point) = (None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => kind = v.int(),
            2 => point = v.bytes(),
            _ => {}
        }
    }
    if kind != Some(1) {
        return None;
    }
    let (mut x, mut y) = (None, None);
    for (n, v) in fields(point?)? {
        match n {
            1 => x = coord(v.bytes()?),
            2 => y = coord(v.bytes()?),
            _ => {}
        }
    }
    Some((x?, y?))
}

// ---------------------------------------------------------------- SecureMessage

const HMAC_SHA256: i64 = 1;
const AES_256_CBC: i64 = 2;
const DEVICE_TO_DEVICE_MESSAGE: i64 = 13;

/// `SecureMessage { header_and_body, signature }`.
#[must_use]
pub fn secure_message(header_and_body: &[u8], signature: &[u8]) -> Vec<u8> {
    Writer::new()
        .bytes(1, header_and_body)
        .bytes(2, signature)
        .finish()
}

#[must_use]
pub fn parse_secure_message(b: &[u8]) -> Option<(&[u8], &[u8])> {
    let (mut hb, mut sig) = (None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => hb = v.bytes(),
            2 => sig = v.bytes(),
            _ => {}
        }
    }
    Some((hb?, sig?))
}

/// `HeaderAndBody` for an AES-256-CBC + HMAC-SHA256 device-to-device message.
#[must_use]
pub fn header_and_body(iv: &[u8; 16], body: &[u8]) -> Vec<u8> {
    let meta = Writer::new()
        .int(1, DEVICE_TO_DEVICE_MESSAGE)
        .int(2, 1)
        .finish();
    let header = Writer::new()
        .int(1, HMAC_SHA256)
        .int(2, AES_256_CBC)
        .bytes(5, iv)
        .bytes(6, &meta)
        .finish();
    Writer::new().bytes(1, &header).bytes(2, body).finish()
}

/// `(iv, encrypted body)`, only for the one scheme pair the protocol uses.
#[must_use]
pub fn parse_header_and_body(b: &[u8]) -> Option<([u8; 16], &[u8])> {
    let (mut header, mut body) = (None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => header = v.bytes(),
            2 => body = v.bytes(),
            _ => {}
        }
    }
    let (mut sig, mut enc, mut iv) = (None, None, None);
    for (n, v) in fields(header?)? {
        match n {
            1 => sig = v.int(),
            2 => enc = v.int(),
            5 => iv = v.bytes(),
            _ => {}
        }
    }
    if sig != Some(HMAC_SHA256) || enc != Some(AES_256_CBC) {
        return None;
    }
    Some((iv?.try_into().ok()?, body?))
}

#[must_use]
pub fn device_message(seq: i64, message: &[u8]) -> Vec<u8> {
    Writer::new().bytes(1, message).int(2, seq).finish()
}

#[must_use]
pub fn parse_device_message(b: &[u8]) -> Option<(i64, &[u8])> {
    let (mut msg, mut seq) = (None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => msg = v.bytes(),
            2 => seq = v.int(),
            _ => {}
        }
    }
    Some((seq?, msg?))
}

// ---------------------------------------------------------------- Offline frames (Nearby Connections)

pub const WIFI_LAN: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadHeader {
    pub id: i64,
    /// 1 bytes, 2 file.
    pub kind: i64,
    pub total_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub flags: i64,
    pub offset: i64,
    pub body: &'a [u8],
}

/// Bit 0 of `Chunk::flags`.
pub const LAST_CHUNK: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offline<'a> {
    ConnectionRequest {
        endpoint_id: &'a str,
        endpoint_info: &'a [u8],
    },
    ConnectionResponse {
        accepted: bool,
    },
    Payload {
        header: PayloadHeader,
        chunk: Option<Chunk<'a>>,
        /// A control message: 1 error, 2 cancelled.
        control: Option<i64>,
    },
    KeepAlive {
        ack: bool,
    },
    Disconnection,
    /// Any other frame type (bandwidth upgrade and the like): ignored.
    Other(i64),
}

fn offline(v1: &[u8]) -> Vec<u8> {
    Writer::new().int(1, 1).bytes(2, v1).finish()
}

#[must_use]
pub fn connection_request(endpoint_id: &str, endpoint_info: &[u8]) -> Vec<u8> {
    let req = Writer::new()
        .string(1, endpoint_id)
        .bytes(6, endpoint_info)
        .int(5, WIFI_LAN)
        .finish();
    offline(&Writer::new().int(1, 1).bytes(2, &req).finish())
}

#[must_use]
pub fn connection_response(accept: bool) -> Vec<u8> {
    let os = Writer::new().int(1, 2).finish(); // OsInfo { type: LINUX }; Android only logs it
    let resp = Writer::new()
        .int(1, i64::from(!accept))
        .int(3, if accept { 1 } else { 2 })
        .bytes(4, &os)
        .finish();
    offline(&Writer::new().int(1, 2).bytes(3, &resp).finish())
}

#[must_use]
pub fn keep_alive(ack: bool) -> Vec<u8> {
    let ka = Writer::new().bool(1, ack).finish();
    offline(&Writer::new().int(1, 5).bytes(6, &ka).finish())
}

#[must_use]
pub fn disconnection() -> Vec<u8> {
    offline(&Writer::new().int(1, 6).bytes(7, &[]).finish())
}

/// One payload transfer frame carrying `body` at `offset` (`last` sets [`LAST_CHUNK`]).
#[must_use]
pub fn payload(header: &PayloadHeader, offset: i64, body: &[u8], last: bool) -> Vec<u8> {
    let h = Writer::new()
        .int(1, header.id)
        .int(2, header.kind)
        .int(3, header.total_size)
        .finish();
    let c = Writer::new()
        .int(1, if last { LAST_CHUNK } else { 0 })
        .int(2, offset)
        .bytes(3, body)
        .finish();
    let p = Writer::new().int(1, 1).bytes(2, &h).bytes(3, &c).finish();
    offline(&Writer::new().int(1, 3).bytes(4, &p).finish())
}

fn parse_payload(b: &[u8]) -> Option<Offline<'_>> {
    let (mut header, mut chunk, mut control) = (None, None, None);
    for (n, v) in fields(b)? {
        match n {
            2 => {
                let mut h = PayloadHeader {
                    id: 0,
                    kind: 0,
                    total_size: 0,
                };
                for (m, w) in fields(v.bytes()?)? {
                    match m {
                        1 => h.id = w.int()?,
                        2 => h.kind = w.int()?,
                        3 => h.total_size = w.int()?,
                        _ => {}
                    }
                }
                header = Some(h);
            }
            3 => {
                let mut c = Chunk {
                    flags: 0,
                    offset: 0,
                    body: &[],
                };
                for (m, w) in fields(v.bytes()?)? {
                    match m {
                        1 => c.flags = w.int()?,
                        2 => c.offset = w.int()?,
                        3 => c.body = w.bytes()?,
                        _ => {}
                    }
                }
                chunk = Some(c);
            }
            4 => {
                control = fields(v.bytes()?)?
                    .into_iter()
                    .find(|(m, _)| *m == 1)
                    .and_then(|(_, w)| w.int());
                control.get_or_insert(0);
            }
            _ => {}
        }
    }
    let header = header?;
    if header.total_size < 0 || chunk.as_ref().is_some_and(|c| c.offset < 0) {
        return None;
    }
    Some(Offline::Payload {
        header,
        chunk,
        control,
    })
}

/// Parses an `OfflineFrame` (version 1 only).
#[must_use]
pub fn parse_offline(b: &[u8]) -> Option<Offline<'_>> {
    let (mut version, mut v1) = (None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => version = v.int(),
            2 => v1 = v.bytes(),
            _ => {}
        }
    }
    if version != Some(1) {
        return None;
    }
    let f = fields(v1?)?;
    let kind = f.iter().find(|(n, _)| *n == 1).and_then(|(_, v)| v.int())?;
    let sub = |num: u32| {
        f.iter()
            .find(|(n, _)| *n == num)
            .and_then(|(_, v)| v.bytes())
    };
    match kind {
        1 => {
            let (mut id, mut info) = (None, None);
            for (n, v) in fields(sub(2)?)? {
                match n {
                    1 => id = v.string(),
                    6 => info = v.bytes(),
                    _ => {}
                }
            }
            Some(Offline::ConnectionRequest {
                endpoint_id: id.unwrap_or_default(),
                endpoint_info: info?,
            })
        }
        2 => {
            let (mut status, mut response) = (None, None);
            for (n, v) in fields(sub(3)?)? {
                match n {
                    1 => status = v.int(),
                    3 => response = v.int(),
                    _ => {}
                }
            }
            Some(Offline::ConnectionResponse {
                accepted: response.map_or(status == Some(0), |r| r == 1),
            })
        }
        3 => parse_payload(sub(4)?),
        5 => {
            let ack = sub(6)
                .and_then(fields)
                .and_then(|f| f.into_iter().find(|(n, _)| *n == 1))
                .and_then(|(_, v)| v.varint())
                == Some(1);
            Some(Offline::KeepAlive { ack })
        }
        6 => Some(Offline::Disconnection),
        k => Some(Offline::Other(k)),
    }
}

// ---------------------------------------------------------------- Sharing frames (Nearby Share)

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub name: String,
    pub payload_id: i64,
    pub size: i64,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMeta {
    pub title: String,
    /// 1 text, 2 URL, 3 address, 4 phone number.
    pub kind: i64,
    pub payload_id: i64,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Share {
    Introduction {
        files: Vec<FileMeta>,
        texts: Vec<TextMeta>,
    },
    /// 1 accept, 2 reject, 3 not enough space, 4 unsupported, 5 timed out.
    Response(i64),
    PairedKeyEncryption,
    PairedKeyResult,
    Cancel,
    Other(i64),
}

pub const ACCEPT: i64 = 1;
pub const REJECT: i64 = 2;
pub const NOT_ENOUGH_SPACE: i64 = 3;
pub const UNSUPPORTED: i64 = 4;

fn share(v1: &[u8]) -> Vec<u8> {
    Writer::new().int(1, 1).bytes(2, v1).finish()
}

#[must_use]
pub fn paired_key_encryption(secret_id_hash: &[u8; 6], signed_data: &[u8; 72]) -> Vec<u8> {
    let p = Writer::new()
        .bytes(1, signed_data)
        .bytes(2, secret_id_hash)
        .finish();
    share(&Writer::new().int(1, 3).bytes(4, &p).finish())
}

/// `PairedKeyResult { status: UNABLE }`: we have no Google account keys, so neither side can verify.
#[must_use]
pub fn paired_key_result() -> Vec<u8> {
    let p = Writer::new().int(1, 3).finish();
    share(&Writer::new().int(1, 4).bytes(5, &p).finish())
}

#[must_use]
pub fn response(status: i64) -> Vec<u8> {
    let p = Writer::new().int(1, status).finish();
    share(&Writer::new().int(1, 2).bytes(3, &p).finish())
}

#[must_use]
pub fn cancel() -> Vec<u8> {
    share(&Writer::new().int(1, 6).finish())
}

#[must_use]
pub fn introduction(files: &[FileMeta]) -> Vec<u8> {
    let mut w = Writer::new();
    for f in files {
        let m = Writer::new()
            .string(1, &f.name)
            .int(3, f.payload_id)
            .int(4, f.size)
            .string(5, &f.mime)
            .int(6, f.payload_id)
            .finish();
        w.bytes(1, &m);
    }
    let intro = w.finish();
    share(&Writer::new().int(1, 1).bytes(2, &intro).finish())
}

fn bounded(s: &str) -> String {
    s.chars().take(MAX_NAME).collect()
}

fn parse_intro(b: &[u8]) -> Option<Share> {
    let (mut files, mut texts) = (Vec::new(), Vec::new());
    for (n, v) in fields(b)? {
        if files.len() + texts.len() >= MAX_FILES {
            return None;
        }
        match n {
            1 => {
                let mut f = FileMeta {
                    name: String::new(),
                    payload_id: 0,
                    size: -1,
                    mime: String::new(),
                };
                for (m, w) in fields(v.bytes()?)? {
                    match m {
                        1 => f.name = bounded(w.string()?),
                        3 => f.payload_id = w.int()?,
                        4 => f.size = w.int()?,
                        5 => f.mime = bounded(w.string()?),
                        _ => {}
                    }
                }
                if f.size < 0 {
                    return None;
                }
                files.push(f);
            }
            2 => {
                let mut t = TextMeta {
                    title: String::new(),
                    kind: 0,
                    payload_id: 0,
                    size: -1,
                };
                for (m, w) in fields(v.bytes()?)? {
                    match m {
                        2 => t.title = bounded(w.string()?),
                        3 => t.kind = w.int()?,
                        4 => t.payload_id = w.int()?,
                        5 => t.size = w.int()?,
                        _ => {}
                    }
                }
                if t.size < 0 {
                    return None;
                }
                texts.push(t);
            }
            _ => {}
        }
    }
    Some(Share::Introduction { files, texts })
}

/// Parses a sharing `Frame` (version 1 only).
#[must_use]
pub fn parse_share(b: &[u8]) -> Option<Share> {
    let (mut version, mut v1) = (None, None);
    for (n, v) in fields(b)? {
        match n {
            1 => version = v.int(),
            2 => v1 = v.bytes(),
            _ => {}
        }
    }
    if version != Some(1) {
        return None;
    }
    let f = fields(v1?)?;
    let kind = f.iter().find(|(n, _)| *n == 1).and_then(|(_, v)| v.int())?;
    let sub = |num: u32| {
        f.iter()
            .find(|(n, _)| *n == num)
            .and_then(|(_, v)| v.bytes())
    };
    match kind {
        1 => parse_intro(sub(2).unwrap_or_default()),
        2 => {
            let status = fields(sub(3)?)?
                .into_iter()
                .find(|(n, _)| *n == 1)
                .and_then(|(_, v)| v.int())?;
            Some(Share::Response(status))
        }
        3 => Some(Share::PairedKeyEncryption),
        4 => Some(Share::PairedKeyResult),
        6 => Some(Share::Cancel),
        k => Some(Share::Other(k)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let files = vec![FileMeta {
            name: "IMG_1.jpg".into(),
            payload_id: 77,
            size: 5,
            mime: "image/jpeg".into(),
        }];
        assert_eq!(
            parse_share(&introduction(&files)),
            Some(Share::Introduction {
                files,
                texts: vec![]
            })
        );
        assert_eq!(parse_share(&response(ACCEPT)), Some(Share::Response(1)));
        assert_eq!(
            parse_share(&paired_key_result()),
            Some(Share::PairedKeyResult)
        );
        assert_eq!(
            parse_offline(&connection_response(true)),
            Some(Offline::ConnectionResponse { accepted: true })
        );
        assert_eq!(
            parse_offline(&keep_alive(true)),
            Some(Offline::KeepAlive { ack: true })
        );
        let h = PayloadHeader {
            id: 9,
            kind: 2,
            total_size: 3,
        };
        assert_eq!(
            parse_offline(&payload(&h, 0, b"abc", true)),
            Some(Offline::Payload {
                header: h,
                chunk: Some(Chunk {
                    flags: 1,
                    offset: 0,
                    body: b"abc"
                }),
                control: None
            })
        );
        let x = [0x80u8; 32];
        let y = [0x01u8; 32];
        assert_eq!(parse_p256_key(&p256_key(&x, &y)), Some((x, y)));
        let hb = header_and_body(&[7; 16], b"zz");
        let (iv, body) = parse_header_and_body(&hb).unwrap();
        assert_eq!((iv, body), ([7; 16], &b"zz"[..]));
    }

    #[test]
    fn hostile_frames() {
        // Negative size, wrong version, and an introduction with too many files.
        let neg = Writer::new().string(1, "x").int(4, -5).finish();
        let intro = Writer::new().bytes(1, &neg).finish();
        assert_eq!(
            parse_share(&share(&Writer::new().int(1, 1).bytes(2, &intro).finish())),
            None
        );
        assert_eq!(parse_offline(&Writer::new().int(1, 2).finish()), None);
        let one = Writer::new().string(1, "a").int(4, 1).finish();
        let mut many = Writer::new();
        for _ in 0..=MAX_FILES {
            many.bytes(1, &one);
        }
        let many = many.finish();
        assert_eq!(
            parse_share(&share(&Writer::new().int(1, 1).bytes(2, &many).finish())),
            None
        );
    }
}

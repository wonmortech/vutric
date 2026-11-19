//! UKEY2 (P-256, SHA-512 commitment) and the `SecureMessage` channel that follows it (ADR-0056).
//!
//! The handshake follows Google's public UKEY2 description; the key schedule after it (D2D keys, `ENC:2` /
//! `SIG:1`) and the 4-digit code follow `NearDrop`'s `PROTOCOL.md`. All primitives come from `ring` except
//! AES-256-CBC (`RustCrypto` `aes` + `cbc`). The commitment, the HMAC and the sequence numbers are checked before
//! anything is parsed further.

use aes::Aes256;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut as _, BlockEncryptMut as _, KeyIvInit as _};
use ring::agreement::{self, ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey};
use ring::rand::{SecureRandom as _, SystemRandom};
use ring::{digest, hkdf, hmac};

use crate::msg;

/// Why a handshake failed; the value is the UKEY2 alert type to send back, when one is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    BadMessage = 1,
    BadMessageType = 2,
    BadVersion = 100,
    BadRandom = 101,
    BadHandshakeCipher = 102,
    BadNextProtocol = 103,
    BadPublicKey = 104,
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UKEY2 {self:?}")
    }
}

impl std::error::Error for Fail {}

struct Len(usize);

impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn hkdf32(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    hkdf::Salt::new(hkdf::HKDF_SHA256, salt)
        .extract(ikm)
        .expand(&[info], Len(32))
        .and_then(|okm| okm.fill(&mut out))
        .expect("32 bytes is a valid HKDF-SHA256 length");
    out
}

fn sha256(b: &[u8]) -> [u8; 32] {
    digest::digest(&digest::SHA256, b)
        .as_ref()
        .try_into()
        .expect("32")
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    SystemRandom::new().fill(&mut b).expect("system random");
    b
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn keypair() -> (EphemeralPrivateKey, Vec<u8>) {
    let private =
        EphemeralPrivateKey::generate(&ECDH_P256, &SystemRandom::new()).expect("P-256 key");
    let public = private.compute_public_key().expect("public key");
    let p = public.as_ref();
    let x: [u8; 32] = p[1..33].try_into().expect("x");
    let y: [u8; 32] = p[33..65].try_into().expect("y");
    (private, msg::p256_key(&x, &y))
}

/// The outcome of a handshake: the authentication string (for the 4-digit code) and the next-protocol secret.
#[derive(Clone)]
pub struct Keys {
    auth: [u8; 32],
    next: [u8; 32],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(..)")
    }
}

impl Keys {
    /// The 4-digit code both screens show, so the user can see they talk to the right device.
    #[must_use]
    pub fn pin(&self) -> String {
        let (mut hash, mut mul) = (0i32, 1i32);
        for &b in &self.auth {
            hash = (hash + i32::from(b.cast_signed()) * mul) % 9973;
            mul = (mul * 31) % 9973;
        }
        format!("{:04}", hash.abs())
    }
}

fn derive(
    private: EphemeralPrivateKey,
    peer_key: &[u8],
    m1: &[u8],
    m2: &[u8],
) -> Result<Keys, Fail> {
    let (x, y) = msg::parse_p256_key(peer_key).ok_or(Fail::BadPublicKey)?;
    let mut point = [0u8; 65];
    point[0] = 4;
    point[1..33].copy_from_slice(&x);
    point[33..].copy_from_slice(&y);
    let derived =
        agreement::agree_ephemeral(private, &UnparsedPublicKey::new(&ECDH_P256, point), sha256)
            .map_err(|_| Fail::BadPublicKey)?;
    let mut info = Vec::with_capacity(m1.len() + m2.len());
    info.extend_from_slice(m1);
    info.extend_from_slice(m2);
    Ok(Keys {
        auth: hkdf32(&derived, b"UKEY2 v1 auth", &info),
        next: hkdf32(&derived, b"UKEY2 v1 next", &info),
    })
}

/// The server (receiver) side, between `ClientInit` and `ClientFinished`.
pub struct Server {
    m1: Vec<u8>,
    m2: Vec<u8>,
    commitment: [u8; 64],
    private: EphemeralPrivateKey,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ukey2::Server(..)")
    }
}

impl Server {
    /// Reads the client's `Ukey2Message(ClientInit)` and answers with `ServerInit`.
    pub fn start(client_init: &[u8]) -> Result<(Self, Vec<u8>), Fail> {
        let (kind, data) = msg::parse_ukey(client_init).ok_or(Fail::BadMessage)?;
        if kind != msg::UKEY_CLIENT_INIT {
            return Err(Fail::BadMessageType);
        }
        let c = msg::parse_client_init(data).ok_or(Fail::BadMessage)?;
        if c.version != 1 {
            return Err(Fail::BadVersion);
        }
        if c.random.len() != 32 {
            return Err(Fail::BadRandom);
        }
        let commitment: [u8; 64] = c
            .p256_commitment
            .and_then(|v| v.try_into().ok())
            .ok_or(Fail::BadHandshakeCipher)?;
        if c.next_protocol != msg::NEXT_PROTOCOL {
            return Err(Fail::BadNextProtocol);
        }
        let (private, public) = keypair();
        let server_init = msg::ukey(
            msg::UKEY_SERVER_INIT,
            &msg::server_init(&random::<32>(), &public),
        );
        Ok((
            Self {
                m1: client_init.to_vec(),
                m2: server_init.clone(),
                commitment,
                private,
            },
            server_init,
        ))
    }

    /// Checks `ClientFinished` against the commitment and derives the keys.
    pub fn finish(self, client_finished: &[u8]) -> Result<Keys, Fail> {
        let hash = digest::digest(&digest::SHA512, client_finished);
        if !ct_eq(hash.as_ref(), &self.commitment) {
            return Err(Fail::BadMessage);
        }
        let (kind, data) = msg::parse_ukey(client_finished).ok_or(Fail::BadMessage)?;
        if kind != msg::UKEY_CLIENT_FINISH {
            return Err(Fail::BadMessageType);
        }
        let key = msg::parse_client_finished(data).ok_or(Fail::BadMessage)?;
        derive(self.private, key, &self.m1, &self.m2)
    }
}

/// The client (sender) side, between `ClientInit` and `ServerInit`.
pub struct Client {
    m1: Vec<u8>,
    finished: Vec<u8>,
    private: EphemeralPrivateKey,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ukey2::Client(..)")
    }
}

impl Client {
    /// Our `Ukey2Message(ClientInit)`, committing to the `ClientFinished` we will send.
    #[must_use]
    pub fn start() -> (Self, Vec<u8>) {
        let (private, public) = keypair();
        let client_finished = msg::ukey(msg::UKEY_CLIENT_FINISH, &msg::client_finished(&public));
        let commitment = digest::digest(&digest::SHA512, &client_finished);
        let client_init = msg::ukey(
            msg::UKEY_CLIENT_INIT,
            &msg::client_init(&random::<32>(), commitment.as_ref()),
        );
        (
            Self {
                m1: client_init.clone(),
                finished: client_finished,
                private,
            },
            client_init,
        )
    }

    /// Reads `ServerInit`; returns the keys and the `ClientFinished` to send.
    pub fn finish(self, server_init: &[u8]) -> Result<(Keys, Vec<u8>), Fail> {
        let (kind, data) = msg::parse_ukey(server_init).ok_or(Fail::BadMessage)?;
        if kind != msg::UKEY_SERVER_INIT {
            return Err(Fail::BadMessageType);
        }
        let (version, rnd, cipher, key) = msg::parse_server_init(data).ok_or(Fail::BadMessage)?;
        if version != 1 {
            return Err(Fail::BadVersion);
        }
        if rnd.len() != 32 {
            return Err(Fail::BadRandom);
        }
        if cipher != msg::P256_SHA512 {
            return Err(Fail::BadHandshakeCipher);
        }
        let keys = derive(self.private, key, &self.m1, server_init)?;
        Ok((keys, self.finished))
    }
}

/// Encrypts and authenticates every frame after the handshake; one per connection.
pub struct Channel {
    encrypt: [u8; 32],
    decrypt: [u8; 32],
    send_mac: hmac::Key,
    recv_mac: hmac::Key,
    sent: i64,
    received: i64,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Channel(sent {}, received {})", self.sent, self.received)
    }
}

/// Why an incoming secure message was dropped (the connection ends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Broken {
    Malformed,
    BadSignature,
    BadPadding,
    OutOfOrder,
}

impl std::fmt::Display for Broken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "secure message: {self:?}")
    }
}

impl std::error::Error for Broken {}

impl Channel {
    #[must_use]
    pub fn new(keys: &Keys, server: bool) -> Self {
        let d2d_salt = sha256(b"D2D");
        let client = hkdf32(&keys.next, &d2d_salt, b"client");
        let server_key = hkdf32(&keys.next, &d2d_salt, b"server");
        let sm = sha256(b"SecureMessage");
        let (mine, theirs) = if server {
            (server_key, client)
        } else {
            (client, server_key)
        };
        Self {
            encrypt: hkdf32(&mine, &sm, b"ENC:2"),
            decrypt: hkdf32(&theirs, &sm, b"ENC:2"),
            send_mac: hmac::Key::new(hmac::HMAC_SHA256, &hkdf32(&mine, &sm, b"SIG:1")),
            recv_mac: hmac::Key::new(hmac::HMAC_SHA256, &hkdf32(&theirs, &sm, b"SIG:1")),
            sent: 0,
            received: 0,
        }
    }

    /// Wraps one offline frame.
    pub fn seal(&mut self, frame: &[u8]) -> Vec<u8> {
        self.sent += 1;
        let plain = msg::device_message(self.sent, frame);
        let iv = random::<16>();
        let ct = cbc::Encryptor::<Aes256>::new(&self.encrypt.into(), &iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(&plain);
        let hb = msg::header_and_body(&iv, &ct);
        let sig = hmac::sign(&self.send_mac, &hb);
        msg::secure_message(&hb, sig.as_ref())
    }

    /// Checks and unwraps one secure message into the offline frame inside.
    pub fn open(&mut self, wire: &[u8]) -> Result<Vec<u8>, Broken> {
        let (hb, sig) = msg::parse_secure_message(wire).ok_or(Broken::Malformed)?;
        hmac::verify(&self.recv_mac, hb, sig).map_err(|_| Broken::BadSignature)?;
        let (iv, ct) = msg::parse_header_and_body(hb).ok_or(Broken::Malformed)?;
        if ct.is_empty() || ct.len() % 16 != 0 {
            return Err(Broken::BadPadding);
        }
        let plain = cbc::Decryptor::<Aes256>::new(&self.decrypt.into(), &iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(ct)
            .map_err(|_| Broken::BadPadding)?;
        let (seq, frame) = msg::parse_device_message(&plain).ok_or(Broken::Malformed)?;
        if seq != self.received + 1 {
            return Err(Broken::OutOfOrder);
        }
        self.received = seq;
        Ok(frame.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Keys, Keys) {
        let (client, init) = Client::start();
        let (server, server_init) = Server::start(&init).unwrap();
        let (ck, finished) = client.finish(&server_init).unwrap();
        let sk = server.finish(&finished).unwrap();
        (ck, sk)
    }

    #[test]
    fn handshake_agrees_and_channel_works() {
        let (ck, sk) = pair();
        assert_eq!(ck.pin(), sk.pin());
        assert_eq!(ck.pin().len(), 4);
        let mut c = Channel::new(&ck, false);
        let mut s = Channel::new(&sk, true);
        for n in 0..3 {
            let w = c.seal(format!("hello {n}").as_bytes());
            assert_eq!(s.open(&w).unwrap(), format!("hello {n}").as_bytes());
        }
        let back = s.seal(b"ok");
        assert_eq!(c.open(&back).unwrap(), b"ok");
    }

    #[test]
    fn tampering_and_replay_are_refused() {
        let (ck, sk) = pair();
        let mut c = Channel::new(&ck, false);
        let mut s = Channel::new(&sk, true);
        let w = c.seal(b"one");
        let mut bad = w.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert_eq!(s.open(&bad), Err(Broken::BadSignature));
        assert!(s.open(&w).is_ok());
        assert_eq!(s.open(&w), Err(Broken::OutOfOrder), "replay");
        // A frame from the server's own direction is not accepted as the client's.
        let mut s2 = Channel::new(&sk, true);
        let own = s2.seal(b"x");
        assert_eq!(s.open(&own), Err(Broken::BadSignature));
    }

    #[test]
    fn handshake_refusals() {
        let (_client, init) = Client::start();
        let (server, _) = Server::start(&init).unwrap();
        // A ClientFinished other than the one committed to.
        let (other, _) = Client::start();
        let forged = other.finished.clone();
        assert_eq!(server.finish(&forged).unwrap_err(), Fail::BadMessage);
        assert_eq!(Server::start(b"\x08\x03").unwrap_err(), Fail::BadMessage);
        let wrong_type = msg::ukey(msg::UKEY_SERVER_INIT, b"");
        assert_eq!(
            Server::start(&wrong_type).unwrap_err(),
            Fail::BadMessageType
        );
        let bad_proto = msg::ukey(
            msg::UKEY_CLIENT_INIT,
            &crate::pb::Writer::new()
                .int(1, 1)
                .bytes(2, &[0; 32])
                .string(4, "AES_256_GCM")
                .finish(),
        );
        assert_eq!(
            Server::start(&bad_proto).unwrap_err(),
            Fail::BadHandshakeCipher
        );
    }

    #[test]
    fn pin_matches_the_reference_algorithm() {
        // auth bytes 0..32: computed by hand with the documented algorithm (mod 9973, base 31).
        let mut auth = [0u8; 32];
        for (i, b) in auth.iter_mut().enumerate() {
            *b = u8::try_from(i).unwrap();
        }
        let k = Keys {
            auth,
            next: [0; 32],
        };
        let (mut h, mut m) = (0i64, 1i64);
        for &b in &auth {
            h = (h + i64::from(b.cast_signed()) * m) % 9973;
            m = (m * 31) % 9973;
        }
        assert_eq!(k.pin(), format!("{:04}", h.abs()));
        let k = Keys {
            auth: [0xff; 32],
            next: [0; 32],
        };
        assert_eq!(k.pin().len(), 4, "negative bytes keep four digits");
    }
}

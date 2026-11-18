//! The wormhole's cryptography (client-protocol.md, transit.md): SPAKE2 on the code, HKDF-SHA256 subkeys, `NaCl`
//! secretbox for mailbox phases and transit records.

use crypto_secretbox::aead::{Aead as _, KeyInit as _};
use crypto_secretbox::{Key, Nonce, XSalsa20Poly1305};
use sha2_10::{Digest as _, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// Bytes of the shared key, phase keys and transit keys.
pub const KEY_BYTES: usize = 32;
/// Bytes of a secretbox nonce.
pub const NONCE_BYTES: usize = 24;
/// Bytes a secretbox adds: the nonce in front and the 16-byte tag.
pub const OVERHEAD: usize = NONCE_BYTES + 16;

pub type Key32 = [u8; KEY_BYTES];

/// HKDF-SHA256 with no salt: `key` as input keying material, `purpose` as info.
#[must_use]
pub fn derive(key: &Key32, purpose: &[u8]) -> Key32 {
    let mut out = [0u8; KEY_BYTES];
    hkdf::Hkdf::<Sha256>::new(None, key)
        .expand(purpose, &mut out)
        .expect("32 bytes is a valid HKDF length");
    out
}

/// The key for one mailbox message: purpose `wormhole:phase:` ‖ SHA-256(side) ‖ SHA-256(phase).
#[must_use]
pub fn phase_key(key: &Key32, side: &str, phase: &str) -> Key32 {
    let mut purpose = b"wormhole:phase:".to_vec();
    purpose.extend_from_slice(&Sha256::digest(side.as_bytes()));
    purpose.extend_from_slice(&Sha256::digest(phase.as_bytes()));
    derive(key, &purpose)
}

/// What both users may compare to rule out a man in the middle who guessed the code.
#[must_use]
pub fn verifier(key: &Key32) -> Key32 {
    derive(key, b"wormhole:verifier")
}

/// Secretbox with `nonce`, output `nonce ‖ ciphertext`.
#[must_use]
pub fn seal_with(key: &Key32, nonce: &[u8; NONCE_BYTES], plain: &[u8]) -> Vec<u8> {
    let ct = XSalsa20Poly1305::new(Key::from_slice(key))
        .encrypt(Nonce::from_slice(nonce), plain)
        .expect("secretbox encryption cannot fail");
    let mut out = Vec::with_capacity(NONCE_BYTES + ct.len());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    out
}

/// Secretbox with a random nonce.
#[must_use]
pub fn seal(key: &Key32, plain: &[u8]) -> Vec<u8> {
    use ring::rand::SecureRandom as _;
    let mut nonce = [0u8; NONCE_BYTES];
    ring::rand::SystemRandom::new()
        .fill(&mut nonce)
        .expect("OS random generator");
    seal_with(key, &nonce, plain)
}

/// Opens `nonce ‖ ciphertext`; returns the nonce too, for callers that check it (transit records).
#[must_use]
pub fn open(key: &Key32, data: &[u8]) -> Option<([u8; NONCE_BYTES], Vec<u8>)> {
    if data.len() < OVERHEAD {
        return None;
    }
    let (nonce, ct) = data.split_at(NONCE_BYTES);
    let plain = XSalsa20Poly1305::new(Key::from_slice(key))
        .decrypt(Nonce::from_slice(nonce), ct)
        .ok()?;
    Some((nonce.try_into().expect("24 bytes"), plain))
}

/// One side of the symmetric SPAKE2 exchange on the code, with the application id as the shared identity.
#[derive(Debug)]
pub struct Pake(Spake2<Ed25519Group>);

impl Pake {
    /// Starts the exchange; the returned bytes go into the `pake` phase as `pake_v1`.
    #[must_use]
    pub fn start(code: &str, appid: &str) -> (Self, Vec<u8>) {
        let (state, msg) = Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(code.as_bytes()),
            &Identity::new(appid.as_bytes()),
        );
        (Self(state), msg)
    }

    /// The shared key from the peer's message, or `None` for a malformed one. A wrong code still gives a key; it
    /// shows up as the first message that does not decrypt.
    #[must_use]
    pub fn finish(self, peer: &[u8]) -> Option<Key32> {
        self.0.finish(peer).ok()?.try_into().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pake_agrees_only_on_the_same_code() {
        let (alice, to_bob) = Pake::start("7-1234-5678", "app");
        let (bob, to_alice) = Pake::start("7-1234-5678", "app");
        assert_eq!(
            alice.finish(&to_alice).unwrap(),
            bob.finish(&to_bob).unwrap()
        );
        let (carol, from_carol) = Pake::start("7-1234-5679", "app");
        let (dave, from_dave) = Pake::start("7-1234-5678", "app");
        assert_ne!(
            carol.finish(&from_dave).unwrap(),
            dave.finish(&from_carol).unwrap()
        );
        let (eve, _) = Pake::start("x", "app");
        assert!(eve.finish(&[1, 2, 3]).is_none());
    }

    #[test]
    fn seal_open_and_tamper() {
        let k = [7u8; 32];
        let boxed = seal(&k, b"salom");
        assert_eq!(open(&k, &boxed).unwrap().1, b"salom");
        let mut bad = boxed.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(open(&k, &bad).is_none());
        assert!(open(&[8u8; 32], &boxed).is_none());
        assert!(open(&k, &boxed[..OVERHEAD - 1]).is_none());
    }

    #[test]
    fn phase_keys_differ_by_side_and_phase() {
        let k = [1u8; 32];
        let a = phase_key(&k, "aaaa", "version");
        assert_ne!(a, phase_key(&k, "bbbb", "version"));
        assert_ne!(a, phase_key(&k, "aaaa", "0"));
        assert_ne!(verifier(&k), derive(&k, b"wormhole:verifier!"));
    }
}

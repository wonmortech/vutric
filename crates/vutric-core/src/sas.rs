//! Commit-then-reveal numeric comparison for pairing without a QR code (backlog S2).
//!
//! The plain six-digit check (`TLS exporter mod 10^6`) can be ground: a man in the middle who answers mDNS runs
//! two TLS handshakes and tries key shares until both codes match, about 2^20 tries. Commit-then-reveal takes
//! that freedom away. On one connection, with `x` = 32 bytes of the connection's TLS exported keying material:
//!
//! 1. the dialer picks a random `a` and sends `PairCommit { c = H(x, a) }`;
//! 2. the listener picks a random `b` and sends `PairNonce { b }`;
//! 3. the dialer sends `PairReveal { a }`; the listener checks `c`;
//! 4. both screens show `code(x, a, b)`, six digits, and each user confirms (`PairResult`).
//!
//! The dialer is bound to `a` before it sees `b`, and the listener picks `b` without knowing `a`, so a man in the
//! middle (who is the listener on one connection and the dialer on the other) cannot make the two codes agree
//! except by a one-in-a-million guess per attempt. `x` binds everything to this very TLS session and its keys.
//! This is the scheme of Bluetooth Secure Simple Pairing numeric comparison and ZRTP's hash commitment.

/// Bytes of each random nonce.
pub const NONCE_BYTES: usize = 32;

/// Bytes of TLS keying material the exchange is bound to.
pub const EXPORTER_BYTES: usize = 32;

/// The TLS exporter label for this exchange (RFC 8446 section 7.5).
pub const EXPORTER_LABEL: &[u8] = b"EXPORTER-vutric/1 pairing sas";

const COMMIT_CONTEXT: &str = "vutric/1 2026-11-18 pairing commitment";
const CODE_CONTEXT: &str = "vutric/1 2026-11-18 pairing code";

/// `H(x, a)`: the dialer's commitment to its nonce.
#[must_use]
pub fn commitment(exporter: &[u8; EXPORTER_BYTES], nonce: &[u8; NONCE_BYTES]) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key(COMMIT_CONTEXT);
    h.update(exporter);
    h.update(nonce);
    *h.finalize().as_bytes()
}

/// Whether `nonce` opens `commit` on this connection. Constant time.
#[must_use]
pub fn opens(
    commit: &[u8; 32],
    exporter: &[u8; EXPORTER_BYTES],
    nonce: &[u8; NONCE_BYTES],
) -> bool {
    blake3::Hash::from_bytes(commitment(exporter, nonce)) == blake3::Hash::from_bytes(*commit)
}

/// The six digits both screens show, from the dialer's nonce `a` and the listener's nonce `b`.
#[must_use]
pub fn code(
    exporter: &[u8; EXPORTER_BYTES],
    dialer: &[u8; NONCE_BYTES],
    listener: &[u8; NONCE_BYTES],
) -> String {
    let mut h = blake3::Hasher::new_derive_key(CODE_CONTEXT);
    h.update(exporter);
    h.update(dialer);
    h.update(listener);
    let out = h.finalize();
    let n = u64::from_le_bytes(out.as_bytes()[..8].try_into().expect("8 bytes"));
    // 2^64 mod 10^6 skews the digits by under 10^-13: negligible.
    format!("{:06}", n % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn honest_run_agrees() {
        let x = [7u8; 32];
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let c = commitment(&x, &a);
        assert!(opens(&c, &x, &a));
        let code = code(&x, &a, &b);
        assert_eq!(code.len(), 6);
        assert!(code.bytes().all(|d| d.is_ascii_digit()));
    }

    #[test]
    fn commitment_is_bound_to_nonce_and_session() {
        let (x, a) = ([7u8; 32], [1u8; 32]);
        let c = commitment(&x, &a);
        assert!(!opens(&c, &x, &[3u8; 32]));
        assert!(!opens(&c, &[8u8; 32], &a));
    }

    #[test]
    fn every_input_changes_the_code() {
        let base = code(&[0; 32], &[0; 32], &[0; 32]);
        let mut changed = 0;
        for i in 0..32 {
            let mut v = [0u8; 32];
            v[i] = 1;
            changed += usize::from(code(&v, &[0; 32], &[0; 32]) != base);
            changed += usize::from(code(&[0; 32], &v, &[0; 32]) != base);
            changed += usize::from(code(&[0; 32], &[0; 32], &v) != base);
        }
        assert!(
            changed >= 95,
            "{changed} of 96 single-bit changes moved the code"
        );
    }

    #[test]
    fn codes_are_spread_evenly() {
        // 10 000 codes: every leading digit should appear about 1 000 times.
        let mut first = [0u32; 10];
        for i in 0u32..10_000 {
            let mut b = [0u8; 32];
            b[..4].copy_from_slice(&i.to_le_bytes());
            let c = code(&[9; 32], &b, &[5; 32]);
            first[usize::from(c.as_bytes()[0] - b'0')] += 1;
        }
        assert!(
            first.iter().all(|&n| (850..=1150).contains(&n)),
            "{first:?}"
        );
    }
}

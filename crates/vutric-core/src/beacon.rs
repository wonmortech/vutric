//! The LAN beacon (backlog R1): a fixed 41-byte UDP datagram that finds Vutric devices where mDNS is filtered.
//!
//! Sent to the link-local multicast group [`GROUP`] (inside `224.0.0.0/24`, which access points forward without
//! IGMP snooping, the group `LocalSend` uses) on [`PORT`], and unicast to single hosts for the probe and the opt-in
//! subnet scan. It carries only the endpoint key and the QUIC port: no name, nothing else to parse. A forged
//! beacon can only make a device try a QUIC connection, which then fails key authentication.
//!
//! Layout: `"VTRC" | version u8 | kind u8 | endpoint id [32] | quic port u16 LE | flags u8`.

use std::net::Ipv4Addr;

use crate::EndpointId;

/// Link-local multicast group of the beacon (shared with `LocalSend`; the port keeps the two apart).
pub const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);

/// UDP port of the beacon. `LocalSend` uses 53317; this is ours.
pub const PORT: u16 = 53319;

/// Exact size of every beacon; anything else is dropped unread.
pub const LEN: usize = 41;

const MAGIC: &[u8; 4] = b"VTRC";
const VERSION: u8 = 1;

/// What a beacon asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// "I am here": the receiver may connect to the sender's address at `port`.
    Announce,
    /// "Who is here?": every visible receiver answers with an [`Kind::Announce`] to the sender, unicast.
    Query,
}

/// One beacon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beacon {
    pub kind: Kind,
    pub id: EndpointId,
    /// The sender's QUIC port; the address is the datagram's source address.
    pub port: u16,
    /// Bit 0: the sender is a phone. Other bits are reserved and must be 0 when sent, ignored when read.
    pub flags: u8,
}

impl Beacon {
    #[must_use]
    pub fn encode(&self) -> [u8; LEN] {
        let mut out = [0u8; LEN];
        out[..4].copy_from_slice(MAGIC);
        out[4] = VERSION;
        out[5] = match self.kind {
            Kind::Announce => 0,
            Kind::Query => 1,
        };
        out[6..38].copy_from_slice(&self.id.0);
        out[38..40].copy_from_slice(&self.port.to_le_bytes());
        out[40] = self.flags;
        out
    }

    /// Reads a datagram. `None` for anything that is not exactly one beacon of our version.
    #[must_use]
    pub fn decode(datagram: &[u8]) -> Option<Self> {
        let d: &[u8; LEN] = datagram.try_into().ok()?;
        if &d[..4] != MAGIC || d[4] != VERSION {
            return None;
        }
        let kind = match d[5] {
            0 => Kind::Announce,
            1 => Kind::Query,
            _ => return None,
        };
        let port = u16::from_le_bytes([d[38], d[39]]);
        if port == 0 {
            return None;
        }
        Some(Self {
            kind,
            id: EndpointId(d[6..38].try_into().expect("32 bytes")),
            port,
            flags: d[40],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(kind: Kind) -> Beacon {
        Beacon {
            kind,
            id: EndpointId([7; 32]),
            port: 47_110,
            flags: 1,
        }
    }

    #[test]
    fn round_trips() {
        for k in [Kind::Announce, Kind::Query] {
            assert_eq!(Beacon::decode(&b(k).encode()), Some(b(k)));
        }
    }

    #[test]
    fn anything_else_is_dropped() {
        let ok = b(Kind::Announce).encode();
        assert_eq!(Beacon::decode(&ok[..LEN - 1]), None);
        let mut long = ok.to_vec();
        long.push(0);
        assert_eq!(Beacon::decode(&long), None);
        for (i, v) in [(0, b'X'), (4, 2), (5, 9)] {
            let mut bad = ok;
            bad[i] = v;
            assert_eq!(Beacon::decode(&bad), None, "byte {i}");
        }
        let mut zero_port = ok;
        zero_port[38] = 0;
        zero_port[39] = 0;
        assert_eq!(Beacon::decode(&zero_port), None);
    }
}

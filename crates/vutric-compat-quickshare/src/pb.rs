//! The protobuf wire format, only as much as Quick Share needs (ADR-0056).
//!
//! Hand-written instead of generated so that every limit is ours: a reader never allocates, never recurses on
//! its own, refuses groups and truncated fields, and returns `None` for anything malformed. Unknown fields are
//! skipped, as protobuf requires.

/// A field value as read from the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed64(u64),
    Fixed32(u32),
}

impl<'a> Value<'a> {
    #[must_use]
    pub fn varint(self) -> Option<u64> {
        match self {
            Value::Varint(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub fn bytes(self) -> Option<&'a [u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// An `int32`/`int64`/enum field: protobuf sign-extends negatives to 64 bits.
    #[must_use]
    #[allow(clippy::cast_possible_wrap)]
    pub fn int(self) -> Option<i64> {
        self.varint().map(|v| v as i64)
    }

    #[must_use]
    pub fn string(self) -> Option<&'a str> {
        std::str::from_utf8(self.bytes()?).ok()
    }
}

/// Iterates the fields of one message. Stops (and remembers the error) at the first malformed byte.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    bad: bool,
}

impl<'a> Reader<'a> {
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, bad: false }
    }

    /// Whether the message ended cleanly (call after iterating).
    #[must_use]
    pub fn ok(&self) -> bool {
        !self.bad
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v: u64 = 0;
        for i in 0..10 {
            let (&b, rest) = self.buf.split_first()?;
            self.buf = rest;
            if i == 9 && b > 1 {
                return None;
            }
            v |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.buf.len() < n {
            return None;
        }
        let (a, b) = self.buf.split_at(n);
        self.buf = b;
        Some(a)
    }

    fn field(&mut self) -> Option<(u32, Value<'a>)> {
        let key = self.varint()?;
        let number = u32::try_from(key >> 3).ok().filter(|&n| n != 0)?;
        let v = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => Value::Fixed64(u64::from_le_bytes(self.take(8)?.try_into().ok()?)),
            2 => {
                let n = usize::try_from(self.varint()?).ok()?;
                Value::Bytes(self.take(n)?)
            }
            5 => Value::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
            _ => return None, // groups (3, 4) and reserved types
        };
        Some((number, v))
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = (u32, Value<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.bad || self.buf.is_empty() {
            return None;
        }
        let r = self.field();
        if r.is_none() {
            self.bad = true;
        }
        r
    }
}

/// Builds one message.
#[derive(Debug, Default, Clone)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn raw_varint(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.buf
                .push(u8::try_from(v & 0x7f).expect("7 bits") | 0x80);
            v >>= 7;
        }
        self.buf.push(u8::try_from(v).expect("7 bits"));
    }

    fn key(&mut self, number: u32, wire: u8) {
        self.raw_varint((u64::from(number) << 3) | u64::from(wire));
    }

    pub fn varint(&mut self, number: u32, v: u64) -> &mut Self {
        self.key(number, 0);
        self.raw_varint(v);
        self
    }

    /// `int32`/`int64`/enum: negatives are sign-extended, as protobuf does.
    #[allow(clippy::cast_sign_loss)]
    pub fn int(&mut self, number: u32, v: i64) -> &mut Self {
        self.varint(number, v as u64)
    }

    pub fn bool(&mut self, number: u32, v: bool) -> &mut Self {
        self.varint(number, u64::from(v))
    }

    pub fn bytes(&mut self, number: u32, v: &[u8]) -> &mut Self {
        self.key(number, 2);
        self.raw_varint(v.len() as u64);
        self.buf.extend_from_slice(v);
        self
    }

    pub fn string(&mut self, number: u32, v: &str) -> &mut Self {
        self.bytes(number, v.as_bytes())
    }

    #[must_use]
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_refusals() {
        let inner = Writer::new().string(1, "ok").finish();
        let msg = Writer::new()
            .varint(1, 300)
            .int(2, -1)
            .bytes(3, &inner)
            .bool(4, true)
            .finish();
        let fields: Vec<_> = Reader::new(&msg).collect();
        assert_eq!(fields[0], (1, Value::Varint(300)));
        assert_eq!(fields[1].1.int(), Some(-1));
        assert_eq!(fields[2].1.bytes(), Some(inner.as_slice()));
        assert_eq!(fields[3], (4, Value::Varint(1)));
        // Truncated length, group wire type, field number 0, an 11-byte varint.
        for bad in [
            &[0x1a, 0x05, 0x01][..],
            &[0x0b],
            &[0x00, 0x01],
            &[
                0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02,
            ],
        ] {
            let mut r = Reader::new(bad);
            let _ = r.by_ref().count();
            assert!(!r.ok(), "{bad:?}");
        }
    }
}

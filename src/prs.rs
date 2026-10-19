//! Bounded SEGA PRS decoding and a literal-only encoder for small generated message files.

const LIMIT: usize = 1 << 20;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    control: u8,
    bits: u8,
}
impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.at).ok_or("truncated PRS stream")?;
        self.at += 1;
        Ok(b)
    }
    fn bit(&mut self) -> Result<bool, String> {
        if self.bits == 0 {
            self.control = self.byte()?;
            self.bits = 8;
        }
        let value = self.control & 1 != 0;
        self.control >>= 1;
        self.bits -= 1;
        Ok(value)
    }
}

pub fn decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = Reader {
        bytes,
        at: 0,
        control: 0,
        bits: 0,
    };
    let mut out = Vec::new();
    loop {
        if reader.bit()? {
            if out.len() == LIMIT {
                return Err("PRS output limit exceeded".into());
            }
            out.push(reader.byte()?);
            continue;
        }
        let (distance, count) = if reader.bit()? {
            let word = u16::from_le_bytes([reader.byte()?, reader.byte()?]);
            if word == 0 {
                return Ok(out);
            }
            let count = if word & 7 == 0 {
                usize::from(reader.byte()?) + 1
            } else {
                usize::from(word & 7) + 2
            };
            (8192 - usize::from(word >> 3), count)
        } else {
            let count = (usize::from(reader.bit()?) << 1) + usize::from(reader.bit()?) + 2;
            (256 - usize::from(reader.byte()?), count)
        };
        let start = out
            .len()
            .checked_sub(distance)
            .ok_or("invalid PRS back reference")?;
        if count > LIMIT - out.len() {
            return Err("PRS output limit exceeded".into());
        }
        for i in 0..count {
            let value = out[start + i];
            out.push(value);
        }
    }
}

pub fn encode(bytes: &[u8]) -> Vec<u8> {
    struct Writer {
        bytes: Vec<u8>,
        control: usize,
        bits: u8,
    }
    impl Writer {
        fn bit(&mut self, set: bool) {
            if self.bits == 8 {
                self.control = self.bytes.len();
                self.bytes.push(0);
                self.bits = 0;
            }
            if set {
                self.bytes[self.control] |= 1 << self.bits;
            }
            self.bits += 1;
        }
    }
    let mut out = Writer {
        bytes: Vec::new(),
        control: 0,
        bits: 8,
    };
    for &byte in bytes {
        out.bit(true);
        out.bytes.push(byte);
    }
    out.bit(false);
    out.bit(true);
    out.bytes.extend([0, 0]);
    out.bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literals_round_trip_at_control_byte_boundaries() {
        for size in [0, 1, 6, 7, 8, 9, 15, 16, 4096] {
            let bytes: Vec<u8> = (0..size).map(|i| i as u8).collect();
            assert_eq!(decode(&encode(&bytes)).unwrap(), bytes);
        }
    }
    #[test]
    fn compressed_overlapping_copies_are_supported() {
        // Literal A, then a short distance-1 copy of length 5, then the terminator.
        assert_eq!(decode(&[0x59, b'A', 0xff, 0, 0]).unwrap(), b"AAAAAA");
    }
    #[test]
    fn truncated_and_invalid_back_references_are_rejected() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[1]).is_err());
        assert!(decode(&[0, 0]).is_err());
    }
}

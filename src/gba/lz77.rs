//! GBA BIOS LZ77 (type 0x10) decompression.

/// Upper bound on the decompressed size; GBA graphics are far smaller.
const MAX_OUT: usize = 1 << 20;

/// Decompresses a type 0x10 stream: header byte 0x10, 24-bit LE output size, then blocks of a flag
/// byte (MSB first; 1 = back-reference) and 8 items. A back-reference is 2 bytes: length - 3 in the
/// high nibble, distance - 1 in the remaining 12 bits. `None` on a malformed stream.
pub(crate) fn decompress(src: &[u8]) -> Option<Vec<u8>> {
    if src.len() < 4 || src[0] != 0x10 {
        return None;
    }
    let size = usize::from(src[1]) | usize::from(src[2]) << 8 | usize::from(src[3]) << 16;
    if size > MAX_OUT {
        return None;
    }
    let mut out = Vec::with_capacity(size);
    let mut pos = 4;
    while out.len() < size {
        let flags = *src.get(pos)?;
        pos += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags >> bit & 1 == 0 {
                out.push(*src.get(pos)?);
                pos += 1;
            } else {
                let hi = *src.get(pos)?;
                let lo = *src.get(pos + 1)?;
                pos += 2;
                let len = usize::from(hi >> 4) + 3;
                let dist = (usize::from(hi & 0x0F) << 8 | usize::from(lo)) + 1;
                let start = out.len().checked_sub(dist)?;
                for i in 0..len {
                    if out.len() >= size {
                        break;
                    }
                    let b = out[start + i];
                    out.push(b);
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_only() {
        let s = [0x10, 3, 0, 0, 0x00, b'a', b'b', b'c'];
        assert_eq!(decompress(&s).unwrap(), b"abc");
    }

    #[test]
    fn overlapping_back_reference() {
        // 'a', 'b', then copy 6 bytes from distance 2.
        let s = [0x10, 8, 0, 0, 0b0010_0000, b'a', b'b', 0x30, 0x01];
        assert_eq!(decompress(&s).unwrap(), b"abababab");
    }

    #[test]
    fn reference_truncated_to_size() {
        let s = [0x10, 4, 0, 0, 0b0100_0000, b'x', 0xF0, 0x00];
        assert_eq!(decompress(&s).unwrap(), b"xxxx");
    }

    #[test]
    fn malformed_streams() {
        assert!(decompress(&[]).is_none());
        assert!(decompress(&[0x11, 1, 0, 0, 0, 0]).is_none());
        // Truncated literal.
        assert!(decompress(&[0x10, 2, 0, 0, 0, b'a']).is_none());
        // Reference before any output.
        assert!(decompress(&[0x10, 3, 0, 0, 0x80, 0x00, 0x00]).is_none());
        // Size beyond the cap.
        assert!(decompress(&[0x10, 0xFF, 0xFF, 0xFF]).is_none());
        assert_eq!(decompress(&[0x10, 0, 0, 0]).unwrap(), b"");
    }
}

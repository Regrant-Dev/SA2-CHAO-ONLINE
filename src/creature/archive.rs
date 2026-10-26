//! GARC metadata and individually bounded member reads. Large model/map archives stay on
//! disk: opening their table never copies the complete archive into the importer.

use super::source::{GameSource, ImportError, Region, Result, bytes, invalid, u16_at, u32_at};

const MAX_TABLE: usize = 8 << 20;
const MAX_MEMBERS: usize = 131_072;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub index: u32,
    pub subfile: u8,
    pub region: Region,
}

pub struct Garc<'a> {
    source: &'a GameSource,
    entries: Vec<ArchiveEntry>,
    count: usize,
}

impl<'a> Garc<'a> {
    pub fn open(source: &'a GameSource, archive: Region) -> Result<Self> {
        let header = source.read(archive.sub(0, 28)?, 28)?;
        let size = u32_at(&header, 4)?;
        let version = u16_at(&header, 10)?;
        if &header[..4] != b"CRAG"
            || u16_at(&header, 8)? != 0xFEFF
            || !matches!((version, size), (0x400, 28) | (0x600, 36))
            || u32_at(&header, 12)? != 4
            || u64::from(u32_at(&header, 20)?) != archive.size
        {
            return Err(invalid("unsupported or inconsistent GARC header"));
        }
        let header = source.read(archive.sub(0, u64::from(size))?, 36)?;
        let data_offset = u64::from(u32_at(&header, 16)?);
        let fato = source.read(archive.sub(u64::from(size), 12)?, 12)?;
        if &fato[..4] != b"OTAF" {
            return Err(invalid("GARC FATO table is missing"));
        }
        let fato_size = u32_at(&fato, 4)?;
        let count = usize::from(u16_at(&fato, 8)?);
        if count == 0 || fato_size as usize != 12 + count * 4 {
            return Err(invalid("invalid GARC FATO count/size"));
        }
        let offsets = source.read(
            archive.sub(u64::from(size) + 12, (count * 4) as u64)?,
            MAX_TABLE,
        )?;
        let fatb_offset = u64::from(size) + u64::from(fato_size);
        let fatb_header = source.read(archive.sub(fatb_offset, 12)?, 12)?;
        if &fatb_header[..4] != b"BTAF" || u32_at(&fatb_header, 8)? as usize != count {
            return Err(invalid("invalid GARC FATB table"));
        }
        let fatb_size = u32_at(&fatb_header, 4)?;
        if fatb_size < 12 {
            return Err(invalid("invalid GARC FATB length"));
        }
        let table = source.read(
            archive.sub(fatb_offset + 12, u64::from(fatb_size - 12))?,
            MAX_TABLE,
        )?;
        let fimb_offset = fatb_offset + u64::from(fatb_size);
        let fimb = source.read(archive.sub(fimb_offset, 12)?, 12)?;
        if &fimb[..4] != b"BMIF" || u32_at(&fimb, 4)? != 12 || data_offset != fimb_offset + 12 {
            return Err(invalid("invalid GARC data section"));
        }
        let data = archive.sub(data_offset, u64::from(u32_at(&fimb, 8)?))?;
        let entries = entries(&offsets, &table, data)?;
        Ok(Self {
            source,
            entries,
            count,
        })
    }

    pub fn entry_count(&self) -> usize {
        self.count
    }

    pub fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }

    pub fn read_entry(&self, index: usize, limit: usize) -> Result<Vec<u8>> {
        self.read_member(index, 0, limit)
    }

    pub fn read_member(&self, index: usize, subfile: u8, limit: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.index as usize == index && e.subfile == subfile)
            .ok_or_else(|| {
                ImportError::new(format!("GARC member {index}:{subfile} was not found"))
            })?;
        self.source.read(entry.region, limit)
    }
}

fn entries(offsets: &[u8], table: &[u8], data: Region) -> Result<Vec<ArchiveEntry>> {
    if !offsets.len().is_multiple_of(4) {
        return Err(invalid("unaligned GARC offsets table"));
    }
    let mut out = Vec::new();
    let mut previous_record_end = 0usize;
    for (index, word) in offsets.as_chunks::<4>().0.iter().enumerate() {
        let mut at = u32::from_le_bytes(*word) as usize;
        if !at.is_multiple_of(4) || at < previous_record_end {
            return Err(invalid("GARC member metadata overlaps or is unaligned"));
        }
        let bitmap = u32_at(table, at)?;
        at = at
            .checked_add(4)
            .ok_or_else(|| invalid("GARC table offset overflow"))?;
        for subfile in 0..32 {
            if bitmap & (1 << subfile) == 0 {
                continue;
            }
            let record = bytes(table, at, 12)?;
            let start = u32_at(record, 0)?;
            let end = u32_at(record, 4)?;
            let size = u32_at(record, 8)?;
            if end < start || size > end - start {
                return Err(invalid("invalid GARC member range"));
            }
            data.sub(u64::from(start), u64::from(end - start))?;
            out.push(ArchiveEntry {
                index: index as u32,
                subfile,
                region: data.sub(u64::from(start), u64::from(size))?,
            });
            if out.len() > MAX_MEMBERS {
                return Err(invalid("GARC has too many members"));
            }
            at += 12;
        }
        previous_record_end = at;
    }
    Ok(out)
}

/// Nintendo LZ11 stream, decoded with a caller-chosen output bound. Back-references are
/// permitted to overlap, but cannot refer before the beginning or exceed the declared size.
pub fn decompress_lz11(input: &[u8], limit: usize) -> Result<Vec<u8>> {
    if input.first() != Some(&0x11) {
        return Err(invalid("expected an LZ11 stream"));
    }
    let header = bytes(input, 0, 4)?;
    let mut length =
        usize::from(header[1]) | usize::from(header[2]) << 8 | usize::from(header[3]) << 16;
    let mut at = 4;
    if length == 0 {
        length = u32_at(input, at)? as usize;
        at += 4;
    }
    if length == 0 || length > limit {
        return Err(invalid("LZ11 declared output exceeds its allocation bound"));
    }
    let mut out = Vec::with_capacity(length);
    while out.len() < length {
        let flags = bytes(input, at, 1)?[0];
        at += 1;
        for bit in (0..8).rev() {
            if out.len() == length {
                break;
            }
            if flags & (1 << bit) == 0 {
                out.push(bytes(input, at, 1)?[0]);
                at += 1;
                continue;
            }
            let a = bytes(input, at, 1)?[0];
            at += 1;
            let (count, displacement) = match a >> 4 {
                0 => {
                    let b = bytes(input, at, 2)?;
                    at += 2;
                    (
                        (usize::from(a & 15) << 4 | usize::from(b[0] >> 4)) + 0x11,
                        (usize::from(b[0] & 15) << 8 | usize::from(b[1])) + 1,
                    )
                }
                1 => {
                    let b = bytes(input, at, 3)?;
                    at += 3;
                    (
                        (usize::from(a & 15) << 12
                            | usize::from(b[0]) << 4
                            | usize::from(b[1] >> 4))
                            + 0x111,
                        (usize::from(b[1] & 15) << 8 | usize::from(b[2])) + 1,
                    )
                }
                short => {
                    let b = bytes(input, at, 1)?[0];
                    at += 1;
                    (
                        usize::from(short) + 1,
                        (usize::from(a & 15) << 8 | usize::from(b)) + 1,
                    )
                }
            };
            if displacement > out.len() || count > length - out.len() {
                return Err(invalid("invalid LZ11 back-reference or output length"));
            }
            for _ in 0..count {
                out.push(out[out.len() - displacement]);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(bitmap: u32, ranges: &[(u32, u32, u32)]) -> Vec<u8> {
        bitmap
            .to_le_bytes()
            .into_iter()
            .chain(
                ranges
                    .iter()
                    .flat_map(|&(a, b, c)| [a, b, c].into_iter().flat_map(u32::to_le_bytes)),
            )
            .collect()
    }

    #[test]
    fn archive_members_have_independent_bounded_ranges_and_subfile_ids() {
        let table = record(5, &[(0, 8, 5), (8, 12, 4)]);
        let out = entries(
            &0u32.to_le_bytes(),
            &table,
            Region {
                offset: 100,
                size: 12,
            },
        )
        .unwrap();
        assert_eq!(
            out,
            vec![
                ArchiveEntry {
                    index: 0,
                    subfile: 0,
                    region: Region {
                        offset: 100,
                        size: 5
                    }
                },
                ArchiveEntry {
                    index: 0,
                    subfile: 2,
                    region: Region {
                        offset: 108,
                        size: 4
                    }
                },
            ]
        );
    }

    #[test]
    fn archive_member_ranges_reject_reversal_padding_escape_and_oversized_length() {
        for range in [(5, 4, 0), (0, 13, 4), (0, 4, 5)] {
            assert!(
                entries(
                    &[0; 4],
                    &record(1, &[range]),
                    Region {
                        offset: 0,
                        size: 12
                    }
                )
                .is_err()
            );
        }
        let table = record(1, &[(0, 4, 4)]);
        let offsets: Vec<_> = [0u32, 0].into_iter().flat_map(u32::to_le_bytes).collect();
        assert!(
            entries(
                &offsets,
                &table,
                Region {
                    offset: 0,
                    size: 12
                }
            )
            .is_err()
        );
    }

    #[test]
    fn truncated_archive_tables_never_panic() {
        let table = record(1, &[(0, 4, 4)]);
        for size in 0..table.len() {
            assert!(
                entries(
                    &[0; 4],
                    &table[..size],
                    Region {
                        offset: 0,
                        size: 12
                    }
                )
                .is_err()
            );
        }
        assert!(
            entries(
                &[0; 3],
                &table,
                Region {
                    offset: 0,
                    size: 12
                }
            )
            .is_err()
        );
    }

    #[test]
    fn lz11_all_reference_encodings_decode_and_reject_truncation_or_overrun() {
        let cases = [
            // Two literal bytes, then a short reference with displacement two.
            (
                vec![0x11, 7, 0, 0, 0x20, b'a', b'b', 0x40, 1],
                b"abababa".to_vec(),
            ),
            // Medium encoding, its minimum length (17), referring to the first literal.
            (vec![0x11, 18, 0, 0, 0x40, b'a', 0, 0, 0], vec![b'a'; 18]),
            // Long encoding with all length components populated: 0x1234 + 0x111.
            (
                vec![0x11, 0x46, 0x13, 0, 0x40, b'a', 0x11, 0x23, 0x40, 0],
                vec![b'a'; 0x1346],
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(decompress_lz11(&input, expected.len()).unwrap(), expected);
            assert!(decompress_lz11(&input, expected.len() - 1).is_err());
            for end in 0..input.len() {
                assert!(decompress_lz11(&input[..end], expected.len()).is_err());
            }
            let mut overrun = input;
            overrun[1] -= 1;
            assert!(decompress_lz11(&overrun, expected.len()).is_err());
        }
    }

    #[test]
    fn lz11_extended_output_header_is_checked_before_allocating() {
        let input = [0x11, 0, 0, 0, 3, 0, 0, 0, 0, b'a', b'b', b'c'];
        assert_eq!(decompress_lz11(&input, 3).unwrap(), b"abc");
        assert!(decompress_lz11(&input, 2).is_err());
        assert!(decompress_lz11(&[0x11, 0, 0, 0, 0, 0, 0, 0], 100).is_err());
        assert!(decompress_lz11(&[0x11, 0, 0, 0, 255, 255, 255, 255], 100).is_err());
    }

    #[test]
    fn lz11_literals_and_overlapping_references_are_bounded() {
        assert_eq!(
            decompress_lz11(&[0x11, 3, 0, 0, 0, b'a', b'b', b'c'], 3).unwrap(),
            b"abc"
        );
        assert_eq!(
            decompress_lz11(&[0x11, 6, 0, 0, 0x40, b'a', 0x40, 0], 6).unwrap(),
            b"aaaaaa"
        );
        assert!(decompress_lz11(&[0x11, 3, 0, 0], 2).is_err());
        assert!(decompress_lz11(&[0x11, 3, 0, 0, 0x80, 0x20, 0], 3).is_err());
        let valid = [0x11, 3, 0, 0, 0, b'a', b'b', b'c'];
        for size in 0..valid.len() {
            assert!(decompress_lz11(&valid[..size], 10).is_err());
        }
    }
}

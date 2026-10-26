//! Bounded, read-only NCSD/NCCH and RomFS reader for the player's local cartridge image.
//! File format references: 3dbrew NCSD/NCCH/RomFS documentation and pyctr's NCCH reader.
//! No keys or emulator are used, and the ExeFS `.code` is read for its data tables, never
//! executed. Plaintext is checked rather than trusting a filename or an encryption flag left
//! unchanged by some decrypted dumps.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

pub const SOURCE_TITLE_ID: u64 = 0x0004_0000_001B_5000;
/// Title id categories (high word): the base application, and its update data.
const CATEGORY_APPLICATION: u64 = 0x0004_0000;
const CATEGORY_UPDATE: u64 = 0x0004_000E;
/// CIA installables start with their header size.
const CIA_HEADER_SIZE: u32 = 0x2020;
const UNIT: u64 = 0x200;
const MAX_METADATA: usize = 16 << 20;
const MAX_ENTRIES: usize = 100_000;
const NONE: u32 = u32::MAX;

pub use super::import_error::ErrorKind;

#[derive(Debug)]
pub struct ImportError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ImportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with(ErrorKind::Invalid, message)
    }

    pub fn with(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ImportError {}

impl From<std::io::Error> for ImportError {
    fn from(error: std::io::Error) -> Self {
        use std::io::ErrorKind as Io;
        let kind = match error.kind() {
            Io::StorageFull | Io::QuotaExceeded => ErrorKind::Space,
            _ => ErrorKind::Invalid,
        };
        Self::with(kind, error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ImportError>;

pub(crate) fn invalid(message: &str) -> ImportError {
    ImportError::new(message)
}

fn update_error(message: &str) -> ImportError {
    ImportError::with(
        ErrorKind::Update,
        format!("{message}; use the base game image without update data"),
    )
}

fn encrypted(message: &str) -> ImportError {
    ImportError::with(
        ErrorKind::Encrypted,
        format!("{message}: the image is encrypted or damaged; use a decrypted dump"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub offset: u64,
    pub size: u64,
}

impl Region {
    pub fn sub(self, offset: u64, size: u64) -> Result<Self> {
        let end = offset
            .checked_add(size)
            .ok_or_else(|| invalid("region overflow"))?;
        if end > self.size {
            return Err(invalid("region extends beyond its containing section"));
        }
        Ok(Self {
            offset: self
                .offset
                .checked_add(offset)
                .ok_or_else(|| invalid("file offset overflow"))?,
            size,
        })
    }
}

#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub file_size: u64,
    pub program_id: u64,
    pub product_code: String,
    pub ncch_format_version: u16,
    pub remaster_version: u16,
    pub ncch_flags: [u8; 8],
    pub plaintext_with_unfixed_flag: bool,
    pub exefs_header_sha256: [u8; 32],
    pub smdh_region_mask: u32,
    pub romfs: Region,
    pub level3: Region,
}

struct Image {
    file: Mutex<File>,
    length: u64,
    modified: SystemTime,
}

impl Image {
    fn unchanged(&self) -> Result<()> {
        let file = self
            .file
            .lock()
            .map_err(|_| invalid("source reader lock was poisoned"))?;
        let metadata = file.metadata()?;
        if metadata.len() != self.length || metadata.modified()? != self.modified {
            return Err(ImportError::with(
                ErrorKind::Changed,
                "the source changed after inspection began; retry with an unchanged image",
            ));
        }
        Ok(())
    }

    fn read(&self, region: Region, limit: usize) -> Result<Vec<u8>> {
        if region.size > limit as u64 {
            return Err(invalid("read exceeds the caller's allocation limit"));
        }
        Region {
            offset: 0,
            size: self.length,
        }
        .sub(region.offset, region.size)?;
        let size =
            usize::try_from(region.size).map_err(|_| invalid("read size exceeds this platform"))?;
        let mut out = vec![0; size];
        let mut file = self
            .file
            .lock()
            .map_err(|_| invalid("source reader lock was poisoned"))?;
        file.seek(SeekFrom::Start(region.offset))?;
        file.read_exact(&mut out)?;
        Ok(out)
    }
}

pub struct GameSource {
    path: PathBuf,
    image: Image,
    info: SourceInfo,
    files: BTreeMap<String, Region>,
    code: (Region, [u8; 32]),
}

impl GameSource {
    pub fn open(path: &Path) -> Result<Self> {
        let missing = |error: std::io::Error| {
            ImportError::with(
                ErrorKind::Missing,
                format!("the game image can't be opened ({error})"),
            )
        };
        let file = File::open(path).map_err(missing)?;
        let metadata = file.metadata().map_err(missing)?;
        if !metadata.is_file() {
            return Err(ImportError::with(
                ErrorKind::Missing,
                "the game image must be a file",
            ));
        }
        let image = Image {
            file: Mutex::new(file),
            length: metadata.len(),
            modified: metadata.modified()?,
        };
        let whole = Region {
            offset: 0,
            size: image.length,
        };
        let not_cartridge = |what: &str| {
            ImportError::with(
                ErrorKind::Format,
                format!("{what}; use a cartridge image (.3ds) of the base game"),
            )
        };
        if image.length < UNIT {
            return Err(not_cartridge("the file is too small to be a game image"));
        }
        let ncsd = image.read(whole.sub(0, UNIT)?, UNIT as usize)?;
        if bytes(&ncsd, 0x100, 4)? != b"NCSD" {
            return Err(if u32_at(&ncsd, 0)? == CIA_HEADER_SIZE {
                not_cartridge("this is an installable (.cia) file")
            } else if bytes(&ncsd, 0x100, 4)? == b"NCCH" {
                not_cartridge("this is a single application (.cxi/.app) file")
            } else {
                not_cartridge("this is not a cartridge image")
            });
        }
        if ncsd[0x18E] != 0 {
            return Err(invalid("unsupported NCSD media-unit size"));
        }
        let declared = u64::from(u32_at(&ncsd, 0x104)?) * UNIT;
        if declared < image.length {
            return Err(invalid(
                "source is larger than its declared NCSD media size",
            ));
        }
        let mut partition = None;
        let mut occupied = Vec::new();
        for i in 0..8 {
            let at = 0x120 + i * 8;
            let offset = u64::from(u32_at(&ncsd, at)?) * UNIT;
            let size = u64::from(u32_at(&ncsd, at + 4)?) * UNIT;
            if size == 0 {
                continue;
            }
            if offset < UNIT {
                return Err(invalid("NCSD partition overlaps its header"));
            }
            let region = whole.sub(offset, size)?;
            if occupied.iter().any(|&other| overlaps(region, other)) {
                return Err(invalid("NCSD partitions overlap"));
            }
            occupied.push(region);
            if i == 0 {
                partition = Some(region);
            }
        }
        let partition = partition.ok_or_else(|| invalid("base-game NCCH partition is missing"))?;
        let ncch = image.read(partition.sub(0, UNIT)?, UNIT as usize)?;
        if bytes(&ncch, 0x100, 4)? != b"NCCH" {
            return Err(invalid("partition zero is not an NCCH application"));
        }
        if u16_at(&ncch, 0x112)? != 2 {
            return Err(invalid("unsupported NCCH application format revision"));
        }
        let ncch_size = u64::from(u32_at(&ncch, 0x104)?) * UNIT;
        let partition = partition.sub(0, ncch_size)?;
        let program_id = u64_at(&ncch, 0x118)?;
        let wrong_title = || {
            ImportError::with(
                ErrorKind::WrongTitle,
                "this image is a different game; use a Pokémon Ultra Sun cartridge image",
            )
        };
        if program_id != SOURCE_TITLE_ID {
            let same_game = program_id & 0xFFFF_FFFF == SOURCE_TITLE_ID & 0xFFFF_FFFF;
            if same_game && program_id >> 32 == CATEGORY_UPDATE {
                return Err(update_error("this image is update data"));
            }
            if same_game && program_id >> 32 != CATEGORY_APPLICATION {
                return Err(update_error("this image is add-on data"));
            }
            return Err(wrong_title());
        }
        let product = bytes(&ncch, 0x150, 16)?;
        let product = product.split(|&b| b == 0).next().unwrap_or_default();
        if product != b"CTR-P-A2AA" {
            return Err(wrong_title());
        }
        let flags: [u8; 8] = bytes(&ncch, 0x188, 8)?.try_into().unwrap();
        if flags[6] != 0 {
            return Err(invalid("unsupported NCCH content-unit size"));
        }
        let exefs = section(partition, &ncch, 0x1A0)?;
        let romfs = section(partition, &ncch, 0x1B0)?;
        if overlaps(exefs, romfs) {
            return Err(invalid("ExeFS and RomFS sections overlap"));
        }
        let romfs_hash_size = u64::from(u32_at(&ncch, 0x1B8)?) * UNIT;
        if romfs_hash_size == 0 {
            return Err(invalid("RomFS integrity superblock is missing"));
        }
        let romfs_header = image.read(romfs.sub(0, romfs_hash_size)?, MAX_METADATA)?;
        if Sha256::digest(&romfs_header).as_slice() != bytes(&ncch, 0x1E0, 32)? {
            return Err(encrypted("RomFS superblock digest mismatch"));
        }
        let hash_size = u64::from(u32_at(&ncch, 0x1A8)?) * UNIT;
        if hash_size < UNIT {
            return Err(invalid("ExeFS integrity header is missing"));
        }
        let hashed = image.read(exefs.sub(0, hash_size)?, MAX_METADATA)?;
        let digest: [u8; 32] = Sha256::digest(&hashed).into();
        if digest.as_slice() != bytes(&ncch, 0x1C0, 32)? {
            return Err(encrypted("ExeFS header digest mismatch"));
        }
        let exefs_header = bytes(&hashed, 0, UNIT as usize)?;
        let mut icon = None;
        let mut code = None;
        let mut exefs_names = HashSet::new();
        let mut exefs_regions = Vec::new();
        for i in 0..10 {
            let entry = bytes(exefs_header, i * 16, 16)?;
            let name = &entry[..8];
            let name = name.split(|&b| b == 0).next().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            if !exefs_names.insert(name.to_vec()) {
                return Err(invalid("duplicate ExeFS filename"));
            }
            let offset = UNIT + u64::from(u32_at(entry, 8)?);
            let region = exefs.sub(offset, u64::from(u32_at(entry, 12)?))?;
            if exefs_regions.iter().any(|&other| overlaps(region, other)) {
                return Err(invalid("ExeFS file entries overlap"));
            }
            exefs_regions.push(region);
            if name == b".code" {
                code = Some((
                    region,
                    bytes(exefs_header, 0x1E0 - i * 32, 32)?.try_into().unwrap(),
                ));
            }
            if name == b"icon" {
                let expected: [u8; 32] =
                    bytes(exefs_header, 0x1E0 - i * 32, 32)?.try_into().unwrap();
                icon = Some((region, expected));
            }
        }
        if code.is_none_or(|(region, _)| region.size == 0) {
            return Err(invalid(
                "the base application's ExeFS code entry is missing",
            ));
        }
        let (icon, icon_digest) =
            icon.ok_or_else(|| invalid("the base application's SMDH icon is missing"))?;
        let smdh = image.read(icon, MAX_METADATA)?;
        if Sha256::digest(&smdh).as_slice() != icon_digest {
            return Err(encrypted("SMDH icon digest mismatch"));
        }
        bytes(&smdh, 0, 0x201C)?;
        if &smdh[..4] != b"SMDH" {
            return Err(encrypted("SMDH is not readable plaintext"));
        }
        let extended_size = u32_at(&ncch, 0x180)?;
        if extended_size < 0x10 {
            return Err(invalid("application ExHeader is missing"));
        }
        let extended_region = partition.sub(UNIT, u64::from(extended_size))?;
        if overlaps(extended_region, exefs) || overlaps(extended_region, romfs) {
            return Err(invalid("application ExHeader overlaps ExeFS or RomFS"));
        }
        let extended = image.read(extended_region, MAX_METADATA)?;
        if Sha256::digest(&extended).as_slice() != bytes(&ncch, 0x160, 32)? {
            return Err(encrypted("application ExHeader digest mismatch"));
        }
        if &extended[..8] != b"momiji\0\0" {
            return Err(wrong_title());
        }
        if u16_at(&extended, 0xE)? != 0 {
            return Err(update_error(
                "this image has an update applied (remaster version is not zero)",
            ));
        }
        let (level3, files) = read_romfs(&image, romfs)?;
        image.unchanged()?;
        Ok(Self {
            path: path.canonicalize()?,
            info: SourceInfo {
                file_size: image.length,
                program_id,
                product_code: String::from_utf8(product.to_vec())
                    .map_err(|_| invalid("invalid product code"))?,
                ncch_format_version: u16_at(&ncch, 0x112)?,
                remaster_version: u16_at(&extended, 0xE)?,
                ncch_flags: flags,
                plaintext_with_unfixed_flag: flags[7] & 4 == 0,
                exefs_header_sha256: digest,
                smdh_region_mask: u32_at(&smdh, 0x2018)?,
                romfs,
                level3,
            },
            image,
            files,
            code: code.unwrap(),
        })
    }

    pub fn info(&self) -> &SourceInfo {
        &self.info
    }

    pub fn source_path(&self) -> &Path {
        &self.path
    }

    pub fn files(&self) -> &BTreeMap<String, Region> {
        &self.files
    }

    pub fn file(&self, path: &str) -> Result<Region> {
        self.files
            .get(path)
            .copied()
            .ok_or_else(|| ImportError::new(format!("source file not found: {path}")))
    }

    pub fn read(&self, region: Region, limit: usize) -> Result<Vec<u8>> {
        self.image.read(region, limit)
    }

    /// Verified executable bytes for extracting the source's type and teaching tables.
    /// The importer never executes source code.
    pub fn code_bytes(&self) -> Result<Vec<u8>> {
        let bytes = self.read(self.code.0, 32 << 20)?;
        if Sha256::digest(&bytes).as_slice() != self.code.1 {
            return Err(encrypted("executable digest mismatch"));
        }
        if bytes.len().is_multiple_of(0x200) {
            return Ok(bytes);
        }
        decode_backwards_lz(&bytes)
    }

    pub fn verify_unchanged(&self) -> Result<()> {
        self.image.unchanged()
    }

    /// Streaming source fingerprint for private reports/cache provenance, never a 4 GiB allocation.
    pub fn sha256(&self) -> Result<[u8; 32]> {
        let mut file = self
            .image
            .file
            .lock()
            .map_err(|_| invalid("source reader lock was poisoned"))?;
        let before = file.metadata()?;
        if before.len() != self.image.length || before.modified()? != self.image.modified {
            return Err(ImportError::with(
                ErrorKind::Changed,
                "the source changed after it was opened",
            ));
        }
        file.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0; 1 << 20];
        let mut total = 0u64;
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
            total += n as u64;
        }
        let after = file.metadata()?;
        if total != self.image.length
            || before.len() != after.len()
            || before.modified()? != after.modified()?
        {
            return Err(ImportError::with(
                ErrorKind::Changed,
                "the source changed while its fingerprint was computed",
            ));
        }
        Ok(hash.finalize().into())
    }
}

/// Backwards LZ stream: eight footer bytes specify the encoded suffix and output growth.
/// All back-references point into bytes already produced at higher output addresses.
fn decode_backwards_lz(input: &[u8]) -> Result<Vec<u8>> {
    let n = input.len();
    if n < 8 {
        return Err(invalid("truncated executable compression footer"));
    }
    let packed = u32_at(input, n - 8)?;
    let encoded = (packed & 0x00ff_ffff) as usize;
    let footer = (packed >> 24) as usize;
    let growth = u32_at(input, n - 4)? as usize;
    let length = n
        .checked_add(growth)
        .ok_or_else(|| invalid("executable output overflow"))?;
    if !(8..=11).contains(&footer)
        || encoded > n
        || encoded <= footer
        || growth == 0
        || length > 32 << 20
    {
        return Err(invalid("invalid executable compression footer"));
    }
    let lower = n - encoded;
    let mut read = n - footer;
    let mut write = length;
    let mut output = vec![0; length];
    output[..lower].copy_from_slice(&input[..lower]);
    while write > lower {
        if read <= lower {
            return Err(invalid("truncated executable compressed stream"));
        }
        read -= 1;
        let flags = input[read];
        for bit in (0..8).rev() {
            if write == lower {
                break;
            }
            if flags & (1 << bit) == 0 {
                if read <= lower {
                    return Err(invalid("truncated executable literal"));
                }
                read -= 1;
                write -= 1;
                output[write] = input[read];
            } else {
                if read < lower + 2 {
                    return Err(invalid("truncated executable match"));
                }
                let high = input[read - 1];
                let low = input[read - 2];
                read -= 2;
                let count = usize::from(high >> 4) + 3;
                let distance = (usize::from(high & 15) << 8 | usize::from(low)) + 3;
                if count > write - lower {
                    return Err(invalid("executable match exceeds output"));
                }
                for _ in 0..count {
                    write -= 1;
                    let at = write
                        .checked_add(distance)
                        .filter(|&at| at < output.len())
                        .ok_or_else(|| invalid("executable match outside decoded bytes"))?;
                    output[write] = output[at];
                }
            }
        }
    }
    if read != lower {
        return Err(invalid("executable compression has unconsumed bytes"));
    }
    Ok(output)
}

#[cfg(test)]
mod executable_tests {
    use super::decode_backwards_lz;

    #[test]
    fn backwards_matches_follow_already_decoded_output() {
        let mut input = vec![0, 0xf0, b'A', b'B', b'C', 0x10];
        input.extend(0x0800_000eu32.to_le_bytes());
        input.extend(7u32.to_le_bytes());
        assert_eq!(
            decode_backwards_lz(&input).unwrap(),
            b"ABCABCABCABCABCABCABC"
        );
        input[0] = 255;
        assert!(decode_backwards_lz(&input).is_err());
    }

    #[test]
    fn executable_output_growth_and_footer_are_bounded() {
        assert!(decode_backwards_lz(&[]).is_err());
        let mut input = vec![0; 16];
        input[8..12].copy_from_slice(&0x0800_0010u32.to_le_bytes());
        input[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_backwards_lz(&input).is_err());
    }
}

fn section(parent: Region, header: &[u8], at: usize) -> Result<Region> {
    let offset = u64::from(u32_at(header, at)?) * UNIT;
    let size = u64::from(u32_at(header, at + 4)?) * UNIT;
    if offset < UNIT || size == 0 {
        return Err(invalid("required NCCH section is missing"));
    }
    parent.sub(offset, size)
}

fn overlaps(a: Region, b: Region) -> bool {
    a.offset < b.offset.saturating_add(b.size) && b.offset < a.offset.saturating_add(a.size)
}

fn align(value: u64, alignment: u64) -> Result<u64> {
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
        .ok_or_else(|| invalid("alignment overflow"))
}

fn read_romfs(image: &Image, romfs: Region) -> Result<(Region, BTreeMap<String, Region>)> {
    let ivfc = image.read(romfs.sub(0, 0x60)?, 0x60)?;
    if &ivfc[..4] != b"IVFC" || u32_at(&ivfc, 4)? != 0x10000 {
        return Err(encrypted("RomFS is not a readable IVFC section"));
    }
    let exponent = u32_at(&ivfc, 0x4C)?;
    if !(9..=20).contains(&exponent) {
        return Err(invalid("unsupported RomFS block size"));
    }
    let physical = align(0x60 + u64::from(u32_at(&ivfc, 8)?), 1u64 << exponent)?;
    let level3 = romfs.sub(physical, u64_at(&ivfc, 0x44)?)?;
    let header = image.read(level3.sub(0, 40)?, 40)?;
    if u32_at(&header, 0)? != 40 {
        return Err(invalid("invalid RomFS level-three header"));
    }
    let mut tables = Vec::new();
    for at in [4, 12, 20, 28] {
        let offset = u64::from(u32_at(&header, at)?);
        let size = u64::from(u32_at(&header, at + 4)?);
        if offset < 40 || size == 0 {
            return Err(invalid("invalid RomFS metadata table"));
        }
        let table = level3.sub(offset, size)?;
        if tables.iter().any(|&other| overlaps(table, other)) {
            return Err(invalid("RomFS metadata tables overlap"));
        }
        tables.push(table);
    }
    let data_offset = u64::from(u32_at(&header, 36)?);
    let file_data = level3.sub(
        data_offset,
        level3
            .size
            .checked_sub(data_offset)
            .ok_or_else(|| invalid("invalid RomFS data offset"))?,
    )?;
    if tables.iter().any(|&table| overlaps(table, file_data)) {
        return Err(invalid("RomFS metadata overlaps file data"));
    }
    let directories = image.read(tables[1], MAX_METADATA)?;
    let files = image.read(tables[3], MAX_METADATA)?;
    let index = index_files(&directories, &files, file_data)?;
    Ok((level3, index))
}

fn index_files(dirs: &[u8], files: &[u8], data: Region) -> Result<BTreeMap<String, Region>> {
    let mut index = BTreeMap::new();
    let mut seen_dirs = HashSet::from([0u32]);
    let mut seen_files = HashSet::new();
    let mut pending = vec![(0u32, 0u32, String::new(), 0usize)];
    while let Some((offset, expected_parent, parent_path, depth)) = pending.pop() {
        if depth > 64 || !offset.is_multiple_of(4) {
            return Err(invalid(
                "RomFS directory tree has a cycle or exceeds its bounds",
            ));
        }
        let record = bytes(dirs, offset as usize, 24)?;
        if u32_at(record, 0)? != expected_parent {
            return Err(invalid("RomFS directory parent mismatch"));
        }
        let name_length = u32_at(record, 20)? as usize;
        let name = name_at(dirs, offset as usize + 24, name_length, offset == 0)?;
        if offset == 0 && !name.is_empty() {
            return Err(invalid("RomFS root directory must have an empty name"));
        }
        let path = if parent_path.is_empty() {
            name
        } else {
            format!("{parent_path}/{name}")
        };
        if path.len() > 1024 {
            return Err(invalid("RomFS path is too long"));
        }
        let mut child = u32_at(record, 8)?;
        let mut siblings = HashSet::new();
        while child != NONE {
            if !siblings.insert(child) || siblings.len() > MAX_ENTRIES {
                return Err(invalid("RomFS directory sibling cycle"));
            }
            let child_record = bytes(dirs, child as usize, 24)?;
            if !child.is_multiple_of(4)
                || u32_at(child_record, 0)? != offset
                || !seen_dirs.insert(child)
                || seen_dirs.len() > MAX_ENTRIES
            {
                return Err(invalid(
                    "RomFS directory is aliased, has an invalid parent, or exceeds its bounds",
                ));
            }
            pending.push((child, offset, path.clone(), depth + 1));
            child = u32_at(child_record, 4)?;
        }
        let mut file_offset = u32_at(record, 12)?;
        while file_offset != NONE {
            if !file_offset.is_multiple_of(4)
                || !seen_files.insert(file_offset)
                || seen_files.len() > MAX_ENTRIES
            {
                return Err(invalid("RomFS file list has a cycle or exceeds its bounds"));
            }
            let entry = bytes(files, file_offset as usize, 32)?;
            if u32_at(entry, 0)? != offset {
                return Err(invalid("RomFS file parent mismatch"));
            }
            let name = name_at(
                files,
                file_offset as usize + 32,
                u32_at(entry, 28)? as usize,
                false,
            )?;
            let name = if path.is_empty() {
                name
            } else {
                format!("{path}/{name}")
            };
            if name.len() > 1024 {
                return Err(invalid("RomFS file path is too long"));
            }
            let region = data.sub(u64_at(entry, 8)?, u64_at(entry, 16)?)?;
            if index.insert(name, region).is_some() {
                return Err(invalid("RomFS contains duplicate paths"));
            }
            file_offset = u32_at(entry, 4)?;
        }
    }
    Ok(index)
}

fn name_at(table: &[u8], offset: usize, length: usize, root: bool) -> Result<String> {
    if !length.is_multiple_of(2) || length > 510 {
        return Err(invalid("invalid RomFS UTF-16 name size"));
    }
    let data = bytes(table, offset, length)?;
    let words: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    let name = String::from_utf16(&words).map_err(|_| invalid("invalid RomFS UTF-16 name"))?;
    if (name.is_empty() && !root)
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        return Err(invalid("unsafe RomFS path component"));
    }
    Ok(name)
}

pub(crate) fn bytes(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    data.get(
        offset
            ..offset
                .checked_add(length)
                .ok_or_else(|| invalid("metadata offset overflow"))?,
    )
    .ok_or_else(|| invalid("truncated metadata"))
}

pub(crate) fn u16_at(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
pub(crate) fn u32_at(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(data, at, 4)?.try_into().unwrap()))
}
pub(crate) fn u64_at(data: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(bytes(data, at, 8)?.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> (Vec<u8>, Vec<u8>) {
        let mut dirs = vec![0; 24];
        for at in [4, 8, 16] {
            dirs[at..at + 4].copy_from_slice(&NONE.to_le_bytes());
        }
        let mut files = vec![0; 34];
        files[4..8].copy_from_slice(&NONE.to_le_bytes());
        files[16..24].copy_from_slice(&3u64.to_le_bytes());
        files[28..32].copy_from_slice(&2u32.to_le_bytes());
        files[32..34].copy_from_slice(&u16::from(b'x').to_le_bytes());
        (dirs, files)
    }

    fn append_directory(table: &mut Vec<u8>, parent: u32, name: &str) -> u32 {
        let offset = table.len() as u32;
        let text: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let start = table.len();
        table.resize(start + 24 + text.len().next_multiple_of(4), 0);
        table[start..start + 4].copy_from_slice(&parent.to_le_bytes());
        for at in [4, 8, 12, 16] {
            table[start + at..start + at + 4].copy_from_slice(&NONE.to_le_bytes());
        }
        table[start + 20..start + 24].copy_from_slice(&(text.len() as u32).to_le_bytes());
        table[start + 24..start + 24 + text.len()].copy_from_slice(&text);
        offset
    }

    #[test]
    fn directory_discovery_refuses_shared_children_and_wrong_parents() {
        let mut dirs = Vec::new();
        let root = append_directory(&mut dirs, 0, "");
        let first = append_directory(&mut dirs, root, "first");
        let second = append_directory(&mut dirs, root, "second");
        let shared = append_directory(&mut dirs, first, "shared");
        dirs[8..12].copy_from_slice(&first.to_le_bytes());
        dirs[first as usize + 4..first as usize + 8].copy_from_slice(&second.to_le_bytes());
        dirs[first as usize + 8..first as usize + 12].copy_from_slice(&shared.to_le_bytes());
        dirs[second as usize + 8..second as usize + 12].copy_from_slice(&shared.to_le_bytes());
        assert!(index_files(&dirs, &[], Region { offset: 0, size: 1 }).is_err());
        // An unrelated parent is invalid even without an alias or a sibling cycle.
        dirs[first as usize + 4..first as usize + 8].copy_from_slice(&NONE.to_le_bytes());
        dirs[first as usize..first as usize + 4].copy_from_slice(&first.to_le_bytes());
        assert!(index_files(&dirs, &[], Region { offset: 0, size: 1 }).is_err());
    }

    #[test]
    fn filesystem_root_is_unnamed_and_complete_file_paths_are_bounded() {
        let mut named_root = Vec::new();
        append_directory(&mut named_root, 0, "not-a-root-name");
        assert!(index_files(&named_root, &[], Region { offset: 0, size: 1 }).is_err());

        let mut dirs = Vec::new();
        let mut parent = append_directory(&mut dirs, 0, "");
        // Directory paths remain below 1024 bytes; the final file component crosses it.
        for _ in 0..10 {
            let child = append_directory(&mut dirs, parent, &"a".repeat(100));
            dirs[parent as usize + 8..parent as usize + 12].copy_from_slice(&child.to_le_bytes());
            parent = child;
        }
        dirs[parent as usize + 12..parent as usize + 16].copy_from_slice(&0u32.to_le_bytes());
        let file_name: Vec<_> = "b"
            .repeat(20)
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut files = vec![0u8; 32 + file_name.len()];
        files[..4].copy_from_slice(&parent.to_le_bytes());
        files[4..8].copy_from_slice(&NONE.to_le_bytes());
        files[24..28].copy_from_slice(&NONE.to_le_bytes());
        files[28..32].copy_from_slice(&(file_name.len() as u32).to_le_bytes());
        files[32..].copy_from_slice(&file_name);
        assert!(index_files(&dirs, &files, Region { offset: 0, size: 1 }).is_err());
    }

    #[test]
    fn regions_reject_overflow_and_escape_before_allocating() {
        let region = Region {
            offset: 100,
            size: 20,
        };
        assert_eq!(
            region.sub(4, 3).unwrap(),
            Region {
                offset: 104,
                size: 3
            }
        );
        assert!(region.sub(19, 2).is_err());
        assert!(region.sub(u64::MAX, 2).is_err());
        assert!(
            Region {
                offset: u64::MAX,
                size: 2
            }
            .sub(1, 1)
            .is_err()
        );
    }

    #[test]
    fn romfs_tree_indexes_files_and_refuses_cycles_or_out_of_bounds_data() {
        let (mut dirs, mut files) = tables();
        let data = Region {
            offset: 500,
            size: 10,
        };
        assert_eq!(
            index_files(&dirs, &files, data).unwrap()["x"],
            Region {
                offset: 500,
                size: 3
            }
        );
        files[4..8].fill(0);
        assert!(index_files(&dirs, &files, data).is_err());
        files[4..8].copy_from_slice(&NONE.to_le_bytes());
        files[8..16].copy_from_slice(&9u64.to_le_bytes());
        assert!(index_files(&dirs, &files, data).is_err());
        dirs[8..12].fill(0);
        assert!(index_files(&dirs, &files, data).is_err());
    }

    #[test]
    fn names_never_allow_traversal_or_invalid_utf16() {
        for text in ["..", ".", "a/b", "a\\b", "a:b", "\0"] {
            let bytes: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
            assert!(name_at(&bytes, 0, bytes.len(), false).is_err());
        }
        assert!(name_at(&[0, 0xD8], 0, 2, false).is_err());
        assert!(name_at(&[], 0, 0, true).is_ok());
    }

    #[test]
    fn truncated_binary_metadata_never_panics() {
        for size in 0..40 {
            let input = vec![0; size];
            for at in 0..=size {
                let _ = u16_at(&input, at);
                let _ = u32_at(&input, at);
                let _ = u64_at(&input, at);
            }
            let _ = index_files(
                &input,
                &input,
                Region {
                    offset: 0,
                    size: 10,
                },
            );
        }
    }

    /// Tiny synthetic cartridge container: metadata, a four-byte dummy code file and a
    /// minimal RomFS file. No Nintendo executable, artwork or source bytes are included.
    fn synthetic_image() -> Vec<u8> {
        let mut image = vec![0; 0x5200];
        image[0x100..0x104].copy_from_slice(b"NCSD");
        image[0x104..0x108].copy_from_slice(&41u32.to_le_bytes());
        image[0x120..0x124].copy_from_slice(&1u32.to_le_bytes());
        image[0x124..0x128].copy_from_slice(&40u32.to_le_bytes());
        let base = UNIT as usize;
        let ncch = &mut image[base..base + 0x200];
        ncch[0x100..0x104].copy_from_slice(b"NCCH");
        ncch[0x104..0x108].copy_from_slice(&40u32.to_le_bytes());
        ncch[0x112..0x114].copy_from_slice(&2u16.to_le_bytes());
        ncch[0x118..0x120].copy_from_slice(&SOURCE_TITLE_ID.to_le_bytes());
        ncch[0x150..0x15A].copy_from_slice(b"CTR-P-A2AA");
        ncch[0x180..0x184].copy_from_slice(&0x400u32.to_le_bytes());
        for (at, value) in [
            (0x1A0, 4u32),
            (0x1A4, 18),
            (0x1A8, 1),
            (0x1B0, 24),
            (0x1B4, 16),
            (0x1B8, 1),
        ] {
            ncch[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let extended = &mut image[base + 0x200..base + 0x600];
        extended[..8].copy_from_slice(b"momiji\0\0");
        let digest = Sha256::digest(&*extended);
        image[base + 0x160..base + 0x180].copy_from_slice(&digest);
        let exefs = base + 0x800;
        image[exefs..exefs + 5].copy_from_slice(b".code");
        image[exefs + 12..exefs + 16].copy_from_slice(&4u32.to_le_bytes());
        image[exefs + 16..exefs + 20].copy_from_slice(b"icon");
        image[exefs + 24..exefs + 28].copy_from_slice(&16u32.to_le_bytes());
        image[exefs + 28..exefs + 32].copy_from_slice(&0x201Cu32.to_le_bytes());
        let icon = exefs + 0x210;
        image[icon..icon + 4].copy_from_slice(b"SMDH");
        image[icon + 0x2018..icon + 0x201C].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
        let digest = Sha256::digest(&image[icon..icon + 0x201C]);
        image[exefs + 0x1C0..exefs + 0x1E0].copy_from_slice(&digest);
        let digest = Sha256::digest(&image[exefs..exefs + 0x200]);
        image[base + 0x1C0..base + 0x1E0].copy_from_slice(&digest);
        let romfs = base + 0x3000;
        image[romfs..romfs + 4].copy_from_slice(b"IVFC");
        image[romfs + 4..romfs + 8].copy_from_slice(&0x10000u32.to_le_bytes());
        image[romfs + 8..romfs + 12].copy_from_slice(&32u32.to_le_bytes());
        image[romfs + 0x44..romfs + 0x4C].copy_from_slice(&0x1000u64.to_le_bytes());
        image[romfs + 0x4C..romfs + 0x50].copy_from_slice(&12u32.to_le_bytes());
        let digest = Sha256::digest(&image[romfs..romfs + 0x200]);
        image[base + 0x1E0..base + 0x200].copy_from_slice(&digest);
        let l3 = romfs + 0x1000;
        for (i, value) in [40u32, 40, 4, 44, 24, 68, 4, 72, 36, 128]
            .into_iter()
            .enumerate()
        {
            image[l3 + i * 4..l3 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let (dirs, files) = tables();
        image[l3 + 44..l3 + 44 + dirs.len()].copy_from_slice(&dirs);
        image[l3 + 72..l3 + 72 + files.len()].copy_from_slice(&files);
        image[l3 + 128..l3 + 131].copy_from_slice(b"abc");
        image
    }

    fn with_image<T>(input: &[u8], check: impl FnOnce(&Path) -> T) -> T {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sa2co-source-{}-{}.bin",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, input).unwrap();
        let result = check(&path);
        std::fs::remove_file(path).unwrap();
        result
    }

    #[test]
    fn plaintext_is_verified_even_when_the_dump_has_an_unfixed_encryption_flag() {
        with_image(&synthetic_image(), |path| {
            let source = GameSource::open(path).unwrap();
            assert!(source.info().plaintext_with_unfixed_flag);
            assert_eq!(source.files().len(), 1);
            assert_eq!(source.read(source.file("x").unwrap(), 3).unwrap(), b"abc");
            assert!(source.read(source.file("x").unwrap(), 2).is_err());
            source.verify_unchanged().unwrap();
        });
    }

    #[test]
    fn section_digest_mismatches_fail_before_catalog_reading() {
        for (offset, message) in [
            (0x200 + 0x160, "ExHeader"),
            (0x200 + 0x1C0, "ExeFS"),
            (0x200 + 0x1E0, "RomFS"),
            (0x200 + 0x800 + 0x210 + 100, "SMDH"),
        ] {
            let mut input = synthetic_image();
            input[offset] ^= 1;
            with_image(&input, |path| {
                let error = match GameSource::open(path) {
                    Ok(_) => panic!("corrupt image accepted"),
                    Err(error) => error,
                };
                assert!(error.to_string().contains(message), "{error}");
                assert_eq!(error.kind, ErrorKind::Encrypted);
            });
        }
    }

    #[test]
    fn missing_files_and_other_containers_have_their_own_error_kinds() {
        let error = GameSource::open(&std::env::temp_dir().join("sa2co-no-such-image.3ds"))
            .err()
            .expect("missing image accepted");
        assert_eq!(error.kind, ErrorKind::Missing);
        let mut cia = vec![0; 0x400];
        cia[..4].copy_from_slice(&CIA_HEADER_SIZE.to_le_bytes());
        let mut bare = synthetic_image()[0x200..].to_vec();
        bare.resize(0x400, 0);
        for input in [cia, bare, vec![0; 0x400], vec![0; 16]] {
            with_image(&input, |path| {
                let error = GameSource::open(path)
                    .err()
                    .expect("non-cartridge accepted");
                assert_eq!(error.kind, ErrorKind::Format, "{error}");
            });
        }
    }

    #[test]
    fn unsupported_titles_and_container_or_base_revisions_are_rejected() {
        let mut wrong_title = synthetic_image();
        wrong_title[0x200 + 0x118..0x200 + 0x120]
            .copy_from_slice(&(SOURCE_TITLE_ID + 0x100).to_le_bytes());
        with_image(&wrong_title, |path| {
            let error = GameSource::open(path).err().expect("wrong title accepted");
            assert_eq!(error.kind, ErrorKind::WrongTitle);
        });
        let mut update = synthetic_image();
        update[0x200 + 0x118..0x200 + 0x120]
            .copy_from_slice(&(SOURCE_TITLE_ID | (CATEGORY_UPDATE << 32)).to_le_bytes());
        with_image(&update, |path| {
            let error = GameSource::open(path).err().expect("update data accepted");
            assert_eq!(error.kind, ErrorKind::Update);
        });
        for version in [1u16, u16::MAX] {
            let mut wrong_revision = synthetic_image();
            wrong_revision[0x200 + 0x112..0x200 + 0x114].copy_from_slice(&version.to_le_bytes());
            with_image(&wrong_revision, |path| {
                let error = GameSource::open(path)
                    .err()
                    .expect("wrong NCCH revision accepted");
                assert!(error.to_string().contains("format revision"));
            });
        }
        let mut updated_base = synthetic_image();
        updated_base[0x400 + 0xE..0x400 + 0x10].copy_from_slice(&2u16.to_le_bytes());
        // A valid digest must not make an unsupported remaster pass the profile check.
        let digest = Sha256::digest(&updated_base[0x400..0x800]);
        updated_base[0x200 + 0x160..0x200 + 0x180].copy_from_slice(&digest);
        with_image(&updated_base, |path| {
            let error = GameSource::open(path).err().expect("updated base accepted");
            assert_eq!(error.kind, ErrorKind::Update);
        });
    }

    #[test]
    fn exefs_duplicate_names_and_overlapping_file_or_section_ranges_are_rejected() {
        let exefs = 0x200 + 0x800;
        let mut duplicate = synthetic_image();
        duplicate[exefs + 16..exefs + 24].fill(0);
        duplicate[exefs + 16..exefs + 21].copy_from_slice(b".code");
        let digest = Sha256::digest(&duplicate[exefs..exefs + 0x200]);
        duplicate[0x200 + 0x1C0..0x200 + 0x1E0].copy_from_slice(&digest);
        with_image(&duplicate, |path| {
            let error = GameSource::open(path)
                .err()
                .expect("duplicate ExeFS name accepted");
            assert!(error.to_string().contains("duplicate ExeFS filename"));
        });

        let mut overlapping_files = synthetic_image();
        overlapping_files[exefs + 24..exefs + 28].copy_from_slice(&0u32.to_le_bytes());
        let digest = Sha256::digest(&overlapping_files[exefs..exefs + 0x200]);
        overlapping_files[0x200 + 0x1C0..0x200 + 0x1E0].copy_from_slice(&digest);
        with_image(&overlapping_files, |path| {
            let error = GameSource::open(path)
                .err()
                .expect("overlapping ExeFS files accepted");
            assert!(error.to_string().contains("ExeFS file entries overlap"));
        });

        let mut overlapping_sections = synthetic_image();
        overlapping_sections[0x200 + 0x1B0..0x200 + 0x1B4].copy_from_slice(&4u32.to_le_bytes());
        with_image(&overlapping_sections, |path| {
            let error = GameSource::open(path)
                .err()
                .expect("overlapping NCCH sections accepted");
            assert!(
                error
                    .to_string()
                    .contains("ExeFS and RomFS sections overlap")
            );
        });
    }

    #[test]
    fn changed_sources_cannot_publish_a_mixed_inspection_or_new_fingerprint() {
        with_image(&synthetic_image(), |path| {
            use std::io::Write;
            let source = GameSource::open(path).unwrap();
            std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(b"changed")
                .unwrap();
            assert_eq!(
                source.verify_unchanged().unwrap_err().kind,
                ErrorKind::Changed
            );
            assert_eq!(source.sha256().unwrap_err().kind, ErrorKind::Changed);
        });
    }
}

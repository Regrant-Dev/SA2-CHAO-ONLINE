//! Our art on the boot screens: where the Sonic Team logo shows, the SA2 Chao Online art.
//!
//! The boot logos come from the game's `segalogo` texture pack (0x791FC0 loads it; its sprites
//! at 0x9722E8 are 511x511 squares: textures 0/1 the SEGA logo, 2 the Sonic Team logo). A mod
//! loader texture pack of the same name replaces it, but whole, so its other textures (SEGA's)
//! are copied out of the player's own game on first start, never shipped: they're read from
//! `resource/gd_PC/PRS/segalogo.pak`, decoded (DXT1/DXT5) and written as PNGs into
//! `<mod>/textures/segalogo/`, with our art in the team logo's place.

use std::path::Path;

use crate::log;

/// Our art, square, transparent above and below (made from the mod's art, docs/images).
const ART: &[u8] = include_bytes!("../assets/boot_teamlogo.png");
/// The pack texture our art replaces.
const REPLACED: &str = "beta_teamlogo_l";
/// Written last: the pack is done (and for which art).
const DONE: &str = "sa2co-boot-art";
const MAX_PAK: usize = 16 << 20;

/// Builds `<mod>/textures/segalogo/` once (again if our art changed). The mod loader looks for
/// texture packs as the game starts, so the very first start may still show the old logo.
pub fn install(mod_path: &str) {
    let out = Path::new(mod_path).join("textures").join("segalogo");
    let stamp = format!("{:016x}", crate::net::protocol::skin_hash(ART));
    if std::fs::read_to_string(out.join(DONE)).is_ok_and(|s| s.trim() == stamp) {
        return;
    }
    let pak = Path::new(mod_path).join("../../resource/gd_PC/PRS/segalogo.pak");
    match std::fs::read(&pak)
        .map_err(|e| e.to_string())
        .and_then(|b| build(&b, &out))
    {
        Ok(()) => {
            let _ = std::fs::write(out.join(DONE), stamp);
            log!("boot art: logo pack written");
        }
        Err(e) => {
            // Half a pack would break the logo screen: none at all leaves the game's own.
            let _ = std::fs::remove_dir_all(&out);
            log!("boot art: not used ({e})");
        }
    }
}

/// The pack's files: (name, bytes).
fn pak_files(pak: &[u8]) -> Result<Vec<(String, &[u8])>, String> {
    if pak.len() > MAX_PAK || pak.get(..4) != Some(b"\x01pak") {
        return Err("not a texture pack".into());
    }
    let u32_at = |at: usize| -> Result<usize, String> {
        pak.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| "pack ends early".to_string())
    };
    let count = u32_at(0x39)?;
    if count == 0 || count > 64 {
        return Err(format!("{count} files"));
    }
    let mut at = 0x39 + 4;
    let mut entries = Vec::new();
    for _ in 0..count {
        let full = u32_at(at)?;
        at += 4 + full;
        let short = u32_at(at)?;
        let name = pak
            .get(at + 4..at + 4 + short)
            .and_then(|n| std::str::from_utf8(n).ok())
            .ok_or("bad name")?
            .rsplit('\\')
            .next()
            .unwrap_or_default()
            .to_string();
        at += 4 + short;
        let size = u32_at(at)?;
        at += 8;
        entries.push((name, size));
    }
    let mut files = Vec::new();
    for (name, size) in entries {
        let bytes = pak.get(at..at + size).ok_or("pack ends early")?;
        at += size;
        files.push((name, bytes));
    }
    Ok(files)
}

fn build(pak: &[u8], out: &Path) -> Result<(), String> {
    let files = pak_files(pak)?;
    let file = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, b)| *b);
    // The texture list: 60-byte records, name (28 bytes) then the global index.
    let inf = file("segalogo.inf").ok_or("no segalogo.inf")?;
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut index = String::new();
    for rec in inf.as_chunks::<60>().0 {
        let name: String = rec[..28]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("bad texture name".into());
        }
        let gbix = u32::from_le_bytes([rec[28], rec[29], rec[30], rec[31]]);
        let png = if name == REPLACED {
            ART.to_vec()
        } else {
            let dds = file(&format!("{name}.dds")).ok_or(format!("no {name}.dds"))?;
            let (w, h, rgba) = decode_dds(dds).ok_or(format!("{name}.dds not DXT1/DXT5"))?;
            png(w, h, &rgba)
        };
        std::fs::write(out.join(format!("{name}.png")), png).map_err(|e| e.to_string())?;
        index.push_str(&format!("{gbix},{name}.png\n"));
    }
    std::fs::write(out.join("index.txt"), index).map_err(|e| e.to_string())
}

/// A DXT1 or DXT5 DDS's top image as RGBA.
fn decode_dds(dds: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if dds.get(..4)? != b"DDS " || dds.len() < 128 {
        return None;
    }
    let u32_at = |at: usize| u32::from_le_bytes([dds[at], dds[at + 1], dds[at + 2], dds[at + 3]]);
    let (h, w) = (u32_at(12) as usize, u32_at(16) as usize);
    let dxt5 = match &dds[84..88] {
        b"DXT1" => false,
        b"DXT5" => true,
        _ => return None,
    };
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let block = if dxt5 { 16 } else { 8 };
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let data = dds.get(128..128 + bw * bh * block)?;
    let mut rgba = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let b = &data[(by * bw + bx) * block..][..block];
            let (alpha, colour) = if dxt5 {
                (Some(&b[..8]), &b[8..])
            } else {
                (None, b)
            };
            let px = decode_block(colour, alpha, dxt5);
            for (i, p) in px.iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    rgba[(y * w + x) * 4..][..4].copy_from_slice(p);
                }
            }
        }
    }
    Some((w, h, rgba))
}

/// One 4x4 block: colours (8 bytes) and, for DXT5, alpha (8 bytes).
fn decode_block(c: &[u8], alpha: Option<&[u8]>, dxt5: bool) -> [[u8; 4]; 16] {
    let rgb = |v: u16| {
        let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
        [
            (r * 255 / 31) as u8,
            (g * 255 / 63) as u8,
            (b * 255 / 31) as u8,
        ]
    };
    let (c0, c1) = (
        u16::from_le_bytes([c[0], c[1]]),
        u16::from_le_bytes([c[2], c[3]]),
    );
    let (a, b) = (rgb(c0), rgb(c1));
    let mix = |p: u16, q: u16, d: u16| -> [u8; 3] {
        std::array::from_fn(|i| ((u16::from(a[i]) * p + u16::from(b[i]) * q) / d) as u8)
    };
    let four = c0 > c1 || dxt5;
    let colours = if four {
        [a, b, mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [a, b, mix(1, 1, 2), [0, 0, 0]]
    };
    let bits = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
    let alphas: [u8; 16] = match alpha {
        Some(al) => {
            let (a0, a1) = (u16::from(al[0]), u16::from(al[1]));
            let table: [u8; 8] = std::array::from_fn(|i| match i {
                0 => a0 as u8,
                1 => a1 as u8,
                _ if a0 > a1 => ((a0 * (8 - i as u16) + a1 * (i as u16 - 1)) / 7) as u8,
                2..=5 => ((a0 * (6 - i as u16) + a1 * (i as u16 - 1)) / 5) as u8,
                6 => 0,
                _ => 255,
            });
            let mut abits = 0u64;
            for (i, &v) in al[2..8].iter().enumerate() {
                abits |= u64::from(v) << (8 * i);
            }
            std::array::from_fn(|i| table[((abits >> (3 * i)) & 7) as usize])
        }
        None => [255; 16],
    };
    std::array::from_fn(|i| {
        let k = ((bits >> (2 * i)) & 3) as usize;
        let [r, g, b] = colours[k];
        let transparent = !four && k == 3;
        [r, g, b, if transparent { 0 } else { alphas[i] }]
    })
}

/// An RGBA PNG (stored, not compressed: it's written once, on the player's own disk).
fn png(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(h * (w * 4 + 1));
    for row in rgba.chunks_exact(w * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut s1, mut s2) = (1u32, 0u32);
    for &b in &raw {
        s1 = (s1 + u32::from(b)) % 65_521;
        s2 = (s2 + s1) % 65_521;
    }
    z.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    for (kind, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &z[..]), (b"IEND", &[][..])] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxt1_blocks_decode() {
        // Pure red and pure blue endpoints; pixel 0 colour 0, pixel 1 colour 1.
        let mut dds = vec![0u8; 128 + 8];
        dds[..4].copy_from_slice(b"DDS ");
        dds[12..16].copy_from_slice(&4u32.to_le_bytes());
        dds[16..20].copy_from_slice(&4u32.to_le_bytes());
        dds[84..88].copy_from_slice(b"DXT1");
        dds[128..130].copy_from_slice(&0xF800u16.to_le_bytes());
        dds[130..132].copy_from_slice(&0x001Fu16.to_le_bytes());
        dds[132] = 0b0100;
        let (w, h, px) = decode_dds(&dds).unwrap();
        assert_eq!((w, h), (4, 4));
        assert_eq!(&px[..8], &[255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn pngs_are_well_formed() {
        let p = png(2, 2, &[1; 16]);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        // Every PNG ends with the same IEND chunk.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert!(p.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
    }

    /// The real logo pack, with SA2_DIR set to the game's folder (skipped without it).
    #[test]
    fn the_games_logo_pack_rebuilds() {
        let Ok(dir) = std::env::var("SA2_DIR") else {
            return;
        };
        let Ok(bytes) = std::fs::read(format!("{dir}/resource/gd_PC/PRS/segalogo.pak")) else {
            return;
        };
        let out = std::env::temp_dir().join(format!("sa2co-boot-{}", std::process::id()));
        build(&bytes, &out).unwrap();
        let index = std::fs::read_to_string(out.join("index.txt")).unwrap();
        assert_eq!(index.lines().count(), 12, "{index}");
        assert!(index.contains("beta_teamlogo_l.png"));
        assert_eq!(std::fs::read(out.join("beta_teamlogo_l.png")).unwrap(), ART);
        if std::env::var("SA2CO_KEEP_BOOT_ART").is_err() {
            let _ = std::fs::remove_dir_all(&out);
        }
    }
}

//! Mod-generated effect sheets. No source-game sprite bytes, palettes, scripts or callback
//! addresses are read. Source move metadata selects a semantic presentation in anim_kinds.

use super::rom::Rom;

pub const STATUS_COUNT: u8 = 9;
pub const TAG_BASE: u16 = 10_000;
pub const TYPE_STYLES: u16 = 18;
pub const HIT_TAG: u16 = TAG_BASE + TYPE_STYLES * 3;
pub const STATUS_TAG_BASE: u16 = HIT_TAG + 1;
pub const SHEET_COUNT: u16 = TYPE_STYLES * 3 + 1 + STATUS_COUNT as u16;
const TILE_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    pub tile: u16,
    pub duration: u8,
    pub h_flip: bool,
    pub v_flip: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Template {
    pub tag: u16,
    pub width: u8,
    pub height: u8,
    pub frames: Vec<Frame>,
    pub blended: bool,
}
impl Template {
    pub fn generated(tag: u16) -> Self {
        Self {
            tag,
            width: 16,
            height: 16,
            frames: (0..4)
                .map(|frame| Frame {
                    tile: frame * 4,
                    duration: 4,
                    h_flip: false,
                    v_flip: false,
                })
                .collect(),
            blended: true,
        }
    }
}

/// RGB type colours in the modern Normal..Fairy ordering. These are presentation choices,
/// not an imported palette or an old source-game address table.
pub fn type_colour(kind: u8) -> [u8; 3] {
    const COLOURS: [[u8; 3]; 18] = [
        [190, 190, 166],
        [214, 112, 90],
        [151, 158, 232],
        [181, 100, 190],
        [214, 181, 103],
        [186, 165, 91],
        [161, 186, 81],
        [139, 119, 183],
        [159, 171, 188],
        [244, 137, 73],
        [91, 168, 230],
        [120, 196, 110],
        [239, 208, 78],
        [238, 127, 172],
        [132, 207, 220],
        [129, 125, 231],
        [133, 116, 108],
        [231, 155, 205],
    ];
    COLOURS[usize::from(kind).min(COLOURS.len() - 1)]
}

pub struct Sheet {
    tiles: Vec<u8>,
    colours: [[u8; 4]; 16],
}
impl Sheet {
    pub fn tiles(&self) -> usize {
        self.tiles.len() / TILE_BYTES
    }
    pub fn frame(&self, width: u8, height: u8, frame: Frame) -> Option<Vec<u8>> {
        if width == 0
            || height == 0
            || !width.is_multiple_of(8)
            || !height.is_multiple_of(8)
            || width > 64
            || height > 64
        {
            return None;
        }
        let (w, h) = (usize::from(width), usize::from(height));
        let cols = w / 8;
        let start = usize::from(frame.tile).checked_mul(TILE_BYTES)?;
        let end = start.checked_add((w / 8).checked_mul(h / 8)?.checked_mul(TILE_BYTES)?)?;
        let tiles = self.tiles.get(start..end)?;
        let mut out = vec![0; w * h * 4];
        for (i, &byte) in tiles.iter().enumerate() {
            let tile = i / TILE_BYTES;
            let within = i % TILE_BYTES;
            for (k, colour) in [byte & 15, byte >> 4].into_iter().enumerate() {
                let mut x = (tile % cols) * 8 + (within % 4) * 2 + k;
                let mut y = (tile / cols) * 8 + within / 4;
                if frame.h_flip {
                    x = w - 1 - x;
                }
                if frame.v_flip {
                    y = h - 1 - y;
                }
                let at = (y * w + x) * 4;
                out[at..at + 4].copy_from_slice(&self.colours[usize::from(colour)]);
            }
        }
        Some(out)
    }
}

fn ink(style: u16, x: i32, y: i32, frame: i32) -> u8 {
    let dx = x - 7;
    let dy = y - 7;
    let radius2 = dx * dx + dy * dy;
    let lit = match style {
        0 => radius2 <= (4 + frame).pow(2),
        1 => radius2 < 53 && (dx.abs() <= 1 || dy.abs() <= 1 || (dx.abs() - dy.abs()).abs() <= 1),
        2 => (radius2 - (4 + frame).pow(2)).abs() <= 8,
        3 => {
            ((dx + 3).pow(2) + (dy + 3 - frame).pow(2) < 10)
                || ((dx - 3).pow(2) + (dy - 3 + frame).pow(2) < 9)
        }
        4 => radius2 < 50 && ((dy * 2 + frame).rem_euclid(5) < 2 || dx.abs() < 2),
        5 => dy > -(frame + 3) && dy < 6 && dx.abs() < (6 - dy.abs()).max(1),
        6 => radius2 < 40 && (dx - 3).pow(2) + (dy + 2).pow(2) > 24,
        7 => (dx + (dy + frame).rem_euclid(6) - 3).abs() < 2 && dy.abs() < 7,
        _ => radius2 < 55 && (dx.abs() <= 1 || dy.abs() <= 1 || dx.abs() == dy.abs()),
    };
    if lit {
        (15 - (radius2 / 8).min(12)) as u8
    } else {
        0
    }
}

/// A small animated orb/spark/ring or status mark, made deterministically in memory.
pub fn generated_sheet(tag: u16) -> Option<Sheet> {
    let local = tag.checked_sub(TAG_BASE)?;
    if local >= SHEET_COUNT {
        return None;
    }
    let (colour, style) = if tag == HIT_TAG {
        ([250, 238, 173], 1)
    } else if tag >= STATUS_TAG_BASE {
        let index = tag - STATUS_TAG_BASE;
        let colors = [
            [177, 89, 194],
            [214, 126, 225],
            [250, 137, 78],
            [161, 205, 173],
            [220, 222, 252],
            [248, 224, 85],
            [151, 222, 238],
            [220, 205, 116],
            [180, 170, 225],
        ];
        (
            colors[usize::from(index)],
            [3, 4, 5, 2, 6, 7, 8, 2, 4][usize::from(index)],
        )
    } else {
        (
            type_colour((local % TYPE_STYLES) as u8),
            local / TYPE_STYLES,
        )
    };
    let mut colours = [[0; 4]; 16];
    for (i, colour_slot) in colours.iter_mut().enumerate().skip(1) {
        *colour_slot = [
            colour[0].saturating_add((i * 2) as u8),
            colour[1].saturating_add((i * 2) as u8),
            colour[2].saturating_add((i * 2) as u8),
            128 + (i as u8) * 8,
        ];
    }
    let mut tiles = Vec::with_capacity(4 * 4 * TILE_BYTES);
    for frame in 0..4 {
        for ty in 0..2 {
            for tx in 0..2 {
                for y in 0..8 {
                    for x in (0..8).step_by(2) {
                        let a = ink(style, tx * 8 + x, y + ty * 8, frame);
                        let b = ink(style, tx * 8 + x + 1, y + ty * 8, frame);
                        tiles.push(a | (b << 4));
                    }
                }
            }
        }
    }
    Some(Sheet { tiles, colours })
}
impl Rom {
    /// A generated template key is its bounded sheet tag, never a source code pointer.
    pub fn anim_template(&self, key: usize) -> Option<Template> {
        let tag = u16::try_from(key).ok()?;
        (TAG_BASE..TAG_BASE + SHEET_COUNT)
            .contains(&tag)
            .then(|| Template::generated(tag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_generated_style_has_bounded_animated_alpha_frames() {
        let mut signatures = std::collections::HashSet::new();
        for tag in TAG_BASE..TAG_BASE + SHEET_COUNT {
            let sheet = generated_sheet(tag).unwrap();
            assert_eq!(sheet.tiles(), 16);
            let template = Template::generated(tag);
            for frame in &template.frames {
                let rgba = sheet.frame(16, 16, *frame).unwrap();
                assert_eq!(rgba.len(), 16 * 16 * 4);
                assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
                assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
            }
            signatures.insert(sheet.frame(16, 16, template.frames[0]).unwrap());
            assert!(
                sheet
                    .frame(
                        16,
                        16,
                        Frame {
                            tile: 16,
                            ..template.frames[0]
                        }
                    )
                    .is_none()
            );
        }
        assert!(signatures.len() >= 24);
        assert!(generated_sheet(TAG_BASE - 1).is_none());
        assert!(generated_sheet(TAG_BASE + SHEET_COUNT).is_none());
    }
    #[test]
    fn flips_are_applied_within_the_requested_frame_and_bad_sizes_reject() {
        let sheet = generated_sheet(STATUS_TAG_BASE + 4).unwrap();
        let frame = Template::generated(STATUS_TAG_BASE + 4).frames[0];
        let plain = sheet.frame(16, 16, frame).unwrap();
        let flip = sheet
            .frame(
                16,
                16,
                Frame {
                    h_flip: true,
                    ..frame
                },
            )
            .unwrap();
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(
                    &plain[(y * 16 + x) * 4..(y * 16 + x + 1) * 4],
                    &flip[(y * 16 + 15 - x) * 4..(y * 16 + 16 - x) * 4]
                );
            }
        }
        for (w, h) in [(0, 16), (7, 16), (16, 65), (64, 64)] {
            assert!(sheet.frame(w, h, frame).is_none());
        }
    }
}

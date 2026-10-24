//! The ROM's battle animation graphics (re/chao-battle-visuals.md 6): which sprites a move's
//! or a status condition's animation script makes, and those sprites' frames as pictures.
//!
//! Only the script and sprite formats are known here; which sheet belongs to what comes from
//! walking the ROM's own scripts. `move_play` gives a move's script in play order, with each
//! sprite's callback and arguments and the frame it comes at, for `anim_kinds` to sort.

use std::collections::{HashMap, HashSet};

use super::lz77;
use super::rom::{MOVE_COUNT, Rom};

/// Move animation scripts: a ROM pointer per move id.
const MOVE_SCRIPTS: usize = 0x2C_8D6C;
/// Status condition scripts (poison, confusion, burn, ... in the game's order).
const STATUS_SCRIPTS: usize = 0x2C_92FC;
pub const STATUS_COUNT: u8 = 9;
/// General scripts (switching, stat changes, a plain hit, ...).
const GENERAL_SCRIPTS: usize = 0x2C_9320;
const GENERAL_COUNT: u8 = 23;
/// The general script of a plain hit.
pub const HIT_SCRIPT: u8 = 15;
/// Sprite sheets `{ptr to LZ77 4bpp tiles, u16 size, u16 tag}` and their palettes
/// `{ptr to LZ77 BGR555, u16 tag, u16 0}`, both indexed by sheet tag - `TAG_BASE`.
const SHEETS: usize = 0x52_4B44;
const SHEET_PALETTES: usize = 0x52_544C;
const SHEET_ENTRY: usize = 8;
pub const SHEET_COUNT: u16 = 289;
/// Sheet tags start here (templates with smaller tags have no sheet).
pub const TAG_BASE: u16 = 10_000;
/// A sprite template: u16 sheet tag, u16 palette tag, then pointers to its OAM data, animations,
/// images, affine animations and callback.
const TEMPLATE_SIZE: usize = 0x18;
const TEMPLATE_OAM: usize = 4;
const TEMPLATE_ANIMS: usize = 8;
const TEMPLATE_IMAGES: usize = 0xC;
const TEMPLATE_AFFINE: usize = 0x10;
/// OAM byte 1: the object mode (bits 2..3; 1 is blended with what's behind it) and 8bpp.
const OAM_MODE: u8 = 0x0C;
const OAM_BLENDED: u8 = 0x04;
const OAM_8BPP: u8 = 0x20;
/// The ROM's addresses start here.
const ROM_ADDRESS: u32 = 0x0800_0000;
/// One 8x8 4bpp tile.
const TILE_BYTES: usize = 32;
/// Frames read from one animation.
const MAX_FRAMES: usize = 16;
/// Every supported ROM's size (`Rom::from_bytes` takes no other).
const ROM_LEN: usize = 16 << 20;
/// Script bytes walked at most per script (all its branches together).
const MAX_STEPS: usize = 20_000;

/// The "create sprite" command's placement bit: the sprite is layered by the target's draw
/// order rather than the attacker's (where it starts and goes is its callback's business).
const BY_TARGET: u8 = 0x80;
/// A template's callback (a ROM code address).
const TEMPLATE_CALLBACK: usize = 0x14;
/// Calls followed inside each other at most, and gotos followed in one script.
const MAX_DEPTH: usize = 8;
const MAX_JUMPS: usize = 64;
/// Frames counted for a "wait for the sprites and tasks to finish".
pub const WAIT_FRAMES: u16 = 8;

/// Script opcodes with something to follow.
mod op {
    pub const LOAD_SHEET: u8 = 0x00;
    pub const CREATE_SPRITE: u8 = 0x02;
    pub const CREATE_TASK: u8 = 0x03;
    pub const DELAY: u8 = 0x04;
    pub const WAIT: u8 = 0x05;
    pub const END: u8 = 0x08;
    pub const CALL: u8 = 0x0E;
    pub const RETURN: u8 = 0x0F;
    pub const PICK_BY_TURN: u8 = 0x11;
    pub const JUMP_IF_TURN: u8 = 0x12;
    pub const GOTO: u8 = 0x13;
    pub const SOUND_TASK: u8 = 0x1F;
    pub const JUMP_IF_ARGUMENT: u8 = 0x21;
    pub const JUMP_IF_CONTEST: u8 = 0x24;
}

/// Length of a fixed-size opcode (with its operands); `None` for the ones walked by hand and
/// unknown ones.
fn fixed_len(code: u8) -> Option<usize> {
    Some(match code {
        0x05..=0x07 | 0x0D | 0x15..=0x17 | 0x20 | 0x29 | 0x2F => 1,
        0x04 | 0x0A | 0x0B | 0x14 | 0x18 | 0x1A | 0x22 | 0x23 | 0x28 | 0x2A..=0x2E => 2,
        0x01 | 0x09 | 0x0C | 0x1E => 3,
        0x10 | 0x19 | 0x25 => 4,
        0x1D => 5,
        0x1C => 6,
        0x1B | 0x26 | 0x27 => 7,
        _ => return None,
    })
}

/// One "create sprite" in a script.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Created {
    /// The template's file offset.
    pub template: usize,
    /// Layered by the target's draw order (not where it shows: see `BY_TARGET`).
    pub by_target: bool,
}

/// A command a script runs, as `move_play` meets it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Loads the sheet with this tag.
    Load(u16),
    /// Makes a sprite from the template at file offset `template`; `callback` is the template's
    /// callback (the whole ROM address), `args` the command's arguments.
    Sprite {
        template: usize,
        callback: u32,
        by_target: bool,
        args: Vec<i16>,
    },
    /// Starts the task at ROM address `function` with `args`.
    Task { function: u32, args: Vec<i16> },
}

/// A step and the frame it comes at (delays added up; a wait counts `WAIT_FRAMES`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timed {
    pub frame: u16,
    pub step: Step,
}

/// A script in play order: every call each time it's made, the attack turn of a two-turn
/// move, and (with `branches`) the conditional jumps taken.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Play {
    pub steps: Vec<Timed>,
    /// It picks its commands by the turn (a charge turn, then the attack).
    pub two_turn: bool,
}

/// Walks a script in play order (`Rom::move_play`).
struct Player<'a> {
    rom: &'a Rom,
    out: Play,
    clock: u16,
    budget: usize,
    branches: bool,
}

impl Player<'_> {
    /// Runs the script at `at` until it ends or returns; `None` on a bad command.
    fn run(&mut self, mut at: usize, depth: usize) -> Option<()> {
        let rom = self.rom;
        let mut jumps = 0;
        loop {
            // Out of steps: what was met so far stands.
            let Some(left) = self.budget.checked_sub(1) else {
                return Some(());
            };
            self.budget = left;
            let code = *rom.bytes_at(at, 1)?.first()?;
            let byte = |o: usize| rom.bytes_at(at + o, 1).and_then(|b| b.first().copied());
            let args = |o: usize, n: usize| -> Option<Vec<i16>> {
                (0..n)
                    .map(|i| rom.u16_at(at + o + 2 * i).map(|v| v as i16))
                    .collect()
            };
            let frame = self.clock;
            match code {
                op::LOAD_SHEET => {
                    let tag = rom.u16_at(at + 1)?;
                    self.out.steps.push(Timed {
                        frame,
                        step: Step::Load(tag),
                    });
                    at += 3;
                }
                op::CREATE_SPRITE => {
                    let template = rom.ptr_at(at + 1)?;
                    let n = usize::from(byte(6)?);
                    self.out.steps.push(Timed {
                        frame,
                        step: Step::Sprite {
                            template,
                            callback: rom.u32_at(template + TEMPLATE_CALLBACK).unwrap_or(0),
                            by_target: byte(5)? & BY_TARGET != 0,
                            args: args(7, n)?,
                        },
                    });
                    at += 7 + 2 * n;
                }
                op::CREATE_TASK => {
                    let n = usize::from(byte(6)?);
                    self.out.steps.push(Timed {
                        frame,
                        step: Step::Task {
                            function: rom.u32_at(at + 1)?,
                            args: args(7, n)?,
                        },
                    });
                    at += 7 + 2 * n;
                }
                op::SOUND_TASK => at += 6 + 2 * usize::from(byte(5)?),
                op::DELAY => {
                    self.clock = self.clock.saturating_add(u16::from(byte(1)?));
                    at += 2;
                }
                op::WAIT => {
                    self.clock = self.clock.saturating_add(WAIT_FRAMES);
                    at += 1;
                }
                op::END | op::RETURN => return Some(()),
                op::CALL => {
                    if depth < MAX_DEPTH {
                        self.run(rom.ptr_at(at + 1)?, depth + 1)?;
                    }
                    at += 5;
                }
                op::PICK_BY_TURN => {
                    self.out.two_turn = true;
                    at = rom.ptr_at(at + 5)?;
                }
                op::JUMP_IF_TURN if self.branches => at = rom.ptr_at(at + 2)?,
                op::JUMP_IF_TURN => at += 6,
                op::JUMP_IF_ARGUMENT if self.branches => at = rom.ptr_at(at + 4)?,
                op::JUMP_IF_ARGUMENT => at += 8,
                op::JUMP_IF_CONTEST => at += 5,
                op::GOTO => {
                    jumps += 1;
                    if jumps > MAX_JUMPS {
                        return Some(());
                    }
                    at = rom.ptr_at(at + 1)?;
                }
                other => at += fixed_len(other)?,
            }
        }
    }
}

/// What a script shows: the sprites it creates and the sheets it loads (some are drawn by
/// tasks rather than sprites), each in the order met.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Script {
    pub created: Vec<Created>,
    pub loaded: Vec<u16>,
}

/// A frame of a sprite's first animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    /// First tile in the sheet.
    pub tile: u16,
    /// Game frames it shows (60 a second).
    pub duration: u8,
    pub h_flip: bool,
    pub v_flip: bool,
}

/// A sprite template with a sheet.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Template {
    pub tag: u16,
    pub width: u8,
    pub height: u8,
    pub frames: Vec<Frame>,
    /// The GBA draws it see-through, blended with what's behind it.
    pub blended: bool,
}

impl Template {
    /// The sheet's first 8x8 tile alone (for a sheet with no template of its own).
    pub fn first_tile(tag: u16) -> Template {
        Template {
            tag,
            width: 8,
            height: 8,
            frames: vec![Frame {
                tile: 0,
                duration: 0,
                h_flip: false,
                v_flip: false,
            }],
            blended: false,
        }
    }
}

/// A sheet's tiles and its 16 colours (RGBA8, colour 0 clear).
pub struct Sheet {
    tiles: Vec<u8>,
    colours: [[u8; 4]; 16],
}

impl Sheet {
    /// Its 8x8 tiles.
    pub fn tiles(&self) -> usize {
        self.tiles.len() / TILE_BYTES
    }

    /// One `width` x `height` frame starting at `tile` (tiles row-major, low nibble the left
    /// pixel) as RGBA8; `None` when it runs past the sheet.
    pub fn frame(&self, width: u8, height: u8, f: Frame) -> Option<Vec<u8>> {
        let (w, h) = (usize::from(width), usize::from(height));
        let (cols, rows) = (w / 8, h / 8);
        let start = usize::from(f.tile) * TILE_BYTES;
        let end = start + cols * rows * TILE_BYTES;
        let tiles = self.tiles.get(start..end)?;
        let mut out = vec![0u8; w * h * 4];
        for (i, &byte) in tiles.iter().enumerate() {
            let tile = i / TILE_BYTES;
            let within = i % TILE_BYTES;
            let (tx, ty) = (tile % cols, tile / cols);
            let (px, py) = ((within % 4) * 2, within / 4);
            for (k, index) in [byte & 0x0F, byte >> 4].into_iter().enumerate() {
                let (mut x, mut y) = (tx * 8 + px + k, ty * 8 + py);
                if f.h_flip {
                    x = w - 1 - x;
                }
                if f.v_flip {
                    y = h - 1 - y;
                }
                let o = (y * w + x) * 4;
                out[o..o + 4].copy_from_slice(&self.colours[usize::from(index)]);
            }
        }
        Some(out)
    }
}

/// An OAM shape and size as pixels (shape 0 square, 1 wide, 2 tall).
pub fn oam_size(shape: u8, size: u8) -> Option<(u8, u8)> {
    const SQUARE: [(u8, u8); 4] = [(8, 8), (16, 16), (32, 32), (64, 64)];
    const WIDE: [(u8, u8); 4] = [(16, 8), (32, 8), (32, 16), (64, 32)];
    const TALL: [(u8, u8); 4] = [(8, 16), (8, 32), (16, 32), (32, 64)];
    let table = match shape {
        0 => SQUARE,
        1 => WIDE,
        2 => TALL,
        _ => return None,
    };
    table.get(usize::from(size)).copied()
}

/// A BGR555 colour as RGBA8 (opaque).
fn rgba(v: u16) -> [u8; 4] {
    let ch = |s: u16| {
        let x = (v >> s & 31) as u8;
        x << 3 | x >> 2
    };
    [ch(0), ch(5), ch(10), 255]
}

impl Rom {
    /// What move `id`'s animation script shows, each command once (following calls and both
    /// sides of branches). `None` when the script can't be walked.
    pub fn move_anim(&self, id: u16) -> Option<Script> {
        if id == 0 || id >= MOVE_COUNT {
            return None;
        }
        self.walk(self.ptr_at(MOVE_SCRIPTS + usize::from(id) * 4)?)
    }

    /// Move `id`'s script in play order (the attack turn of a two-turn move; with `branches`,
    /// taking the jumps on the turn and on an argument). `None` when it can't be walked.
    pub fn move_play(&self, id: u16, branches: bool) -> Option<Play> {
        if id == 0 || id >= MOVE_COUNT {
            return None;
        }
        let mut player = Player {
            rom: self,
            out: Play::default(),
            clock: 0,
            budget: MAX_STEPS,
            branches,
        };
        player.run(self.ptr_at(MOVE_SCRIPTS + usize::from(id) * 4)?, 0)?;
        Some(player.out)
    }

    /// The sheet tag of the template at `offset`, when it has a sheet.
    pub fn template_tag(&self, offset: usize) -> Option<u16> {
        let tag = self.u16_at(offset)?;
        (TAG_BASE..TAG_BASE + SHEET_COUNT)
            .contains(&tag)
            .then_some(tag)
    }

    /// What status condition script `index` shows.
    pub fn status_anim(&self, index: u8) -> Option<Script> {
        if index >= STATUS_COUNT {
            return None;
        }
        self.walk(self.ptr_at(STATUS_SCRIPTS + usize::from(index) * 4)?)
    }

    /// What general script `index` shows.
    pub fn general_anim(&self, index: u8) -> Option<Script> {
        if index >= GENERAL_COUNT {
            return None;
        }
        self.walk(self.ptr_at(GENERAL_SCRIPTS + usize::from(index) * 4)?)
    }

    fn walk(&self, start: usize) -> Option<Script> {
        let mut out = Script::default();
        let mut todo = vec![start];
        let mut seen = HashSet::new();
        let mut steps = 0;
        while let Some(mut at) = todo.pop() {
            loop {
                steps += 1;
                if steps > MAX_STEPS {
                    return None;
                }
                if !seen.insert(at) {
                    break;
                }
                let code = *self.bytes_at(at, 1)?.first()?;
                match code {
                    op::LOAD_SHEET => {
                        out.loaded.push(self.u16_at(at + 1)?);
                        at += 3;
                    }
                    op::CREATE_SPRITE => {
                        let template = self.ptr_at(at + 1)?;
                        let placement = *self.bytes_at(at + 5, 1)?.first()?;
                        let args = usize::from(*self.bytes_at(at + 6, 1)?.first()?);
                        out.created.push(Created {
                            template,
                            by_target: placement & BY_TARGET != 0,
                        });
                        at += 7 + 2 * args;
                    }
                    op::CREATE_TASK => {
                        let args = usize::from(*self.bytes_at(at + 6, 1)?.first()?);
                        at += 7 + 2 * args;
                    }
                    op::SOUND_TASK => {
                        let args = usize::from(*self.bytes_at(at + 5, 1)?.first()?);
                        at += 6 + 2 * args;
                    }
                    op::END | op::RETURN => break,
                    op::CALL => {
                        todo.push(self.ptr_at(at + 1)?);
                        at += 5;
                    }
                    op::PICK_BY_TURN => {
                        todo.push(self.ptr_at(at + 1)?);
                        todo.push(self.ptr_at(at + 5)?);
                        break;
                    }
                    op::JUMP_IF_TURN => {
                        todo.push(self.ptr_at(at + 2)?);
                        at += 6;
                    }
                    op::GOTO => at = self.ptr_at(at + 1)?,
                    op::JUMP_IF_ARGUMENT => {
                        todo.push(self.ptr_at(at + 4)?);
                        at += 8;
                    }
                    op::JUMP_IF_CONTEST => {
                        todo.push(self.ptr_at(at + 1)?);
                        at += 5;
                    }
                    other => at += fixed_len(other)?,
                }
            }
        }
        Some(out)
    }

    /// The template at `offset`, when it draws from a 4bpp sheet: its tag, size and the frames
    /// of its first animation (one frame at tile 0 when it has none).
    pub fn anim_template(&self, offset: usize) -> Option<Template> {
        self.bytes_at(offset, TEMPLATE_SIZE)?;
        let tag = self.u16_at(offset)?;
        if !(TAG_BASE..TAG_BASE + SHEET_COUNT).contains(&tag) {
            return None;
        }
        let oam = self.bytes_at(self.ptr_at(offset + TEMPLATE_OAM)?, 8)?;
        if oam[1] & OAM_8BPP != 0 {
            // 8bpp: not used by the battle animations.
            return None;
        }
        let (width, height) = oam_size(oam[1] >> 6, oam[3] >> 6)?;
        let mut frames = Vec::new();
        if let Some(first) = self
            .ptr_at(offset + TEMPLATE_ANIMS)
            .and_then(|anims| self.ptr_at(anims))
        {
            for i in 0..MAX_FRAMES {
                let Some(word) = self.u32_at(first + i * 4) else {
                    break;
                };
                let value = word as u16;
                // End, jump and loop commands are negative.
                if (value as i16) < 0 {
                    break;
                }
                frames.push(Frame {
                    tile: value,
                    duration: (word >> 16 & 0x3F) as u8,
                    h_flip: word & 1 << 22 != 0,
                    v_flip: word & 1 << 23 != 0,
                });
            }
        }
        if frames.is_empty() {
            frames.push(Frame {
                tile: 0,
                duration: 0,
                h_flip: false,
                v_flip: false,
            });
        }
        Some(Template {
            tag,
            width,
            height,
            frames,
            blended: oam[1] & OAM_MODE == OAM_BLENDED,
        })
    }

    /// The sprite templates anywhere in the ROM for each of `tags` (a sheet that only tasks
    /// draw from has its sprites made from templates no script names): records whose sheet
    /// and palette tags are both the tag, with no images of their own, OAM data, animations,
    /// a callback in the ROM and no affine animations or ROM ones.
    pub fn sheet_templates(&self, tags: &HashSet<u16>) -> HashMap<u16, Vec<Template>> {
        let mut out: HashMap<u16, Vec<Template>> = HashMap::new();
        if tags.is_empty() {
            return out;
        }
        let in_rom = |a: u32| (ROM_ADDRESS..ROM_ADDRESS + ROM_LEN as u32).contains(&a);
        for offset in (0..ROM_LEN - TEMPLATE_SIZE).step_by(4) {
            let Some(tag) = self.u16_at(offset) else {
                break;
            };
            if !(TAG_BASE..TAG_BASE + SHEET_COUNT).contains(&tag)
                || self.u16_at(offset + 2) != Some(tag)
                || !tags.contains(&tag)
            {
                continue;
            }
            let word = |o: usize| self.u32_at(offset + o).unwrap_or(0);
            let affine = word(TEMPLATE_AFFINE);
            let fits = word(TEMPLATE_IMAGES) == 0
                && in_rom(word(TEMPLATE_OAM))
                && in_rom(word(TEMPLATE_ANIMS))
                && (affine == 0 || in_rom(affine))
                && word(TEMPLATE_CALLBACK) & 1 == 1
                && in_rom(word(TEMPLATE_CALLBACK));
            if let Some(t) = fits.then(|| self.anim_template(offset)).flatten() {
                out.entry(tag).or_default().push(t);
            }
        }
        out
    }

    /// The sheet with `tag`: its tiles (the LZ77 data's own length) and the first 16 colours of
    /// its palette.
    pub fn anim_sheet(&self, tag: u16) -> Option<Sheet> {
        let index = usize::from(tag.checked_sub(TAG_BASE)?);
        if index >= usize::from(SHEET_COUNT) {
            return None;
        }
        let sheet = SHEETS + index * SHEET_ENTRY;
        let palette = SHEET_PALETTES + index * SHEET_ENTRY;
        // A palette entry may carry a neighbour's tag (a shared palette); the index decides.
        if self.u16_at(sheet + 6)? != tag {
            return None;
        }
        let tiles = lz77::decompress(self.tail(self.ptr_at(sheet)?)?)?;
        let pal = lz77::decompress(self.tail(self.ptr_at(palette)?)?)?;
        if pal.len() < 32 || tiles.is_empty() {
            return None;
        }
        let mut colours = [[0u8; 4]; 16];
        for (c, colour) in colours.iter_mut().enumerate() {
            *colour = rgba(u16::from_le_bytes([pal[c * 2], pal[c * 2 + 1]]));
        }
        colours[0] = [0; 4];
        Some(Sheet { tiles, colours })
    }

    /// Everything from `offset` to the end of the ROM.
    fn tail(&self, offset: usize) -> Option<&[u8]> {
        self.bytes_at(offset, ROM_LEN.checked_sub(offset)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn rom() -> Option<Rom> {
        let home = std::env::var_os("HOME")?;
        let path = Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return None;
        }
        Some(Rom::open(&path).expect("ROM opens"))
    }

    #[test]
    fn oam_sizes() {
        assert_eq!(oam_size(0, 2), Some((32, 32)));
        assert_eq!(oam_size(1, 3), Some((64, 32)));
        assert_eq!(oam_size(2, 0), Some((8, 16)));
        assert_eq!(oam_size(3, 0), None);
        assert_eq!(oam_size(0, 4), None);
    }

    #[test]
    fn frames_decode_with_flips() {
        let mut colours = [[0u8; 4]; 16];
        colours[1] = [255, 0, 0, 255];
        colours[2] = [0, 255, 0, 255];
        // Two 16x8 frames: tiles 0-1 and 2-3.
        let mut tiles = vec![0u8; 4 * TILE_BYTES];
        tiles[0] = 0x21; // (0,0) = 1, (1,0) = 2
        tiles[TILE_BYTES + 4] = 0x01; // tile 1, row 1: (8,1) = 1
        tiles[2 * TILE_BYTES] = 0x02; // second frame (0,0) = 2
        let sheet = Sheet { tiles, colours };
        let frame = |tile, h_flip, v_flip| Frame {
            tile,
            duration: 1,
            h_flip,
            v_flip,
        };
        let px = |f: &[u8], x: usize, y: usize| f[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].to_vec();
        let f = sheet.frame(16, 8, frame(0, false, false)).unwrap();
        assert_eq!(f.len(), 16 * 8 * 4);
        assert_eq!(px(&f, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&f, 1, 0), [0, 255, 0, 255]);
        assert_eq!(px(&f, 8, 1), [255, 0, 0, 255]);
        assert_eq!(px(&f, 2, 0), [0, 0, 0, 0]);
        let flipped = sheet.frame(16, 8, frame(0, true, true)).unwrap();
        assert_eq!(px(&flipped, 15, 7), [255, 0, 0, 255]);
        assert_eq!(px(&flipped, 7, 6), [255, 0, 0, 255]);
        let second = sheet.frame(16, 8, frame(2, false, false)).unwrap();
        assert_eq!(px(&second, 0, 0), [0, 255, 0, 255]);
        // Past the sheet's end.
        assert!(sheet.frame(16, 8, frame(3, false, false)).is_none());
    }

    #[test]
    fn every_move_script_walks_and_its_sprites_have_sheets() {
        let Some(rom) = rom() else { return };
        let mut creations = 0;
        let mut silent = 0;
        let mut tags = HashSet::new();
        for id in 1..MOVE_COUNT {
            let script = rom.move_anim(id).expect("script walks");
            let with_sheet: Vec<Template> = script
                .created
                .iter()
                .filter_map(|c| rom.anim_template(c.template))
                .collect();
            if with_sheet.is_empty() {
                silent += 1;
            }
            for t in &with_sheet {
                assert!(t.width % 8 == 0 && t.height % 8 == 0 && t.width <= 64 && t.height <= 64);
                assert!(!t.frames.is_empty() && t.frames.len() <= MAX_FRAMES);
                tags.insert(t.tag);
            }
            creations += with_sheet.len();
        }
        assert!(creations > 1000, "{creations}");
        assert!(silent > 10 && silent < 100, "{silent}");
        assert!(tags.len() > 100, "{}", tags.len());
        assert!(rom.move_anim(0).is_none());
        assert!(rom.move_anim(MOVE_COUNT).is_none());
    }

    #[test]
    fn status_scripts_and_sheets_decode() {
        let Some(rom) = rom() else { return };
        let mut drawn = 0;
        for index in 0..STATUS_COUNT {
            let script = rom.status_anim(index).expect("script walks");
            assert!(
                script
                    .loaded
                    .iter()
                    .all(|&tag| rom.anim_sheet(tag).is_some())
            );
            for t in script
                .created
                .iter()
                .filter_map(|c| rom.anim_template(c.template))
            {
                let sheet = rom.anim_sheet(t.tag).expect("sheet decodes");
                for &f in &t.frames {
                    let rgba = sheet
                        .frame(t.width, t.height, f)
                        .expect("frame in the sheet");
                    assert_eq!(rgba.len(), usize::from(t.width) * usize::from(t.height) * 4);
                    assert!(rgba.chunks(4).any(|p| p[3] == 255));
                }
                drawn += 1;
            }
        }
        assert!(drawn >= 5, "{drawn}");
        assert!(rom.status_anim(STATUS_COUNT).is_none());
        // The plain hit makes one sprite with a sheet.
        let hit = rom.general_anim(HIT_SCRIPT).expect("script walks");
        assert!(
            hit.created
                .iter()
                .any(|c| rom.anim_template(c.template).is_some())
        );
        assert!(rom.general_anim(GENERAL_COUNT).is_none());
        // Every sheet and palette decodes.
        for i in 0..SHEET_COUNT {
            assert!(rom.anim_sheet(TAG_BASE + i).is_some(), "sheet {i}");
        }
        assert!(rom.anim_sheet(TAG_BASE + SHEET_COUNT).is_none());
        assert!(rom.anim_sheet(5).is_none());
    }
}

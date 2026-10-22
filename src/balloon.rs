//! The Chao status balloon (ParamFukidasi, re/chao-details.md "The status balloon") with five
//! more rows under the game's five: happiness, alignment, needs, mood and its GBA link partner's
//! types (gba_link.rs; names from the player's ROM). Any Chao's: our own
//! from our save, a friend's from the server's live fields (`ChaoLive`, garden.rs).
//!
//! The balloon keeps its own display (0x565200) for the five stats and the name; we draw the
//! frame ourselves, 9 rows tall, and our rows after it, in the game's font and bars.

// Off the game (Linux builds) only the tests use the rows.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

/// Rows we add under the game's five.
pub const EXTRA_ROWS: usize = 5;
/// The game's rows.
const GAME_ROWS: usize = 5;
/// Glyph size of our text (the font's own cell, 22) and how far apart we set the letters: the
/// game spaces them a whole cell apart, too wide for the balloon, so we place each one.
#[cfg(target_arch = "x86")]
const TEXT_SIZE: f32 = 16.0;
pub const ADVANCE: f32 = 10.2;

/// One row's two lines (label, value; either may be empty) and its bar: the game's
/// (`0x536E80`) as (shown, target) segments 0..=10; both blue, shown only red.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub lines: [(String, String); 2],
    pub bar: Option<(i32, i32)>,
}

/// Emotion bytes `rec+0x12C..` shown as the mood (re/chao-details.md): the strongest wins.
const MOODS: [&str; 7] = ["Joy", "Anger", "Sorrow", "Fear", "Pain", "Dizzy", "Relax"];
/// Below this every emotion is quiet: "Calm".
const MOOD_SHOWN: u8 = 30;

fn u16_at(rec: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([rec[at], rec[at + 1]])
}

/// A need word (0..10000) as a percentage.
fn percent(rec: &[u8], at: usize) -> String {
    format!("{}%", u16_at(rec, at).min(10_000) / 100)
}

/// Segments 0..=10 of `v` in `lo..=hi`, rounded.
/// A signed value `v` of `-max..=max` as a bar: how far from 0, blue above and red below.
fn signed_bar(v: f32, max: f32) -> (i32, i32) {
    let k = ((v / max).abs().clamp(0.0, 1.0) * 10.0).round() as i32;
    if v >= 0.0 { (k, k) } else { (k, 0) }
}

/// The rows for a Chao record (at least 0x190 bytes) and its partner's type names.
pub fn rows(rec: &[u8], partner: Option<&[String]>) -> [Row; EXTRA_ROWS] {
    let happiness = i16::from_le_bytes([rec[0x82], rec[0x83]]).clamp(-100, 100);
    let alignment = f32::from_le_bytes([rec[0xB0], rec[0xB1], rec[0xB2], rec[0xB3]]);
    let alignment = if alignment.is_finite() {
        alignment.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let side = if alignment >= 0.1 {
        "Hero"
    } else if alignment <= -0.1 {
        "Dark"
    } else {
        "Neutral"
    };
    let (mood, level) = MOODS
        .iter()
        .zip(&rec[0x12C..0x133])
        .max_by_key(|(_, v)| **v)
        .map_or(("Calm", 0), |(name, v)| (*name, *v));
    let mood = if level < MOOD_SHOWN { "Calm" } else { mood };
    let pair = |a: &str, b: String| (a.to_owned(), b);
    [
        Row {
            lines: [
                pair("Happiness", format!("{happiness:+}")),
                pair("", String::new()),
            ],
            bar: Some(signed_bar(f32::from(happiness), 100.0)),
        },
        Row {
            lines: [
                pair(side, format!("{alignment:+.2}")),
                pair("", String::new()),
            ],
            bar: Some(signed_bar(alignment, 1.0)),
        },
        Row {
            lines: [
                pair("Hunger", percent(rec, 0x138)),
                pair("Sleepy", percent(rec, 0x134)),
            ],
            bar: None,
        },
        Row {
            lines: [
                pair("Lonely", percent(rec, 0x13E)),
                pair("Mood", mood.to_owned()),
            ],
            bar: None,
        },
        partner_row(partner),
    ]
}

/// Longest type name shown (the line is 14 letters wide with "Partner").
const TYPE_NAME_MAX: usize = 6;

/// "Partner" and its type names, one a line; "none" without one.
fn partner_row(types: Option<&[String]>) -> Row {
    let name = |i: usize| {
        types
            .and_then(|t| t.get(i))
            .map(|n| n.chars().take(TYPE_NAME_MAX).collect::<String>())
    };
    Row {
        lines: [
            (
                "Partner".to_owned(),
                name(0).unwrap_or_else(|| "none".to_owned()),
            ),
            (String::new(), name(1).unwrap_or_default()),
        ],
        bar: None,
    }
}

/// Text in the game's font codes (`0x536BE0`): ASCII − 0x20, 0x5F a space, 0 the end.
pub fn font_codes(text: &str) -> Vec<u8> {
    text.bytes()
        .filter(|b| (0x20..0x7F).contains(b))
        .map(|b| if b == b' ' { 0x5F } else { b - 0x20 })
        .chain([0])
        .collect()
}

#[cfg(target_arch = "x86")]
mod game {
    use super::{ADVANCE, EXTRA_ROWS, GAME_ROWS, TEXT_SIZE, font_codes, rows};

    /// The balloon's display (`obj+0x1C`, written by `mov [esi+0x1C], imm32` at 0x565A80).
    const DISPLAY_IMM: usize = 0x0056_5A83;
    const GAME_DISPLAY: usize = 0x0056_5200;
    /// Rows it grows to and shrinks from (`cmp byte [edi+0xC], 5`, `mov byte [edi+0xC], 5`).
    const GROW_CMP: usize = 0x0056_4CBD;
    const GROW_SET: usize = 0x0056_4CC7;
    /// The game's own frame in its display: jumped over (`and eax, 0x80000001` → `jmp 0x5653C4`).
    const FRAME_SKIP: usize = 0x0056_5269;

    const SPRITE: usize = 0x0055_8BE0;
    const SET_COLOUR: usize = 0x0055_9FA0;
    const TEXT: usize = 0x0053_6BE0;
    const BAR: usize = 0x0053_6E80;
    /// Blend mode the sprite drawers read; the colour they draw with.
    const BLEND: usize = 0x01AE_D260;
    const COLOUR: usize = 0x012D_27A4;
    /// Frame sprites (level00): cap, top edge, one row, bottom.
    const CAP: usize = 0x0131_2B38;
    const TOP: usize = 0x0131_2B5C;
    const MIDDLE: usize = 0x0131_2B80;
    const BOTTOM: usize = 0x0131_2BA4;

    /// Balloon data (`obj+0x40`): state (byte 0), rows shown, X, Y, content alpha, the Chao's
    /// record.
    const D_ROWS: usize = 0x0C;
    const D_X: usize = 0x10;
    const D_Y: usize = 0x14;
    const D_ALPHA: usize = 0x20;
    const D_RECORD: usize = 0x3C;
    /// Its Y with our rows: the game's 216 less half of them, so it stays on screen.
    const Y: f32 = 216.0 - 20.0 * EXTRA_ROWS as f32;
    const ROW: f32 = 40.0;
    const Z: f32 = -1.5;
    const RECORD_SIZE: usize = 0x800;

    pub fn install() {
        let rows = (GAME_ROWS + EXTRA_ROWS) as u8;
        let display = (display as extern "C" fn(usize) as usize as u32).to_le_bytes();
        let patched = crate::patch::replace_code(GROW_CMP, &[5], &[rows])
            && crate::patch::replace_code(GROW_SET, &[5], &[rows])
            && crate::patch::replace_code(
                FRAME_SKIP,
                &[0x25, 0x01, 0x00, 0x00, 0x80],
                &[0xE9, 0x56, 0x01, 0x00, 0x00],
            )
            && crate::patch::replace_code(
                DISPLAY_IMM,
                &(GAME_DISPLAY as u32).to_le_bytes(),
                &display,
            );
        if patched {
            crate::log!("balloon: Chao status balloon shows {EXTRA_ROWS} more rows");
        } else {
            crate::log!("balloon: the status balloon's code isn't as expected; left alone");
        }
    }

    /// `0x558BE0`: sprite `desc` at (x, y), aligned (−1 left/top, 0 centre, 1 right/bottom).
    unsafe fn sprite(desc: usize, x: f32, y: f32, align_x: i32, align_y: i32) {
        let args: [u32; 7] = [
            x.to_bits(),
            y.to_bits(),
            Z.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
            align_x as u32,
            align_y as u32,
        ];
        // SAFETY: the game's sprite call (EAX = desc, 7 cdecl arguments the caller pops).
        unsafe {
            std::arch::asm!(
                "push dword ptr [edx + 24]",
                "push dword ptr [edx + 20]",
                "push dword ptr [edx + 16]",
                "push dword ptr [edx + 12]",
                "push dword ptr [edx + 8]",
                "push dword ptr [edx + 4]",
                "push dword ptr [edx]",
                "call ecx",
                "add esp, 28",
                inout("eax") desc => _,
                inout("edx") args.as_ptr() => _,
                inout("ecx") SPRITE => _,
                clobber_abi("C"),
            );
        }
    }

    /// `text` at (x, y), from x (`align` −1) or ending at x (1), colour 0xAABBGGRR: a letter
    /// at a time, `ADVANCE` apart.
    unsafe fn text(text: &str, x: f32, y: f32, align: i32, colour: u32) {
        let codes = font_codes(text);
        let letters = &codes[..codes.len() - 1];
        let start = if align > 0 {
            x - ADVANCE * letters.len() as f32
        } else {
            x
        };
        for (i, &code) in letters.iter().enumerate() {
            // SAFETY: as `letter`'s.
            unsafe { letter(code, start + ADVANCE * i as f32, y, colour) };
        }
    }

    /// `0x536BE0`: one letter (a font code) at (x, y).
    unsafe fn letter(code: u8, x: f32, y: f32, colour: u32) {
        let codes = [code, 0];
        let align = -1;
        let args: [u32; 7] = [
            codes.as_ptr() as u32,
            x.to_bits(),
            y.to_bits(),
            TEXT_SIZE.to_bits(),
            TEXT_SIZE.to_bits(),
            colour,
            1, // up to the end, not 7 characters
        ];
        // SAFETY: the game's text call (ECX = align, 7 cdecl arguments the caller pops);
        // `codes` outlives it.
        unsafe {
            std::arch::asm!(
                "push dword ptr [edx + 24]",
                "push dword ptr [edx + 20]",
                "push dword ptr [edx + 16]",
                "push dword ptr [edx + 12]",
                "push dword ptr [edx + 8]",
                "push dword ptr [edx + 4]",
                "push dword ptr [edx]",
                "call eax",
                "add esp, 28",
                inout("eax") TEXT => _,
                inout("edx") args.as_ptr() => _,
                inout("ecx") align => _,
                clobber_abi("C"),
            );
        }
    }

    /// The balloon's display: our frame, the game's rows, ours.
    extern "C" fn display(object: usize) {
        crate::watch::mark("balloon");
        // SAFETY: the balloon object the game calls its display with, on the game thread.
        unsafe {
            let d = ((object + 0x40) as *const usize).read();
            if d == 0 || (d as *const u8).read() == 0 {
                return;
            }
            ((d + D_Y) as *mut f32).write(Y);
            let n = i32::from(((d + D_ROWS) as *const i8).read()).max(0);
            let x = ((d + D_X) as *const f32).read();
            type SetColour = extern "C" fn(f32, f32, f32, f32);
            let set_colour = std::mem::transmute::<usize, SetColour>(SET_COLOUR);
            // The frame, as the game draws it but 9 rows tall and its top where the game's 5
            // rows have it (Y − 100), so they line up.
            (BLEND as *mut u32).write(0);
            set_colour(0.4, 1.0, 1.0, 1.0);
            let top = Y - 100.0;
            sprite(CAP, x, top - 8.0, 0, 1);
            sprite(TOP, x, top, 0, 1);
            sprite(BOTTOM, x, top + ROW * n as f32, 0, -1);
            for k in 0..n {
                sprite(MIDDLE, x, top + ROW * k as f32, 0, -1);
            }
            // The game's five rows and the name (its frame is jumped over).
            std::mem::transmute::<usize, extern "C" fn(usize)>(GAME_DISPLAY)(object);
            let record = ((d + D_RECORD) as *const usize).read();
            let alpha = ((d + D_ALPHA) as *const f32).read();
            if record != 0 && alpha > 0.01 {
                let rec = std::slice::from_raw_parts(record as *const u8, RECORD_SIZE);
                let a = ((alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24;
                let white = a | 0x00FF_FFFF;
                type Bar = extern "C" fn(i32, i32, f32, f32, f32, f32, f32);
                let bar = std::mem::transmute::<usize, Bar>(BAR);
                let x0 = x - 72.0;
                let partner = crate::chao::ChaoData::from_bytes(rec)
                    .and_then(|c| crate::gba_link::partner_types(c.identity()));
                for (k, row) in rows(rec, partner.as_deref()).iter().enumerate() {
                    let i = GAME_ROWS + k;
                    // Only once the frame has grown to it.
                    if (i as i32) >= n {
                        break;
                    }
                    let y = Y - 96.0 + ROW * i as f32;
                    for (line, (label, value)) in row.lines.iter().enumerate() {
                        let ly = y + 2.0 + 20.0 * line as f32;
                        if !label.is_empty() {
                            text(label, x0, ly, -1, white);
                        }
                        if !value.is_empty() {
                            text(value, x0 + 144.0, ly, 1, white);
                        }
                    }
                    if let Some((shown, target)) = row.bar {
                        bar(shown, target, x0 + 49.0, y + 26.0, 92.0, 8.0, alpha);
                    }
                }
            }
            (COLOUR as *mut u32).write(0xFFFF_FFFF);
        }
    }
}

#[cfg(target_arch = "x86")]
pub use game::install;

#[cfg(not(target_arch = "x86"))]
pub fn install() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Vec<u8> {
        let mut rec = vec![0u8; 0x800];
        rec[0x82..0x84].copy_from_slice(&42i16.to_le_bytes());
        rec[0xB0..0xB4].copy_from_slice(&(-0.5f32).to_le_bytes());
        rec[0x138..0x13A].copy_from_slice(&3000u16.to_le_bytes());
        rec[0x134..0x136].copy_from_slice(&10_000u16.to_le_bytes());
        rec[0x12C + 1] = 120; // anger
        rec
    }

    #[test]
    fn rows_read_the_record() {
        let rows = rows(&record(), None);
        assert_eq!(rows[0].lines[0].1, "+42");
        // Happy is blue, unhappy red; hero blue, dark red: by how far from 0.
        assert_eq!(rows[0].bar, Some((4, 4)));
        assert_eq!(rows[1].lines[0].0, "Dark");
        assert_eq!(rows[1].lines[0].1, "-0.50");
        assert_eq!(rows[1].bar, Some((5, 0)));
        assert_eq!(rows[2].lines[0].1, "30%");
        assert_eq!(rows[2].lines[1].1, "100%");
        assert_eq!(rows[3].lines[1].1, "Anger");
    }

    #[test]
    fn a_quiet_chao_is_calm_and_a_broken_alignment_neutral() {
        let mut rec = vec![0u8; 0x800];
        rec[0xB0..0xB4].copy_from_slice(&f32::NAN.to_le_bytes());
        let rows = rows(&rec, None);
        assert_eq!(rows[3].lines[1].1, "Calm");
        assert_eq!(rows[4].lines[0], ("Partner".into(), "none".into()));
        assert_eq!(rows[1].lines[0].0, "Neutral");
    }

    #[test]
    fn every_line_fits_the_balloon() {
        let mut widest = record();
        widest[0x82..0x84].copy_from_slice(&(-100i16).to_le_bytes());
        widest[0xB0..0xB4].copy_from_slice(&0.05f32.to_le_bytes());
        widest[0x13E..0x140].copy_from_slice(&10_000u16.to_le_bytes());
        widest[0x12C + 2] = 200; // sorrow
        let long = ["ABCDEFGHIJ".to_string(), "KLMNOPQRS".to_string()];
        let partner = rows(&widest, Some(&long));
        assert_eq!(partner[4].lines[1].1, "KLMNOP");
        for row in partner {
            for (label, value) in row.lines {
                let width = (label.len() + value.len() + 1) as f32 * ADVANCE;
                assert!(width <= 144.0, "{label} {value}");
            }
        }
        // 144 px wide, label and value side by side with a letter between.
        for row in rows(&record(), Some(&["AB".to_string()])) {
            for (label, value) in row.lines {
                let width = (label.len() + value.len() + 1) as f32 * ADVANCE;
                assert!(width <= 144.0, "{label} {value}");
            }
        }
    }

    #[test]
    fn text_goes_in_the_games_font_codes() {
        assert_eq!(font_codes("A B"), vec![0x21, 0x5F, 0x22, 0]);
        assert_eq!(font_codes("+4%"), vec![0x0B, 0x14, 0x05, 0]);
    }
}

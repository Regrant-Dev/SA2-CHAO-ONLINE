//! The Chao status balloon (ParamFukidasi, re/chao-details.md "The status balloon") with all
//! seven abilities and four more rows. Under the game's five ability rows come Luck and
//! Intelligence, drawn the way the game draws its own (word, "Lv." and level, points, points
//! gauge, progress bar), and every ability row gets its grade letter in a column of its own.
//! Then: happiness and alignment; a child's evolution and what it would become, or an adult's
//! form and time left; hunger and mood; its creature link partner's types (creature_link.rs; names from
//! the player's catalog) over its battle level and the XP toward the next (battle/levels.rs). Any
//! Chao's: our own from our save, a friend's from the server's live fields (`ChaoLive`,
//! garden.rs) and what their game sent (`Body::Partners`).
//!
//! The balloon keeps its own display (0x565200) for the five stats and the name; we draw the
//! frame ourselves, 11 rows tall and a grade column wider, and our rows after it, in the game's
//! font, digits and bars.

// Off the game (Linux builds) only the tests use the rows.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use crate::chao_stats::{self as stats, Ability, LifeLeft, Side};

/// Ability rows: the game's five and Luck and Intelligence.
pub const STAT_ROWS: usize = 7;
/// Rows of ours under them.
pub const EXTRA_ROWS: usize = 4;
/// The game's rows.
const GAME_ROWS: usize = 5;
/// Glyph size of our text (the font's own cell, 22) and how far apart we set the letters: the
/// game spaces them a whole cell apart, too wide for the balloon, so we place each one.
const TEXT_SIZE: f32 = 16.0;
pub const ADVANCE: f32 = 10.2;
/// A row's text runs this wide (from the left of the ability words to the right of the points).
pub const LINE_WIDTH: f32 = 144.0;

/// Colours in the game font's order, 0x00BBGGRR (the alpha is added when drawn).
pub const WHITE: u32 = 0x00FF_FFFF;
const BLUE: u32 = 0x00FF_C060;
const RED: u32 = 0x0060_60FF;
const HERO: u32 = 0x00FF_B070;
const DARK: u32 = 0x00E0_60C0;

/// Text and its colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub colour: u32,
}

fn white(text: impl Into<String>) -> Text {
    Text {
        text: text.into(),
        colour: WHITE,
    }
}

/// One of our text rows: two lines of a label and a value (either may be empty).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub lines: [(Text, Text); 2],
}

/// 0xAARRGGBB as the game font's 0x00BBGGRR.
fn bgr(argb: u32) -> u32 {
    (argb & 0x00_FF00) | ((argb >> 16) & 0xFF) | ((argb & 0xFF) << 16)
}

/// Each ability row's grade letter and its colour ('?' past X; `None` while the record's
/// fields aren't set, as in an egg's).
pub fn grades(rec: &[u8]) -> [Option<(char, u32)>; STAT_ROWS] {
    std::array::from_fn(|i| {
        Ability::read(rec, i).map(|a| (a.letter(), bgr(stats::grade_colour(a.grade))))
    })
}

/// Segments 0..=10 of the game's progress bar for an ability (`(p + 5) / 10`, as 0x564C80
/// shows them).
pub fn segments(a: Ability) -> i32 {
    ((i32::from(a.progress) + 5) / 10).clamp(0, 10)
}

/// "5.2h" or "25m" of garden time.
fn short_time(ticks: u32) -> String {
    let h = stats::hours(ticks);
    if h >= 1.0 {
        format!("{h:.1}h")
    } else {
        format!("{}m", (h * 60.0).round() as u32)
    }
}

/// The three text rows for a Chao record (`CHAO_SIZE` bytes).
pub fn rows(rec: &[u8]) -> [Row; 3] {
    let happiness = stats::happiness(rec).clamp(-100, 100);
    let alignment = stats::alignment(rec).unwrap_or(0.0).clamp(-1.0, 1.0);
    let side = Side::of(alignment);
    let pair = |label: Text, value: Text| (label, value);
    let growth = if rec[0x80] == crate::chao_sim::record::TYPE_CHILD {
        let evolution = stats::evolution(rec).map_or_else(
            || "-".into(),
            |f| format!("{}%", (f * 100.0).floor() as u32),
        );
        let becomes = stats::evolves_into(rec)
            .filter(|k| (5..=22).contains(k))
            .map_or("-", |k| {
                ["Normal", "Swim", "Fly", "Run", "Power", "Chaos"][usize::from(k - 5) / 3]
            });
        [
            pair(white("Evolve"), white(evolution)),
            pair(white("Will be"), white(becomes)),
        ]
    } else {
        let life = match stats::life_left(rec) {
            LifeLeft::NotYet => white("-"),
            LifeLeft::Ageless => white("ageless"),
            LifeLeft::Ticks(t) => Text {
                text: short_time(t),
                // Red when it wouldn't be reborn.
                colour: if stats::rebirth_blockers(rec).is_empty() {
                    WHITE
                } else {
                    RED
                },
            },
        };
        [
            pair(
                white(
                    stats::short_form(rec[0x80])
                        .chars()
                        .take(TITLE_MAX)
                        .collect::<String>(),
                ),
                white(""),
            ),
            pair(white("Life"), life),
        ]
    };
    [
        Row {
            lines: [
                pair(
                    white("Happy"),
                    Text {
                        text: format!("{happiness:+}"),
                        colour: if happiness >= 0 { BLUE } else { RED },
                    },
                ),
                pair(
                    Text {
                        text: side.name().into(),
                        colour: match side {
                            Side::Hero => HERO,
                            Side::Dark => DARK,
                            Side::Neutral => WHITE,
                        },
                    },
                    white(format!("{alignment:+.2}")),
                ),
            ],
        },
        Row { lines: growth },
        Row {
            lines: [
                pair(
                    white("Hunger"),
                    white(format!("{}%", stats::need_percent(rec, stats::HUNGER))),
                ),
                pair(white("Mood"), white(stats::mood(rec))),
            ],
        },
    ]
}

/// The battle row: its partner's types (or "Battle" without one) over its level, and the
/// segments 0..=10 of its XP toward the next level when that's known (our own Chao) and there
/// is a next.
#[derive(Clone, Debug, PartialEq)]
pub struct BattleRow {
    pub title: String,
    pub level: u8,
    pub segments: Option<i32>,
}

/// Longest title (a line of `ADVANCE` letters in `LINE_WIDTH`).
const TITLE_MAX: usize = 14;
/// Each type name is cut to this when two don't fit together.
const TYPE_NAME_MAX: usize = 6;

/// The battle row for its partner's type names and its battle level with the part of the way
/// to the next (0..=1; `None` when it isn't known: another player's Chao).
pub fn battle_row(partner: Option<&[String]>, (level, toward): (u8, Option<f32>)) -> BattleRow {
    let names: Vec<&String> = partner.unwrap_or_default().iter().take(2).collect();
    let mut title = names
        .iter()
        .map(|n| n.as_str())
        .collect::<Vec<_>>()
        .join("/");
    if title.chars().count() > TITLE_MAX {
        title = names
            .iter()
            .map(|n| n.chars().take(TYPE_NAME_MAX).collect::<String>())
            .collect::<Vec<_>>()
            .join("/");
    }
    if title.is_empty() {
        title = "Battle".into();
    }
    BattleRow {
        title,
        level,
        segments: toward
            .filter(|_| level < crate::battle::levels::MAX_LEVEL)
            .map(|t| (t.clamp(0.0, 1.0) * 10.0).floor() as i32),
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
    use super::{
        ADVANCE, EXTRA_ROWS, GAME_ROWS, LINE_WIDTH, STAT_ROWS, TEXT_SIZE, battle_row, font_codes,
        grades, rows,
    };
    use crate::chao_stats::Ability;
    use std::sync::Mutex;

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
    /// Four white digits of a value (EAX) and a gauge of it (EAX), as the ability rows' points.
    const POINTS: usize = 0x0053_69F0;
    const GAUGE: usize = 0x0053_6770;
    /// Blend mode the sprite drawers read; the colour they draw with.
    const BLEND: usize = 0x01AE_D260;
    const COLOUR: usize = 0x012D_27A4;
    /// Frame sprites (level00): cap, top edge, one row, bottom; 165 wide.
    const CAP: usize = 0x0131_2B38;
    const TOP: usize = 0x0131_2B5C;
    const MIDDLE: usize = 0x0131_2B80;
    const BOTTOM: usize = 0x0131_2BA4;
    const FRAME_WIDTH: f32 = 165.0;
    /// "Lv." and the digits 0..9 (level00, 0x24 apart).
    const LV: usize = 0x0131_2A3C;
    const DIGITS: usize = 0x0131_28B0;
    const DIGIT_STRIDE: usize = 0x24;

    /// Balloon data (`obj+0x40`): state (byte 0), rows shown, X, Y, content alpha, the Chao's
    /// record.
    const D_ROWS: usize = 0x0C;
    const D_X: usize = 0x10;
    const D_Y: usize = 0x14;
    const D_ALPHA: usize = 0x20;
    const D_RECORD: usize = 0x3C;
    /// Its Y with our rows: higher than the game's 216 so all 11 rows stay on screen. The name
    /// is drawn at Y − 124 (8 to 29 of 480), the cap from Y − 132 (0) and the frame's bottom
    /// edge ends at Y − 100 + 40 * 10 + 36 + 12 (480).
    const Y: f32 = 132.0;
    const ROW: f32 = 40.0;
    /// The last row (the battle row) is this tall, so the frame fits the 480 lines; its bar
    /// ends 2 px into the frame's bottom edge.
    const LAST_ROW: f32 = 36.0;
    const Z: f32 = -1.5;
    const RECORD_SIZE: usize = 0x800;
    /// The grade column: the frame grows this much to the left, and the letters' place and size.
    const WIDEN: f32 = 24.0;
    const GRADE_X: f32 = -22.0;
    const GRADE_Y: f32 = 7.0;
    const GRADE_SIZE: f32 = 18.0;
    /// Luck and Intelligence's words, about as tall as the game's word sprites.
    const WORD_SIZE: f32 = 18.0;
    const WORDS: [&str; 2] = ["Luck", "Intel"];

    pub fn install() {
        let rows = (STAT_ROWS + EXTRA_ROWS) as u8;
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
            crate::log!(
                "balloon: Chao status balloon shows all 7 abilities and {EXTRA_ROWS} more rows"
            );
        } else {
            crate::log!("balloon: the status balloon's code isn't as expected; left alone");
        }
    }

    /// `0x558BE0`: sprite `desc` at (x, y), `scale_x` as wide, aligned (−1 left/top, 0 centre,
    /// 1 right/bottom).
    unsafe fn sprite(desc: usize, x: f32, y: f32, scale_x: f32, align_x: i32, align_y: i32) {
        // SAFETY: as `sprite_scaled`'s.
        unsafe { sprite_scaled(desc, x, y, (scale_x, 1.0), align_x, align_y) };
    }

    /// `sprite`, also `scale.1` as tall.
    unsafe fn sprite_scaled(
        desc: usize,
        x: f32,
        y: f32,
        scale: (f32, f32),
        align_x: i32,
        align_y: i32,
    ) {
        let args: [u32; 7] = [
            x.to_bits(),
            y.to_bits(),
            Z.to_bits(),
            scale.0.to_bits(),
            scale.1.to_bits(),
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

    /// `0x5369F0`: `value`'s four digits from (x, y).
    unsafe fn points(value: u32, x: f32, y: f32, alpha: f32) {
        let args: [u32; 3] = [x.to_bits(), y.to_bits(), alpha.to_bits()];
        // SAFETY: the game's digits call (EAX = value, 3 cdecl arguments the caller pops).
        unsafe {
            std::arch::asm!(
                "push dword ptr [edx + 8]",
                "push dword ptr [edx + 4]",
                "push dword ptr [edx]",
                "call ecx",
                "add esp, 12",
                inout("eax") value => _,
                inout("edx") args.as_ptr() => _,
                inout("ecx") POINTS => _,
                clobber_abi("C"),
            );
        }
    }

    /// `0x536770`: the points gauge of `value` in w × h at (x, y).
    unsafe fn gauge(value: u32, x: f32, y: f32, w: f32, h: f32, alpha: f32) {
        let args: [u32; 5] = [
            x.to_bits(),
            y.to_bits(),
            w.to_bits(),
            h.to_bits(),
            alpha.to_bits(),
        ];
        // SAFETY: the game's gauge call (EAX = value, 5 cdecl arguments the caller pops).
        unsafe {
            std::arch::asm!(
                "push dword ptr [edx + 16]",
                "push dword ptr [edx + 12]",
                "push dword ptr [edx + 8]",
                "push dword ptr [edx + 4]",
                "push dword ptr [edx]",
                "call ecx",
                "add esp, 20",
                inout("eax") value => _,
                inout("edx") args.as_ptr() => _,
                inout("ecx") GAUGE => _,
                clobber_abi("C"),
            );
        }
    }

    /// `text` at (x, y), from x (`align` −1) or ending at x (1), colour 0xAABBGGRR: a letter
    /// at a time, `advance` apart, `size` tall.
    unsafe fn text_sized(text: &str, x: f32, y: f32, align: i32, colour: u32, size: f32) {
        let advance = ADVANCE * size / TEXT_SIZE;
        let codes = font_codes(text);
        let letters = &codes[..codes.len() - 1];
        let start = if align > 0 {
            x - advance * letters.len() as f32
        } else {
            x
        };
        for (i, &code) in letters.iter().enumerate() {
            // SAFETY: as `letter`'s.
            unsafe { letter(code, start + advance * i as f32, y, colour, size) };
        }
    }

    unsafe fn text(text: &str, x: f32, y: f32, align: i32, colour: u32) {
        // SAFETY: as `text_sized`'s.
        unsafe { text_sized(text, x, y, align, colour, TEXT_SIZE) };
    }

    /// `0x536BE0`: one letter (a font code) at (x, y).
    unsafe fn letter(code: u8, x: f32, y: f32, colour: u32, size: f32) {
        let codes = [code, 0];
        let align = -1;
        let args: [u32; 7] = [
            codes.as_ptr() as u32,
            x.to_bits(),
            y.to_bits(),
            size.to_bits(),
            size.to_bits(),
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

    /// The game's "Lv." and `level`'s digits (two at least, as its rows show them) under a row
    /// at (x0, y), in yellow.
    unsafe fn level_digits(level: u8, x0: f32, y: f32, alpha: f32, set_colour: SetColour) {
        set_colour(alpha, 1.0, 1.0, 0.0);
        let digits = format!("{level:02}");
        // SAFETY: the game's sprite call with level00's static descs.
        unsafe {
            sprite(LV, x0, y + 20.0, 1.0, -1, -1);
            for (i, d) in digits.bytes().enumerate() {
                let desc = DIGITS + DIGIT_STRIDE * usize::from(d - b'0');
                sprite(desc, x0 + 24.0 + 12.0 * i as f32, y + 20.0, 1.0, -1, -1);
            }
        }
    }

    type SetColour = extern "C" fn(f32, f32, f32, f32);
    type Bar = extern "C" fn(i32, i32, f32, f32, f32, f32, f32);

    /// An ability row as the game draws its five (0x565200's row loop): word, "Lv." and level,
    /// points, points gauge, progress segments. Read straight from the record: no count-up.
    unsafe fn ability_row(
        word: &str,
        a: Ability,
        x0: f32,
        y: f32,
        alpha: f32,
        set_colour: SetColour,
        bar: Bar,
    ) {
        let a_bits = ((alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24;
        let value = u32::from(a.points.min(9999));
        // SAFETY: the game's own draw calls, as its display makes them.
        unsafe {
            text_sized(word, x0, y + 1.0, -1, a_bits | super::WHITE, WORD_SIZE);
            level_digits(a.level, x0, y, alpha, set_colour);
            points(value, x0 + 86.0, y + 5.0, alpha);
            gauge(value, x0 + 49.0, y + 20.0, 92.0, 8.0, alpha);
        }
        let segments = super::segments(a);
        bar(segments, segments, x0 + 49.0, y + 26.0, 92.0, 8.0, alpha);
    }

    /// The battle level row of the Chao (`rows`).
    type Battle = (u8, Option<f32>);

    /// The last Chao's battle level for the balloon (identity, frames it's kept, the row):
    /// looked up again for another Chao or after a second.
    static LEVEL: Mutex<Option<(u64, u16, Battle)>> = Mutex::new(None);

    /// The battle level of the Chao with `identity` whose record is at `record`, and how far it
    /// is toward the next when that's known (ours: its record is one of our save's slots).
    fn battle_level(identity: u64, record: usize) -> Battle {
        let mut last = LEVEL.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((id, frames, row)) = last.as_mut()
            && *id == identity
            && *frames > 0
        {
            *frames -= 1;
            return *row;
        }
        let own = (0..crate::game::GARDEN_SLOTS)
            .any(|slot| crate::game::chao_slot_address(slot) == record);
        let (level, known) = crate::creature_link::battle_level(identity, own);
        let row = (level.level, known.then(|| level.fraction()));
        *last = Some((identity, 60, row));
        row
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
            let set_colour = std::mem::transmute::<usize, SetColour>(SET_COLOUR);
            // The frame, as the game draws it but as tall as all the rows, a grade column
            // wider on the left, and its top where the game's 5 rows have it (Y − 100), so
            // they line up.
            (BLEND as *mut u32).write(0);
            set_colour(0.4, 1.0, 1.0, 1.0);
            let top = Y - 100.0;
            let (fx, wide) = (x - WIDEN / 2.0, (FRAME_WIDTH + WIDEN) / FRAME_WIDTH);
            let last = (STAT_ROWS + EXTRA_ROWS) as i32 - 1;
            let height = |k: i32| if k == last { LAST_ROW } else { ROW };
            sprite(CAP, fx, top - 8.0, wide, 0, 1);
            sprite(TOP, fx, top, wide, 0, 1);
            let mut bottom = top;
            for k in 0..n {
                sprite_scaled(MIDDLE, fx, bottom, (wide, height(k) / ROW), 0, -1);
                bottom += height(k);
            }
            sprite(BOTTOM, fx, bottom, wide, 0, -1);
            // The game's five rows and the name (its frame is jumped over).
            std::mem::transmute::<usize, extern "C" fn(usize)>(GAME_DISPLAY)(object);
            let record = ((d + D_RECORD) as *const usize).read();
            let alpha = ((d + D_ALPHA) as *const f32).read();
            if record != 0 && alpha > 0.01 {
                let rec = std::slice::from_raw_parts(record as *const u8, RECORD_SIZE);
                let a = ((alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24;
                let white = a | super::WHITE;
                let bar = std::mem::transmute::<usize, Bar>(BAR);
                let x0 = x - 72.0;
                let row_y = |i: usize| Y - 96.0 + ROW * i as f32;
                // Only rows the frame has grown to.
                let shown = |i: usize| (i as i32) < n;
                for (i, grade) in grades(rec).iter().enumerate() {
                    if let Some((letter, colour)) = grade
                        && shown(i)
                    {
                        text_sized(
                            &letter.to_string(),
                            x0 + GRADE_X,
                            row_y(i) + GRADE_Y,
                            -1,
                            a | colour,
                            GRADE_SIZE,
                        );
                    }
                }
                for (k, word) in WORDS.iter().enumerate() {
                    let i = GAME_ROWS + k;
                    if let Some(ability) = Ability::read(rec, i)
                        && shown(i)
                    {
                        ability_row(word, ability, x0, row_y(i), alpha, set_colour, bar);
                    }
                }
                let text_rows = rows(rec);
                for (k, row) in text_rows.iter().enumerate() {
                    let i = STAT_ROWS + k;
                    if !shown(i) {
                        break;
                    }
                    for (line, (label, value)) in row.lines.iter().enumerate() {
                        let ly = row_y(i) + 2.0 + 20.0 * line as f32;
                        if !label.text.is_empty() {
                            text(&label.text, x0, ly, -1, a | label.colour);
                        }
                        if !value.text.is_empty() {
                            text(&value.text, x0 + LINE_WIDTH, ly, 1, a | value.colour);
                        }
                    }
                }
                let i = STAT_ROWS + text_rows.len();
                if shown(i) {
                    let identity = crate::chao::ChaoData::from_bytes(rec).map(|c| c.identity());
                    let partner = identity.and_then(crate::creature_link::partner_types);
                    let level = identity
                        .map_or((crate::battle::levels::START_LEVEL, Some(0.0)), |id| {
                            battle_level(id, record)
                        });
                    let b = battle_row(partner.as_deref(), level);
                    let y = row_y(i);
                    text(&b.title, x0, y + 2.0, -1, white);
                    level_digits(b.level, x0, y, alpha, set_colour);
                    if let Some(s) = b.segments {
                        bar(s, s, x0 + 49.0, y + 26.0, 92.0, 8.0, alpha);
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
        let mut rec = vec![0u8; crate::chao::CHAO_SIZE];
        rec[0x80] = 2;
        rec[0x82..0x84].copy_from_slice(&42i16.to_le_bytes());
        rec[0xB0..0xB4].copy_from_slice(&(-0.5f32).to_le_bytes());
        rec[0xA8..0xAC].copy_from_slice(&0.8f32.to_le_bytes());
        rec[0xC0..0xC4].copy_from_slice(&0.74f32.to_le_bytes());
        rec[0x138..0x13A].copy_from_slice(&3000u16.to_le_bytes());
        rec[0x12C + 1] = 120; // anger
        rec
    }

    /// How wide a line's label and value are side by side, a letter apart.
    fn width((label, value): &(Text, Text)) -> f32 {
        let gap = usize::from(!label.text.is_empty() && !value.text.is_empty());
        (label.text.len() + value.text.len() + gap) as f32 * ADVANCE
    }

    #[test]
    fn rows_read_the_record() {
        let rows = rows(&record());
        assert_eq!(rows[0].lines[0].1.text, "+42");
        assert_eq!(rows[0].lines[0].1.colour, BLUE);
        // Dark from −0.5, as evolution counts it.
        assert_eq!(rows[0].lines[1].0.text, "Dark");
        assert_eq!(rows[0].lines[1].0.colour, DARK);
        assert_eq!(rows[0].lines[1].1.text, "-0.50");
        // A child: how far to its cocoon and what it would become.
        assert_eq!(rows[1].lines[0].1.text, "74%");
        assert_eq!(rows[1].lines[1].1.text, "Power");
        assert_eq!(rows[2].lines[0].1.text, "30%");
        assert_eq!(rows[2].lines[1].1.text, "Angry");
    }

    #[test]
    fn an_adult_shows_its_form_and_time_left() {
        let mut rec = record();
        rec[0x80] = 6 + 3 * 4; // Hero Power
        rec[0x8A..0x8C].copy_from_slice(&3000u16.to_le_bytes());
        rec[0x8C..0x8E].copy_from_slice(&3000u16.to_le_bytes());
        rec[0x82..0x84].copy_from_slice(&60i16.to_le_bytes());
        let rows = rows(&rec);
        assert_eq!(rows[1].lines[0].0.text, "Hero Power");
        assert_eq!(rows[1].lines[1].0.text, "Life");
        assert_eq!(rows[1].lines[1].1.text, "11.9h");
        assert_eq!(rows[1].lines[1].1.colour, WHITE);
        // Unhappy: it wouldn't be reborn.
        rec[0x82..0x84].copy_from_slice(&(-5i16).to_le_bytes());
        let rows = super::rows(&rec);
        assert_eq!(rows[1].lines[1].1.colour, RED);
        assert_eq!(rows[0].lines[0].1.colour, RED);
        rec[0x80] = 21;
        assert_eq!(super::rows(&rec)[1].lines[1].1.text, "ageless");
    }

    #[test]
    fn every_ability_row_has_its_grade() {
        let mut rec = record();
        for i in 0..7 {
            rec[0x28 + i] = i as u8;
        }
        let grades = grades(&rec);
        let letters: String = grades.iter().map(|g| g.unwrap().0).collect();
        assert_eq!(letters, "EDCBASX");
        // In the game font's colour order: S (0xFFA030) is orange.
        assert_eq!(grades[5].unwrap().1, 0x0030_A0FF);
        // A grade past X (Intelligence from leftover genes) still shows, as '?'.
        rec[0x28 + 6] = 0x48;
        assert_eq!(super::grades(&rec)[6], Some(('?', 0x00C8_C8C8)));
        assert!(Ability::read(&rec, 6).is_some());
        rec[0x80] = crate::chao::TYPE_EGG;
        assert_eq!(super::grades(&rec)[6], None);
        rec[0x80] = 2;
        rec[0x20] = 44;
        rec[0x21] = 99;
        assert_eq!(segments(Ability::read(&rec, 0).unwrap()), 4);
        assert_eq!(segments(Ability::read(&rec, 1).unwrap()), 10);
    }

    #[test]
    fn the_battle_row_shows_types_level_and_xp() {
        let b = battle_row(None, (23, Some(0.46)));
        assert_eq!(
            b,
            BattleRow {
                title: "Battle".into(),
                level: 23,
                segments: Some(4),
            }
        );
        assert_eq!(battle_row(None, (100, Some(1.0))).segments, None);
        assert_eq!(battle_row(None, (5, Some(0.0))).segments, Some(0));
        // Another player's Chao: the level only.
        assert_eq!(battle_row(None, (40, None)).segments, None);
        let two = ["Water".to_string(), "Rock".to_string()];
        assert_eq!(battle_row(Some(&two), (5, None)).title, "Water/Rock");
        let long = ["ABCDEFGHIJ".to_string(), "KLMNOPQRS".to_string()];
        assert_eq!(battle_row(Some(&long), (5, None)).title, "ABCDEF/KLMNOP");
    }

    #[test]
    fn a_quiet_chao_is_calm_and_a_broken_alignment_neutral() {
        let mut rec = vec![0u8; crate::chao::CHAO_SIZE];
        rec[0x80] = 2;
        rec[0xB0..0xB4].copy_from_slice(&f32::NAN.to_le_bytes());
        let rows = rows(&rec);
        assert_eq!(rows[2].lines[1].1.text, "Calm");
        assert_eq!(rows[0].lines[1].0.text, "Neutral");
        assert_eq!(rows[1].lines[1].1.text, "-");
    }

    #[test]
    fn every_line_fits_the_balloon() {
        let mut widest = record();
        widest[0x82..0x84].copy_from_slice(&(-100i16).to_le_bytes());
        widest[0xB0..0xB4].copy_from_slice(&(-1.0f32).to_le_bytes());
        widest[0x138..0x13A].copy_from_slice(&10_000u16.to_le_bytes());
        widest[0x12C] = 200;
        for kind in [2, 3, 4, 5, 6, 7, 20, 22, 23, 24, 200] {
            widest[0x80] = kind;
            for row in rows(&widest) {
                for line in &row.lines {
                    assert!(width(line) <= LINE_WIDTH, "{line:?}");
                }
            }
            // An adult's form shows whole.
            if kind > 2 {
                let title = &rows(&widest)[1].lines[0].0.text;
                assert_eq!(*title, crate::chao_stats::short_form(kind));
            }
        }
        for partner in [
            vec!["ABCDEFGHIJKLMN".to_string(), "OPQRSTUVWXYZ".to_string()],
            vec!["ABCDEFGHIJKLMNOP".to_string()],
        ] {
            let title = battle_row(Some(&partner), (100, None)).title;
            assert!(title.len() as f32 * ADVANCE <= LINE_WIDTH, "{title}");
        }
    }

    #[test]
    fn text_goes_in_the_games_font_codes() {
        assert_eq!(font_codes("A B"), vec![0x21, 0x5F, 0x22, 0]);
        assert_eq!(font_codes("+4%"), vec![0x0B, 0x14, 0x05, 0]);
    }
}

//! Read-only views of an owned Chao record. Field meanings/evidence: re/chao-details.md.
//! All formatting is pure: viewing a record never calls native AI or changes a save.

use crate::chao::{ChaoData, TYPE_EGG};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    Abilities,
    Emotions,
    Needs,
    Personality,
    Bonds,
    Appearance,
    Health,
    Raw,
}

impl Page {
    pub const ALL: [Self; 9] = [
        Self::Overview,
        Self::Abilities,
        Self::Emotions,
        Self::Needs,
        Self::Personality,
        Self::Bonds,
        Self::Appearance,
        Self::Health,
        Self::Raw,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview & evolution",
            Self::Abilities => "Abilities",
            Self::Emotions => "Emotions",
            Self::Needs => "Needs & condition",
            Self::Personality => "Temperament & personality",
            Self::Bonds => "Character bonds",
            Self::Appearance => "Appearance & animal parts",
            Self::Health => "Health counters",
            Self::Raw => "Raw record",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&p| p == self).unwrap_or(0)
    }
    pub fn next(self, backwards: bool) -> Self {
        Self::ALL
            [(self.index() + if backwards { Self::ALL.len() - 1 } else { 1 }) % Self::ALL.len()]
    }
}

#[derive(Default)]
pub struct View {
    pub slot: Option<usize>,
    pub page: Page,
    /// Chao selector, page selector, Back.
    pub row: usize,
    pub scroll: usize,
}

impl View {
    pub fn select(&mut self, slots: &[usize], backwards: bool) {
        if slots.is_empty() {
            self.slot = None;
            return;
        }
        let current = self
            .slot
            .and_then(|slot| slots.iter().position(|&s| s == slot))
            .unwrap_or(0);
        self.slot =
            Some(slots[(current + if backwards { slots.len() - 1 } else { 1 }) % slots.len()]);
        self.scroll = 0;
    }

    pub fn retain_slot(&mut self, slots: &[usize]) {
        if !self.slot.is_some_and(|slot| slots.contains(&slot)) {
            self.slot = slots.first().copied();
            self.scroll = 0;
        }
    }
}

pub struct Line {
    pub text: String,
    pub gauge: Option<f32>,
}

pub fn layout(width: f32, height: f32) -> (f32, usize) {
    let font = (height / 40.0).min(width / 66.0).floor().max(4.0);
    let visible = ((height / font) as usize).saturating_sub(12).clamp(4, 24);
    (font, visible)
}

fn text(text: impl Into<String>) -> Line {
    Line {
        text: text.into(),
        gauge: None,
    }
}
fn value(label: &str, value: impl std::fmt::Display) -> Line {
    text(format!("{label:<24} {value}"))
}
fn meter(label: &str, number: i32, min: i32, max: i32) -> Line {
    Line {
        text: format!("{label:<24} {number:>6}  [{min}..{max}]"),
        gauge: Some((number - min) as f32 / (max - min) as f32),
    }
}
fn u16_at(c: &ChaoData, offset: usize) -> u16 {
    u16::from_le_bytes(c.bytes[offset..offset + 2].try_into().unwrap())
}
fn float(c: &ChaoData, label: &str, offset: usize, min: f32, max: f32) -> Line {
    let number = f32::from_le_bytes(c.bytes[offset..offset + 4].try_into().unwrap());
    if number.is_finite() {
        Line {
            text: format!("{label:<24} {number:>+7.3}  [{min}..{max}]"),
            gauge: Some((number - min) / (max - min)),
        }
    } else {
        value(label, "not initialized")
    }
}

pub fn form(kind: u8) -> String {
    match kind {
        0 => "Empty".into(),
        1 => "Egg".into(),
        2 => "Child".into(),
        3 => "Good (legacy form)".into(),
        4 => "Bad (legacy form)".into(),
        5..=22 => {
            let n = usize::from(kind - 5);
            format!(
                "{} {}",
                ["Neutral", "Hero", "Dark"][n % 3],
                ["Normal", "Swim", "Fly", "Run", "Power", "Chaos"][n / 3]
            )
        }
        23 => "Tails".into(),
        24 => "Knuckles".into(),
        25 => "Amy".into(),
        _ => format!("Unknown ({kind})"),
    }
}

pub fn lines(c: &ChaoData, page: Page) -> Vec<Line> {
    if c.chao_type() == TYPE_EGG && !matches!(page, Page::Overview | Page::Raw) {
        return vec![
            text("This Chao is still an egg."),
            text("Its abilities and traits are not initialized yet."),
            text("They become available after hatching."),
        ];
    }
    match page {
        Page::Overview => {
            let mut out = vec![
                value("Form", form(c.chao_type())),
                value(
                    "Garden",
                    match c.garden() {
                        1 => "Neutral",
                        2 => "Hero",
                        3 => "Dark",
                        _ => "Unknown",
                    },
                ),
            ];
            if c.chao_type() == TYPE_EGG {
                out.push(text(
                    "Egg: development values become available after hatching.",
                ));
                return out;
            }
            out.extend([
                meter("Happiness", i32::from(c.happiness()), -100, 100),
                float(c, "Alignment", 0xB0, -1.0, 1.0),
                text("  Alignment: -1 Dark / 0 Neutral / +1 Hero"),
                float(c, "Run / Power tendency", 0xA8, -1.0, 1.0),
                text("  Negative = Run; positive = Power"),
                float(c, "Swim / Fly tendency", 0xAC, -1.0, 1.0),
                text("  Negative = Swim; positive = Fly"),
                float(c, "Evolution progress", 0xC0, 0.0, 1.2),
                value("Age counter", u16_at(c, 0x86)),
                value("Adult age counter", u16_at(c, 0x88)),
                value("Remaining lifespan", u16_at(c, 0x8A)),
                value("Maximum lifespan", u16_at(c, 0x8C)),
                value("Reincarnations", u16_at(c, 0x8E)),
                text("Life and age counters are game units, not real-world minutes."),
            ]);
            out
        }
        Page::Abilities => {
            let mut out = vec![text("Ability        Grade  Level  Points  Level progress")];
            for (index, label) in [
                "Swim",
                "Fly",
                "Run",
                "Power",
                "Stamina",
                "Luck",
                "Intelligence",
            ]
            .iter()
            .enumerate()
            {
                let (grade, level, points, progress) = (
                    c.grade(index),
                    c.level(index),
                    c.points(index),
                    c.bytes[0x20 + index],
                );
                if grade > 6 || level > 99 || !(0..=9999).contains(&points) || progress > 99 {
                    out.push(text(format!(
                        "{label:<14} not initialized (g{grade}, l{level}, p{points})"
                    )));
                } else {
                    out.push(text(format!(
                        "{label:<14} {}      {level:>2}    {points:>4}    {progress:>2}/100",
                        ["E", "D", "C", "B", "A", "S", "X"][usize::from(grade)]
                    )));
                }
            }
            out.extend([
                text("Luck and intelligence are hidden abilities."),
                text("Level 99 is capped; progress is the stored counter."),
                text("Uninitialized hidden fields can contain egg filler."),
            ]);
            out
        }
        Page::Emotions => {
            let mut out = vec![text("Moment-to-moment emotions (separate from happiness)")];
            for (i, label) in [
                "Joy",
                "Anger",
                "Sorrow / urge to cry",
                "Fear",
                "Pain / surprise",
                "Dizziness",
                "Relaxation",
                "Emotional total",
            ]
            .iter()
            .enumerate()
            {
                out.push(meter(label, i32::from(c.bytes[0x12C + i]), 0, 200));
            }
            out
        }
        Page::Needs => {
            let mut out = vec![text(
                "Stored need/condition counters; each has its own meaning.",
            )];
            for (i, label) in [
                "Sleepiness",
                "Sleep depth",
                "Hunger",
                "Mating urge",
                "Boredom",
                "Loneliness",
                "Tiredness",
                "Stress",
                "Nutrition",
                "Condition",
                "Thirst (raw field)",
            ]
            .iter()
            .enumerate()
            {
                out.push(meter(label, i32::from(u16_at(c, 0x134 + i * 2)), 0, 10000));
            }
            out.push(text(
                "A larger value is not always better; these are not percentages.",
            ));
            out
        }
        Page::Personality => {
            let mut out = vec![text("Signed traits: -100..+100; 0 is the baseline.")];
            for (i, label) in [
                "Curiosity",
                "Kindness",
                "Crybaby / energetic",
                "Naive / normal",
                "Solitude",
                "Vitality",
                "Appetite",
                "Recovery",
                "Skillfulness",
                "Charm",
                "Chattiness",
                "Carefree",
                "Fickleness",
            ]
            .iter()
            .enumerate()
            {
                out.push(meter(label, i32::from(c.bytes[0x14A + i] as i8), -100, 100));
            }
            out.push(value("Favorite fruit group", c.bytes[0x157]));
            out.push(text(
                "Some hidden traits have limited or unused vanilla behavior.",
            ));
            out
        }
        Page::Bonds => {
            let mut out = vec![text(
                "Character    Affection  Familiarity  Interactions  Counter",
            )];
            for (i, name) in ["Sonic", "Shadow", "Tails", "Eggman", "Knuckles", "Rouge"]
                .iter()
                .enumerate()
            {
                let offset = 0x16C + i * 6;
                out.push(text(format!(
                    "{name:<11} {:>5}       {:>3}           {:>5}    {:>5}",
                    c.bytes[offset] as i8,
                    c.bytes[offset + 1],
                    u16_at(c, offset + 4),
                    u16_at(c, offset + 2)
                )));
            }
            out.extend([
                text("Affection: -100..+100. Familiarity: 0..200."),
                text("The last column is the game's stored bond counter."),
            ]);
            out
        }
        Page::Appearance => {
            let fields = [
                ("Eyes ID", 0xD1),
                ("Mouth ID", 0xD2),
                ("Emotion ball ID", 0xD3),
                ("Hat ID", 0xD5),
                ("Hide feet", 0xD6),
                ("Medal ID", 0xD7),
                ("Color ID", 0xD8),
                ("Monotone highlights", 0xD9),
                ("Texture ID", 0xDA),
                ("Shiny", 0xDB),
                ("Egg color ID", 0xDC),
                ("Body type ID", 0xDD),
                ("Animal body ID", 0xDE),
            ];
            let mut out: Vec<_> = fields
                .into_iter()
                .map(|(label, at)| value(label, c.bytes[at]))
                .collect();
            let animals = u32::from_le_bytes(c.bytes[0x118..0x11C].try_into().unwrap());
            out.push(value("Animals seen mask", format!("0x{animals:08X}")));
            for (i, label) in [
                "Arms", "Ears", "Forehead", "Horns", "Legs", "Tail", "Wings", "Face",
            ]
            .iter()
            .enumerate()
            {
                let part = c.bytes[0x11C + i] as i8;
                out.push(value(
                    &format!("Animal part: {label}"),
                    if part < 0 {
                        "none".into()
                    } else {
                        format!("ID {part}")
                    },
                ));
            }
            out
        }
        Page::Health => {
            let mut out = vec![text("Signed symptom counters; 0 is the baseline.")];
            for (i, label) in [
                "Cough",
                "Cold",
                "Rash",
                "Runny nose",
                "Hiccups",
                "Stomach ache",
            ]
            .iter()
            .enumerate()
            {
                out.push(meter(label, i32::from(c.bytes[0x15A + i] as i8), -100, 100));
            }
            out.push(text(
                "These are the raw condition counters used by the game.",
            ));
            out
        }
        Page::Raw => {
            let mut out = vec![
                text("Full read-only record, including unknown/unused fields."),
                text("Offsets at left; bytes are hexadecimal. PgUp/PgDn scroll."),
            ];
            for (i, bytes) in c.bytes.chunks(16).enumerate() {
                let mut row = format!("{:03X}:", i * 16);
                for byte in bytes {
                    let _ = write!(row, " {byte:02X}");
                }
                out.push(text(row));
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ChaoData {
        let mut c = ChaoData {
            bytes: [0; crate::chao::CHAO_SIZE],
        };
        c.bytes[0x80] = 2;
        c.bytes[0x81] = 1;
        c
    }

    #[test]
    fn inspection_preserves_signed_personality_and_bonds_and_unsigned_emotions() {
        let mut c = sample();
        c.bytes[0x14C] = (-80i8) as u8;
        c.bytes[0x12C] = 200;
        c.bytes[0x16C] = (-100i8) as u8;
        c.bytes[0x16D] = 200;
        c.bytes[0x138..0x13A].copy_from_slice(&10000u16.to_le_bytes());
        let before = c.bytes;
        assert!(
            lines(&c, Page::Personality)
                .iter()
                .any(|l| l.text.contains("-80"))
        );
        assert!(
            lines(&c, Page::Emotions)
                .iter()
                .any(|l| l.text.contains("200"))
        );
        assert!(
            lines(&c, Page::Bonds)
                .iter()
                .any(|l| l.text.contains("-100") && l.text.contains("200"))
        );
        assert!(
            lines(&c, Page::Needs)
                .iter()
                .any(|l| l.text.contains("10000"))
        );
        for page in Page::ALL {
            let _ = lines(&c, page);
        }
        assert_eq!(c.bytes, before);
    }

    #[test]
    fn eggs_and_nonfinite_evolution_values_are_not_reported_as_real_stats() {
        let mut c = sample();
        c.bytes[0x80] = TYPE_EGG;
        c.bytes[0x2E] = 0x48;
        assert!(lines(&c, Page::Abilities)[0].text.contains("egg"));
        c.bytes[0x80] = 2;
        c.bytes[0xB0..0xB4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(
            lines(&c, Page::Overview)
                .iter()
                .any(|l| l.text.contains("not initialized"))
        );
        assert!(
            lines(&c, Page::Abilities)
                .iter()
                .any(|l| l.text.contains("not initialized"))
        );
    }

    #[test]
    fn browsing_tracks_save_slots_and_survives_a_removed_chao() {
        let mut view = View {
            slot: Some(7),
            ..Default::default()
        };
        view.select(&[2, 7, 20], false);
        assert_eq!(view.slot, Some(20));
        view.select(&[2, 7, 20], false);
        assert_eq!(view.slot, Some(2));
        view.select(&[2, 7, 20], true);
        assert_eq!(view.slot, Some(20));
        view.retain_slot(&[2, 7]);
        assert_eq!(view.slot, Some(2));
        view.retain_slot(&[]);
        assert_eq!(view.slot, None);
        assert_eq!(Page::Overview.next(true), Page::Raw);
        assert_eq!(Page::Raw.next(false), Page::Overview);
    }

    #[test]
    fn form_ids_account_for_the_two_legacy_forms_before_normal_types() {
        assert_eq!(form(6), "Hero Normal");
        assert_eq!(form(20), "Neutral Chaos");
        assert_eq!(form(25), "Amy");
        assert!(form(255).contains("Unknown"));
    }

    #[test]
    fn raw_view_includes_the_entire_record_through_the_final_byte() {
        let mut c = sample();
        for (i, byte) in c.bytes.iter_mut().enumerate() {
            *byte = i as u8;
        }
        let rows = lines(&c, Page::Raw);
        assert_eq!(rows.len(), 2 + crate::chao::CHAO_SIZE / 16);
        assert!(rows[2].text.starts_with("000: 00 01 02"));
        assert!(rows.last().unwrap().text.starts_with("7F0: F0 F1 F2"));
        assert!(rows.last().unwrap().text.ends_with("FD FE FF"));
    }

    #[test]
    fn the_detail_panel_fits_small_windows_and_large_displays() {
        for (width, height) in [
            (320.0, 240.0),
            (640.0, 480.0),
            (1280.0, 720.0),
            (2560.0, 1440.0),
        ] {
            let (font, visible) = layout(width, height);
            assert!(64.0 * font <= width);
            assert!((visible + 12) as f32 * font <= height);
        }
    }
}

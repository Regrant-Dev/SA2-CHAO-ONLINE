//! What a Chao record says to a raiser, shared by F9 > Chao details (chao_details.rs) and the
//! status balloon (balloon.rs): grades, levels and points, alignment and leanings, what a child
//! would evolve into, time left, reincarnation, needs and mood. Rules and offsets:
//! re/chao-ai.md (evolution, life and age ticks, reincarnation) and re/chao-details.md.
//!
//! Only reads: nothing here changes a record.

use crate::chao::{CHAO_SIZE, ChaoData, TYPE_EGG};
use crate::chao_sim::evolve::evolve;
use crate::chao_sim::record::{Record, TYPE_CHILD};

/// The seven abilities in record order (`rec+0x20/0x28/0x30/0x38 + i`).
pub const ABILITIES: [&str; 7] = [
    "Swim",
    "Fly",
    "Run",
    "Power",
    "Stamina",
    "Luck",
    "Intelligence",
];
/// Progress toward the next level `rec+0x20+i` (0..99), grade `0x28+i`, level `0x30+i`,
/// points (u16) `0x38+2i`.
const PROGRESS: usize = 0x20;
const GRADES: usize = 0x28;
const LEVELS: usize = 0x30;
const POINTS: usize = 0x38;
/// Points stop growing here (`0x535E90`: `mov edx, 0xFA0`).
pub const POINTS_CAP: u16 = 4000;
pub const LEVEL_CAP: u8 = 99;

const HAPPINESS: usize = 0x82;
const AGE: usize = 0x86;
const ADULT_AGE: usize = 0x88;
const LIFE: usize = 0x8A;
const MAX_LIFE: usize = 0x8C;
const REINCARNATIONS: usize = 0x8E;
const RUN_POWER: usize = 0xA8;
const SWIM_FLY: usize = 0xAC;
const ALIGNMENT: usize = 0xB0;
const EVOLUTION: usize = 0xC0;
const ANIMALS: usize = 0x118;
pub const EMOTIONS: usize = 0x12C;
pub const NEEDS: usize = 0x134;
const SLEEPY: usize = 0x134;
const BOREDOM: usize = 0x13C;
const LONELY: usize = 0x13E;
pub const STRESS: usize = 0x142;
pub const HUNGER: usize = 0x138;

/// Evolution and alignment count from here on either side (`0x1312C94`, `0x1312C98`).
pub const LEANING: f32 = 0.5;
/// A child's cocoon starts at this evolution progress (picker #20 `0x562800`).
pub const COCOON: f32 = 1.0;
/// Rebirth needs happiness above this and stress below `STRESS_LIMIT` (`0x568F30`).
pub const REBIRTH_HAPPINESS: i16 = 50;
pub const STRESS_LIMIT: u16 = 700;
/// Chaos needs every animal, happiness above this and this many reincarnations (`0x5686B9`).
pub const ANIMAL_COUNT: u32 = 21;
pub const CHAOS_HAPPINESS: i16 = 80;
pub const CHAOS_REINCARNATIONS: u16 = 2;
/// Garden time per age tick: 121 life ticks of 96 frames at 60 frames a second.
pub const AGE_TICK_SECONDS: f32 = 121.0 * 96.0 / 60.0;

fn u16_at(rec: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([rec[at], rec[at + 1]])
}

fn i16_at(rec: &[u8], at: usize) -> i16 {
    u16_at(rec, at) as i16
}

/// A float field, or `None` when it holds no number (egg filler).
pub fn float(rec: &[u8], at: usize) -> Option<f32> {
    let v = f32::from_le_bytes([rec[at], rec[at + 1], rec[at + 2], rec[at + 3]]);
    v.is_finite().then_some(v)
}

pub fn happiness(rec: &[u8]) -> i16 {
    i16_at(rec, HAPPINESS)
}
pub fn age(rec: &[u8]) -> u16 {
    u16_at(rec, AGE)
}
pub fn adult_age(rec: &[u8]) -> u16 {
    u16_at(rec, ADULT_AGE)
}
pub fn life(rec: &[u8]) -> (u16, u16) {
    (u16_at(rec, LIFE), u16_at(rec, MAX_LIFE))
}
pub fn reincarnations(rec: &[u8]) -> u16 {
    u16_at(rec, REINCARNATIONS)
}
pub fn stress(rec: &[u8]) -> u16 {
    u16_at(rec, STRESS)
}
pub fn alignment(rec: &[u8]) -> Option<f32> {
    float(rec, ALIGNMENT)
}
/// Run (−) / Power (+) and Swim (−) / Fly (+) leanings.
pub fn run_power(rec: &[u8]) -> Option<f32> {
    float(rec, RUN_POWER)
}
pub fn swim_fly(rec: &[u8]) -> Option<f32> {
    float(rec, SWIM_FLY)
}
/// A child's way to its cocoon, 0..1 (1: the cocoon starts).
pub fn evolution(rec: &[u8]) -> Option<f32> {
    float(rec, EVOLUTION).map(|v| (v / COCOON).clamp(0.0, 1.0))
}
/// How many of the 21 animals it has absorbed.
pub fn animals(rec: &[u8]) -> u32 {
    (u32::from_le_bytes([
        rec[ANIMALS],
        rec[ANIMALS + 1],
        rec[ANIMALS + 2],
        rec[ANIMALS + 3],
    ]) & ((1 << ANIMAL_COUNT) - 1))
        .count_ones()
}

/// One ability as the record has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ability {
    /// 0..=6: E, D, C, B, A, S, X. Luck and Intelligence can hold any byte: some eggs get
    /// their grade genes from leftover stack bytes (`0x5506B0`), and the game levels them all
    /// the same.
    pub grade: u8,
    pub level: u8,
    pub points: u16,
    /// Toward the next level, 0..99.
    pub progress: u8,
}

impl Ability {
    /// Ability `i` (0..7), or `None` for an egg (its stat block is filler) or when its level,
    /// points or progress are out of the game's ranges.
    pub fn read(rec: &[u8], i: usize) -> Option<Self> {
        let a = Ability {
            grade: rec[GRADES + i],
            level: rec[LEVELS + i],
            points: u16_at(rec, POINTS + 2 * i),
            progress: rec[PROGRESS + i],
        };
        (rec[0x80] != TYPE_EGG && a.level <= LEVEL_CAP && a.points <= 9999 && a.progress <= 99)
            .then_some(a)
    }

    pub fn letter(self) -> char {
        grade_letter(self.grade)
    }

    /// The level's progress as a fraction (full at the top level).
    pub fn fraction(self) -> f32 {
        if self.level >= LEVEL_CAP {
            1.0
        } else {
            f32::from(self.progress) / 100.0
        }
    }
}

pub fn grade_letter(grade: u8) -> char {
    ['E', 'D', 'C', 'B', 'A', 'S', 'X']
        .get(usize::from(grade))
        .copied()
        .unwrap_or('?')
}

/// Points a level adds at a grade (`0x535E90`: `3 * grade + 11 + rand 0..=4`, for any grade
/// byte).
pub fn points_per_level(grade: u8) -> (u16, u16) {
    let base = 3 * u16::from(grade) + 11;
    (base, base + 4)
}

/// A grade's colour, 0xAARRGGBB: E grey, D white, C green, B cyan, A yellow, S orange,
/// X magenta; light grey for a grade past X.
pub fn grade_colour(grade: u8) -> u32 {
    [
        0xFFA0_A0A0,
        0xFFF0_F0F0,
        0xFF70_E070,
        0xFF60_E0FF,
        0xFFFF_E040,
        0xFFFF_A030,
        0xFFFF_70FF,
    ]
    .get(usize::from(grade))
    .copied()
    .unwrap_or(0xFFC8_C8C8)
}

/// Hero, Dark or Neutral.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Hero,
    Neutral,
    Dark,
}

impl Side {
    /// The side an alignment evolves to: Hero from +0.5, Dark from −0.5.
    pub fn of(alignment: f32) -> Self {
        if alignment >= LEANING {
            Self::Hero
        } else if alignment <= -LEANING {
            Self::Dark
        } else {
            Self::Neutral
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Hero => "Hero",
            Self::Neutral => "Neutral",
            Self::Dark => "Dark",
        }
    }
}

/// A form by its type byte `rec+0x80`.
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

/// `form`, short enough for the status balloon (the legacy forms without their note).
pub fn short_form(kind: u8) -> String {
    match kind {
        3 => "Good".into(),
        4 => "Bad".into(),
        _ => form(kind),
    }
}

/// The adult type a child would become if its cocoon started now (`0x568630` on a copy), or
/// `None` for a Chao that isn't a child or whose leanings hold no numbers.
pub fn evolves_into(rec: &[u8]) -> Option<u8> {
    if rec.len() < CHAO_SIZE || rec[0x80] != TYPE_CHILD {
        return None;
    }
    if [RUN_POWER, SWIM_FLY, ALIGNMENT]
        .iter()
        .any(|&at| float(rec, at).is_none())
    {
        return None;
    }
    let copy = ChaoData::from_bytes(&rec[..CHAO_SIZE])?;
    Some(evolve(&mut Record(copy)))
}

/// How a Chao's life goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeLeft {
    /// An egg or a child: the count starts once it is an adult.
    NotYet,
    /// Chaos and character Chao: their lifespan doesn't shrink.
    Ageless,
    /// Age ticks until its life reaches 0.
    Ticks(u32),
}

/// The age ticks left before an adult's life reaches 0: each one adds a year of adult age and
/// takes `(adult age + 100) / 15` off its maximum, which its life follows (re/chao-ai.md,
/// "Age tick").
pub fn life_left(rec: &[u8]) -> LifeLeft {
    let kind = rec[0x80];
    if kind <= TYPE_CHILD {
        return LifeLeft::NotYet;
    }
    if kind >= 0x14 {
        return LifeLeft::Ageless;
    }
    let (mut life, mut max) = (
        i32::from(i16_at(rec, LIFE)),
        i32::from(i16_at(rec, MAX_LIFE)),
    );
    let mut adult = i32::from(adult_age(rec));
    let mut ticks = 0;
    while life > 0 && ticks < 100_000 {
        adult += 1;
        max = (max - (adult + 100) / 15).max(0);
        life = life.min(max);
        ticks += 1;
    }
    LifeLeft::Ticks(ticks)
}

/// Hours of garden time for `ticks` age ticks.
pub fn hours(ticks: u32) -> f32 {
    ticks as f32 * AGE_TICK_SECONDS / 3600.0
}

/// What a Chao still needs to reincarnate when its life ends; empty when it would.
pub fn rebirth_blockers(rec: &[u8]) -> Vec<&'static str> {
    let mut out = Vec::new();
    if happiness(rec) <= REBIRTH_HAPPINESS {
        out.push("happiness above 50");
    }
    if stress(rec) >= STRESS_LIMIT {
        out.push("stress under 7%");
    }
    out
}

/// A need word `rec+at` (0..10000) as a whole percentage.
pub fn need_percent(rec: &[u8], at: usize) -> u16 {
    u16_at(rec, at).min(10_000) / 100
}

/// An emotion byte `rec+0x12C+i` (0..200) as a whole percentage.
pub fn emotion_percent(rec: &[u8], i: usize) -> u16 {
    u16::from(rec[EMOTIONS + i].min(200)) / 2
}

/// Needs that show as the mood once past `NEED_SHOWN`, and emotions (joy..dizziness; relax is
/// left out: it is full whenever the Chao is held) once past `EMOTION_SHOWN`.
const PRESSING: [(usize, &str); 3] = [(SLEEPY, "Sleepy"), (LONELY, "Lonely"), (BOREDOM, "Bored")];
const NEED_SHOWN: u16 = 7000;
const FEELINGS: [&str; 6] = ["Joyful", "Angry", "Sad", "Scared", "Hurt", "Dizzy"];
const EMOTION_SHOWN: u8 = 30;

/// The mood in a word: the most pressing need, else the strongest emotion, else "Calm".
pub fn mood(rec: &[u8]) -> &'static str {
    if let Some((_, word)) = PRESSING
        .iter()
        .map(|&(at, word)| (u16_at(rec, at), word))
        .filter(|&(v, _)| v > NEED_SHOWN)
        .max_by_key(|&(v, _)| v)
    {
        return word;
    }
    FEELINGS
        .iter()
        .zip(&rec[EMOTIONS..EMOTIONS + FEELINGS.len()])
        .filter(|(_, v)| **v >= EMOTION_SHOWN)
        .max_by_key(|(_, v)| **v)
        .map_or("Calm", |(word, _)| word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child() -> Vec<u8> {
        let mut rec = vec![0u8; CHAO_SIZE];
        rec[0x80] = TYPE_CHILD;
        rec
    }

    fn set_f32(rec: &mut [u8], at: usize, v: f32) {
        rec[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn grades_levels_and_points_per_level() {
        assert_eq!(grade_letter(0), 'E');
        assert_eq!(grade_letter(5), 'S');
        assert_eq!(grade_letter(6), 'X');
        assert_eq!(grade_letter(7), '?');
        assert_eq!(points_per_level(0), (11, 15));
        assert_eq!(points_per_level(3), (20, 24));
        assert_eq!(points_per_level(6), (29, 33));
        let mut rec = child();
        rec[GRADES + 6] = 5;
        rec[LEVELS + 6] = 99;
        rec[POINTS + 12..POINTS + 14].copy_from_slice(&4000u16.to_le_bytes());
        rec[PROGRESS + 6] = 99;
        let a = Ability::read(&rec, 6).unwrap();
        assert_eq!((a.letter(), a.level, a.points), ('S', 99, 4000));
        assert_eq!(a.fraction(), 1.0);
        rec[PROGRESS] = 44;
        assert_eq!(Ability::read(&rec, 0).unwrap().fraction(), 0.44);
        // A grade past X still levels (0x48 seen in a child's Intelligence).
        rec[GRADES + 6] = 0x48;
        let a = Ability::read(&rec, 6).unwrap();
        assert_eq!((a.letter(), a.level), ('?', 99));
        assert_eq!(points_per_level(0x48), (227, 231));
        rec[LEVELS + 1] = 100;
        assert_eq!(Ability::read(&rec, 1), None);
        rec[0x80] = TYPE_EGG;
        assert_eq!(Ability::read(&rec, 0), None);
    }

    #[test]
    fn a_child_evolves_by_the_games_rules() {
        let mut rec = child();
        assert_eq!(
            evolves_into(&rec).map(form).as_deref(),
            Some("Neutral Normal")
        );
        set_f32(&mut rec, RUN_POWER, 0.6);
        set_f32(&mut rec, ALIGNMENT, 0.5);
        assert_eq!(evolves_into(&rec).map(form).as_deref(), Some("Hero Power"));
        set_f32(&mut rec, SWIM_FLY, -0.7);
        set_f32(&mut rec, ALIGNMENT, -0.49);
        assert_eq!(
            evolves_into(&rec).map(form).as_deref(),
            Some("Neutral Swim")
        );
        // Every animal, happy and reborn twice: Chaos once both leanings are weak.
        let mut chaos = child();
        chaos[ANIMALS..ANIMALS + 4].copy_from_slice(&0x1F_FFFFu32.to_le_bytes());
        chaos[HAPPINESS..HAPPINESS + 2].copy_from_slice(&81i16.to_le_bytes());
        chaos[REINCARNATIONS..REINCARNATIONS + 2].copy_from_slice(&2u16.to_le_bytes());
        set_f32(&mut chaos, ALIGNMENT, -0.9);
        assert_eq!(
            evolves_into(&chaos).map(form).as_deref(),
            Some("Dark Chaos")
        );
        assert_eq!(animals(&chaos), 21);
        // Only children; no numbers, no guess. The record itself is left alone.
        set_f32(&mut rec, RUN_POWER, f32::NAN);
        let before = rec.clone();
        assert_eq!(evolves_into(&rec), None);
        assert_eq!(rec, before);
        let mut adult = child();
        adult[0x80] = 6;
        assert_eq!(evolves_into(&adult), None);
    }

    #[test]
    fn sides_count_from_one_half() {
        assert_eq!(Side::of(0.5), Side::Hero);
        assert_eq!(Side::of(0.49), Side::Neutral);
        assert_eq!(Side::of(-0.5), Side::Dark);
    }

    #[test]
    fn an_adults_time_left_follows_its_shrinking_lifespan() {
        let mut rec = child();
        assert_eq!(life_left(&rec), LifeLeft::NotYet);
        rec[0x80] = 5;
        rec[LIFE..LIFE + 2].copy_from_slice(&3000u16.to_le_bytes());
        rec[MAX_LIFE..MAX_LIFE + 2].copy_from_slice(&3000u16.to_le_bytes());
        // re/chao-ai.md's worked example: 221 ticks, about 11.9 hours.
        assert_eq!(life_left(&rec), LifeLeft::Ticks(221));
        assert!((hours(221) - 11.9).abs() < 0.05);
        rec[MAX_LIFE..MAX_LIFE + 2].copy_from_slice(&3900u16.to_le_bytes());
        rec[LIFE..LIFE + 2].copy_from_slice(&3900u16.to_le_bytes());
        assert_eq!(life_left(&rec), LifeLeft::Ticks(262));
        rec[LIFE..LIFE + 2].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(life_left(&rec), LifeLeft::Ticks(0));
        rec[0x80] = 20;
        assert_eq!(life_left(&rec), LifeLeft::Ageless);
    }

    #[test]
    fn rebirth_needs_happiness_and_little_stress() {
        let mut rec = child();
        rec[HAPPINESS..HAPPINESS + 2].copy_from_slice(&51i16.to_le_bytes());
        assert!(rebirth_blockers(&rec).is_empty());
        rec[STRESS..STRESS + 2].copy_from_slice(&700u16.to_le_bytes());
        assert_eq!(rebirth_blockers(&rec).len(), 1);
        rec[HAPPINESS..HAPPINESS + 2].copy_from_slice(&50i16.to_le_bytes());
        assert_eq!(rebirth_blockers(&rec).len(), 2);
    }

    #[test]
    fn the_mood_is_the_most_pressing_need_then_the_strongest_feeling() {
        let mut rec = child();
        assert_eq!(mood(&rec), "Calm");
        // Relax is full while held: it never shows.
        rec[EMOTIONS + 6] = 200;
        assert_eq!(mood(&rec), "Calm");
        rec[EMOTIONS + 1] = 120;
        assert_eq!(mood(&rec), "Angry");
        rec[LONELY..LONELY + 2].copy_from_slice(&8000u16.to_le_bytes());
        rec[SLEEPY..SLEEPY + 2].copy_from_slice(&9000u16.to_le_bytes());
        assert_eq!(mood(&rec), "Sleepy");
        assert_eq!(need_percent(&rec, SLEEPY), 90);
        assert_eq!(emotion_percent(&rec, 1), 60);
    }
}

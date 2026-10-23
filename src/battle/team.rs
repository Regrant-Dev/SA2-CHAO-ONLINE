//! Chao as fighters: the stat mapping, and the ROM data a battle needs (moves, names, the type
//! chart), read once from the player's own ROM.
//!
//! Every battle is at level 50. A Chao's stats map onto the battle stats:
//!
//! | Chao stat | battle stat |
//! |---|---|
//! | Stamina | HP |
//! | Power | Attack |
//! | Swim | Defense |
//! | Run | Speed |
//! | Fly | Sp. Defense, and up to 5% off foes' hit chance |
//! | Intelligence | Sp. Attack |
//! | Luck | critical-hit chance |
//!
//! Each stat's points count as a base stat between 20 (a new Chao) and 160 (3000 points, a
//! fully raised stat), straight in between, and its grade as the IV (E 0, D 5, ... S 25, X 30);
//! the battle stat is then Gen 3's formula at level 50 with no EVs. A new Chao's stats come out
//! near 30 (HP near 85) and a fully raised one's near 175 (HP near 230). Luck and intelligence
//! are hidden stats that few things raise, so each counts as at least half the Chao's average
//! points over the five shown stats. A stat the record doesn't fill in (egg filler) counts as 0.

use crate::chao::{ChaoData, GRADE_MAX, LEVEL_MAX};
use crate::gba::rom::{self, MOVE_COUNT, Rom};
use crate::gba_link::Profile;

use super::data::{MoveData, Target, TypeChart, TypeEntry};
use super::engine::Rules;
use super::fighter::{Fighter, Stats, stat_from_base};

/// Every battle is at this level.
pub const LEVEL: u8 = 50;
/// Points at which a stat counts as fully raised (an S-grade stat near level 99).
pub const POINTS_FULL: u32 = 3000;
/// Base stats a stat's points map onto.
const BASE_MIN: u32 = 20;
const BASE_MAX: u32 = 160;
/// IV per grade step.
const IV_PER_GRADE: u16 = 5;
/// Percent off foes' hit chance at full fly points.
const EVASION_EDGE_MAX: u32 = 5;
/// The move a fighter uses when it has nothing else left (the ROM's record for this id).
pub const OUT_OF_PP_MOVE: u16 = 165;
/// A battle that goes on this long is a draw.
pub const TURN_LIMIT: u32 = 200;

/// The record's stat indices.
mod stat {
    pub const SWIM: usize = 0;
    pub const FLY: usize = 1;
    pub const RUN: usize = 2;
    pub const POWER: usize = 3;
    pub const STAMINA: usize = 4;
    pub const LUCK: usize = 5;
    pub const INTELLIGENCE: usize = 6;
    /// The five the game shows.
    pub const SHOWN: [usize; 5] = [SWIM, FLY, RUN, POWER, STAMINA];
}

/// What the battle takes from a Chao's stats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChaoStats {
    pub stats: Stats,
    pub luck: u8,
    pub evasion_edge: u8,
}

/// A stat's (points, grade), or zeros when the record doesn't fill it in.
fn raw(chao: &ChaoData, index: usize) -> (u32, u8) {
    let (grade, level, points) = (chao.grade(index), chao.level(index), chao.points(index));
    if grade > GRADE_MAX || level > LEVEL_MAX || points < 0 {
        return (0, 0);
    }
    (points as u32, grade)
}

/// The base stat `points` count as.
fn base(points: u32) -> u16 {
    (BASE_MIN + points.min(POINTS_FULL) * (BASE_MAX - BASE_MIN) / POINTS_FULL) as u16
}

fn battle_stat(points: u32, grade: u8, hp: bool) -> u16 {
    stat_from_base(base(points), u16::from(grade) * IV_PER_GRADE, 0, LEVEL, hp)
}

/// The Chao's battle stats at `LEVEL` (see the module docs).
pub fn chao_stats(chao: &ChaoData) -> ChaoStats {
    let shown_average = stat::SHOWN.iter().map(|&s| raw(chao, s).0).sum::<u32>() / 5;
    let hidden = |index: usize| {
        let (points, grade) = raw(chao, index);
        (points.max(shown_average / 2), grade)
    };
    let get = |index: usize, hp: bool| {
        let (points, grade) = raw(chao, index);
        battle_stat(points, grade, hp)
    };
    let (intelligence, intelligence_grade) = hidden(stat::INTELLIGENCE);
    let (luck, _) = hidden(stat::LUCK);
    let (fly, _) = raw(chao, stat::FLY);
    ChaoStats {
        stats: Stats {
            hp: get(stat::STAMINA, true),
            attack: get(stat::POWER, false),
            defense: get(stat::SWIM, false),
            speed: get(stat::RUN, false),
            sp_attack: battle_stat(intelligence, intelligence_grade, false),
            sp_defense: get(stat::FLY, false),
        },
        luck: (luck.min(POINTS_FULL) * 255 / POINTS_FULL) as u8,
        evasion_edge: (fly.min(POINTS_FULL) * EVASION_EDGE_MAX / POINTS_FULL) as u8,
    }
}

/// Happiness (-100..=100) as the bond moves' 0..=255.
fn friendship(happiness: i16) -> u8 {
    ((i32::from(happiness.clamp(-100, 100)) + 100) * 255 / 200) as u8
}

/// A Chao as a fighter: its stats, its battle profile's types, ability and moves (the ones the
/// dex doesn't have are left out), at full HP.
pub fn fighter(name: &str, chao: &ChaoData, profile: &Profile, dex: &Dex) -> Fighter {
    let s = chao_stats(chao);
    let moves: Vec<MoveData> = profile
        .moves
        .iter()
        .filter_map(|&id| dex.move_data(id))
        .collect();
    let mut f = Fighter::new(name, LEVEL, s.stats, profile.types, profile.ability, &moves);
    f.luck = s.luck;
    f.evasion_edge = s.evasion_edge;
    f.friendship = friendship(chao.happiness());
    f
}

/// A ROM move record as the engine's.
pub fn move_data(id: u16, info: &rom::MoveInfo) -> MoveData {
    MoveData {
        id,
        effect: info.effect,
        power: info.power,
        type_id: info.type_id,
        accuracy: info.accuracy,
        pp: info.pp,
        chance: info.effect_chance,
        target: Target::from_rom(info.target),
        priority: info.priority,
        flags: info.flags,
    }
}

/// The ROM's type chart as the engine's: the same rows in the same order, split where the rows
/// an identified battler ignores start.
pub fn type_chart(chart: &rom::TypeChart) -> TypeChart {
    let mut out = TypeChart::default();
    let mut split = None;
    for m in &chart.matchups {
        if m.identify_removes && split.is_none() {
            split = Some(out.entries.len());
        }
        out.entries.push(TypeEntry {
            attack: m.attack,
            defend: m.defend,
            factor: m.tenths,
        });
    }
    out.identify_from = split.unwrap_or(out.entries.len());
    out
}

/// What a battle needs from the ROM, read once: every move with its name and description, the
/// ability and type names, the type chart and the sound moves. Names are plain ASCII (the HUD's font has nothing else).
pub struct Dex {
    moves: Vec<Option<MoveData>>,
    move_names: Vec<String>,
    move_descriptions: Vec<String>,
    ability_names: Vec<String>,
    type_names: Vec<String>,
    chart: TypeChart,
    out_of_pp: MoveData,
    sound_moves: Vec<u16>,
}

impl Dex {
    pub fn from_rom(rom: &Rom) -> Result<Dex, String> {
        let ascii = crate::gba_link::ascii;
        let moves: Vec<Option<MoveData>> = (0..MOVE_COUNT)
            .map(|id| rom.move_info(id).map(|info| move_data(id, &info)))
            .collect();
        let move_names = (0..MOVE_COUNT)
            .map(|id| rom.move_name(id).map(|n| ascii(&n)).unwrap_or_default())
            .collect();
        let move_descriptions = (0..MOVE_COUNT)
            .map(|id| {
                rom.move_description(id)
                    .map(|d| ascii(&d.replace('\n', " ")))
                    .unwrap_or_default()
            })
            .collect();
        let ability_names = (0..rom::ABILITY_COUNT)
            .map(|id| rom.ability_name(id).map(|n| ascii(&n)).unwrap_or_default())
            .collect();
        let type_names = (0..rom::TYPE_COUNT)
            .map(|id| rom.type_name(id).map(|n| ascii(&n)).unwrap_or_default())
            .collect();
        let out_of_pp = moves
            .get(usize::from(OUT_OF_PP_MOVE))
            .copied()
            .flatten()
            .ok_or("the ROM has no out-of-PP move")?;
        Ok(Dex {
            moves,
            move_names,
            move_descriptions,
            ability_names,
            type_names,
            chart: type_chart(rom.type_chart()),
            out_of_pp,
            sound_moves: rom.sound_moves(),
        })
    }

    /// A dex of plain moves for tests: `moves` by id, names "Move <id>".
    #[cfg(test)]
    pub fn for_tests(moves: &[MoveData], chart: TypeChart) -> Dex {
        let n = moves
            .iter()
            .map(|m| usize::from(m.id) + 1)
            .max()
            .unwrap_or(0);
        let mut table = vec![None; n.max(usize::from(OUT_OF_PP_MOVE) + 1)];
        for m in moves {
            table[usize::from(m.id)] = Some(*m);
        }
        let out_of_pp = MoveData::plain(OUT_OF_PP_MOVE, 0, 50);
        table[usize::from(OUT_OF_PP_MOVE)] = Some(out_of_pp);
        Dex {
            move_names: (0..table.len()).map(|i| format!("Move {i}")).collect(),
            moves: table,
            move_descriptions: Vec::new(),
            ability_names: Vec::new(),
            type_names: Vec::new(),
            chart,
            out_of_pp,
            sound_moves: Vec::new(),
        }
    }

    pub fn move_data(&self, id: u16) -> Option<MoveData> {
        // Id 0 is the ROM's empty move.
        (id != 0)
            .then(|| self.moves.get(usize::from(id)).copied().flatten())
            .flatten()
    }

    /// Every move a random-move effect can call.
    pub fn table(&self) -> Vec<MoveData> {
        self.moves.iter().skip(1).flatten().copied().collect()
    }

    /// The moves the sound-blocking ability stops.
    pub fn sound_moves(&self) -> &[u16] {
        &self.sound_moves
    }

    pub fn move_name(&self, id: u16) -> String {
        named(&self.move_names, usize::from(id), "Move")
    }

    /// The ROM's description of move `id` on one line (empty when it has none).
    pub fn move_description(&self, id: u16) -> &str {
        self.move_descriptions
            .get(usize::from(id))
            .map_or("", String::as_str)
    }

    pub fn ability_name(&self, id: u8) -> String {
        named(&self.ability_names, usize::from(id), "Ability")
    }

    pub fn type_name(&self, id: u8) -> String {
        named(&self.type_names, usize::from(id), "Type")
    }

    pub fn chart(&self) -> &TypeChart {
        &self.chart
    }

    pub fn rules(&self) -> Rules {
        Rules {
            out_of_pp: self.out_of_pp,
            turn_limit: TURN_LIMIT,
        }
    }
}

/// `names[index]`, or "<kind> <index>" when the ROM has no name for it.
fn named(names: &[String], index: usize, kind: &str) -> String {
    match names.get(index) {
        Some(n) if !n.trim().is_empty() => n.clone(),
        _ => format!("{kind} {index}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chao::CHAO_SIZE;

    /// A child Chao with every stat at (grade, level, points).
    fn chao(stats: [(u8, u8, i16); 7]) -> ChaoData {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        c.bytes[0x80] = 2;
        c.bytes[0x81] = 1;
        for (i, (grade, level, points)) in stats.into_iter().enumerate() {
            c.bytes[0x28 + i] = grade;
            c.bytes[0x30 + i] = level;
            c.bytes[0x38 + i * 2..0x3A + i * 2].copy_from_slice(&points.to_le_bytes());
        }
        c
    }

    #[test]
    fn a_new_chao_is_weak_but_usable_and_a_raised_one_is_strong() {
        let new = chao_stats(&chao([(2, 1, 30); 7]));
        assert!((25..=40).contains(&new.stats.attack), "{new:?}");
        assert!((75..=100).contains(&new.stats.hp), "{new:?}");
        assert_eq!(new.evasion_edge, 0);

        let raised = chao_stats(&chao([(5, 99, 3000); 7]));
        assert!((160..=190).contains(&raised.stats.attack), "{raised:?}");
        assert!((210..=250).contains(&raised.stats.hp), "{raised:?}");
        assert_eq!(raised.luck, 255);
        assert_eq!(raised.evasion_edge, 5);
        // More points never go past the full mark.
        assert_eq!(chao_stats(&chao([(5, 99, 9999); 7])).stats, raised.stats);
    }

    #[test]
    fn each_chao_stat_drives_its_battle_stat() {
        let mut stats = [(2u8, 10u8, 200i16); 7];
        let plain = chao_stats(&chao(stats)).stats;
        for (index, pick) in [
            (stat::STAMINA, (|s: &Stats| s.hp) as fn(&Stats) -> u16),
            (stat::POWER, |s| s.attack),
            (stat::SWIM, |s| s.defense),
            (stat::RUN, |s| s.speed),
            (stat::FLY, |s| s.sp_defense),
            (stat::INTELLIGENCE, |s| s.sp_attack),
        ] {
            stats[index] = (2, 40, 1500);
            let raised = chao_stats(&chao(stats)).stats;
            assert!(pick(&raised) > pick(&plain), "stat {index}");
            stats[index] = (2, 10, 200);
        }
        // A better grade is a little stronger with the same points.
        let mut graded = [(2u8, 10u8, 200i16); 7];
        graded[stat::POWER] = (5, 10, 200);
        assert!(chao_stats(&chao(graded)).stats.attack > plain.attack);
    }

    #[test]
    fn hidden_stats_count_as_half_the_shown_average_and_filler_counts_as_zero() {
        let mut stats = [(3u8, 50u8, 2000i16); 7];
        stats[stat::INTELLIGENCE] = (0, 0, 0);
        stats[stat::LUCK] = (0x48, 0xC4, -5);
        let s = chao_stats(&chao(stats));
        // Half of 2000 points: base 66.
        assert_eq!(s.stats.sp_attack, stat_from_base(66, 0, 0, LEVEL, false));
        assert_eq!(s.luck, (1000 * 255 / POINTS_FULL) as u8);
        // A filler stat among the shown ones counts as 0 points.
        stats[stat::POWER] = (0x48, 0xC4, 300);
        let s = chao_stats(&chao(stats));
        assert_eq!(s.stats.attack, stat_from_base(20, 0, 0, LEVEL, false));
    }

    #[test]
    fn happiness_becomes_the_bond() {
        assert_eq!(friendship(-100), 0);
        assert_eq!(friendship(0), 127);
        assert_eq!(friendship(100), 255);
        assert_eq!(friendship(i16::MAX), 255);
    }

    #[test]
    fn a_fighter_takes_the_profile_and_skips_unknown_moves() {
        let dex = Dex::for_tests(
            &[MoveData::plain(1, 0, 40), MoveData::plain(2, 10, 90)],
            TypeChart::default(),
        );
        let profile = Profile {
            types: [10, 2],
            ability: 7,
            moves: [2, 1, 300, 0],
            partner: Some(4),
        };
        let c = chao([(2, 10, 300); 7]);
        let f = fighter("Pip", &c, &profile, &dex);
        assert_eq!(f.name, "Pip");
        assert_eq!(f.level, LEVEL);
        assert_eq!((f.types, f.ability), ([10, 2], 7));
        let ids: Vec<u16> = f.moves.iter().map(|m| m.data.id).collect();
        assert_eq!(ids, [2, 1]);
        assert_eq!(f.hp, f.stats.hp);
    }

    #[test]
    fn the_chart_keeps_its_order_and_split() {
        let rom_chart = rom::TypeChart {
            matchups: vec![
                rom::TypeMatchup {
                    attack: 10,
                    defend: 12,
                    tenths: 20,
                    identify_removes: false,
                },
                rom::TypeMatchup {
                    attack: 0,
                    defend: 7,
                    tenths: 0,
                    identify_removes: true,
                },
            ],
        };
        let c = type_chart(&rom_chart);
        assert_eq!(c.entries.len(), 2);
        assert_eq!(c.identify_from, 1);
        assert_eq!(c.multiplier(0, [7, 7], false), 0);
        assert_eq!(c.multiplier(0, [7, 7], true), 100);
        assert_eq!(c.multiplier(10, [12, 12], true), 200);
    }

    #[test]
    fn reads_the_rom() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return;
        }
        let rom = Rom::open(&path).expect("ROM opens");
        let dex = Dex::from_rom(&rom).expect("dex");
        assert_eq!(dex.table().len(), usize::from(MOVE_COUNT) - 1);
        assert!(dex.move_data(0).is_none());
        let out = dex.rules().out_of_pp;
        assert_eq!(out.id, OUT_OF_PP_MOVE);
        assert!(out.power > 0);
        assert!(dex.chart().entries.len() > 50);
        assert!(dex.chart().identify_from < dex.chart().entries.len());
        assert!(!dex.sound_moves().is_empty());
        assert!(
            dex.sound_moves()
                .iter()
                .all(|&id| dex.move_data(id).is_some())
        );
        for id in 1..MOVE_COUNT {
            let name = dex.move_name(id);
            assert!(name.is_ascii() && !name.trim().is_empty(), "move {id}");
            let about = dex.move_description(id);
            assert!(about.is_ascii() && !about.trim().is_empty(), "move {id}");
        }
        for id in 1..rom::ABILITY_COUNT {
            assert!(!dex.ability_name(id).is_empty());
        }
        assert!(dex.type_name(10).is_ascii());
    }
}

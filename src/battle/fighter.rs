//! A fighter: one team member with its stats, moves, status and the battle state that goes away
//! when it leaves the field.

use super::data::MoveData;
use super::event::Slot;

/// Battle stats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub hp: u16,
    pub attack: u16,
    pub defense: u16,
    pub speed: u16,
    pub sp_attack: u16,
    pub sp_defense: u16,
}

/// Stats with stages, in stage index order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    Attack = 0,
    Defense = 1,
    Speed = 2,
    SpAttack = 3,
    SpDefense = 4,
    Accuracy = 5,
    Evasion = 6,
}

pub const STAGED_STATS: usize = 7;
pub const STAGE_MIN: i8 = -6;
pub const STAGE_MAX: i8 = 6;

impl Stat {
    pub const ALL: [Stat; STAGED_STATS] = [
        Stat::Attack,
        Stat::Defense,
        Stat::Speed,
        Stat::SpAttack,
        Stat::SpDefense,
        Stat::Accuracy,
        Stat::Evasion,
    ];
}

/// Stage ratios for the five battle stats: numerator, denominator at -6..=+6.
const STAGE_RATIOS: [(u32, u32); 13] = [
    (10, 40),
    (10, 35),
    (10, 30),
    (10, 25),
    (10, 20),
    (10, 15),
    (10, 10),
    (15, 10),
    (20, 10),
    (25, 10),
    (30, 10),
    (35, 10),
    (40, 10),
];

/// `value` at `stage` (-6..=6), rounded down.
pub fn staged(value: u32, stage: i8) -> u32 {
    let (n, d) = STAGE_RATIOS[(stage.clamp(STAGE_MIN, STAGE_MAX) + 6) as usize];
    value * n / d
}

/// The stat formula without a nature: `(2 * base + iv + ev / 4) * level / 100 + 5`, or for
/// HP `... + level + 10`.
pub fn stat_from_base(base: u16, iv: u16, ev: u16, level: u8, hp: bool) -> u16 {
    let core = (2 * u32::from(base) + u32::from(iv) + u32::from(ev) / 4) * u32::from(level) / 100;
    let v = if hp {
        core + u32::from(level) + 10
    } else {
        core + 5
    };
    v.min(u32::from(u16::MAX)) as u16
}

/// Major status: one at a time, kept when switching out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ailment {
    Sleep,
    Poison,
    /// Poison that hurts more each turn.
    BadPoison,
    Burn,
    Freeze,
    Paralysis,
}

/// A known move and its PP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveSlot {
    pub data: MoveData,
    pub pp: u8,
}

/// Where a fighter is during a two-turn move that takes it off the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hidden {
    Air,
    Underground,
    Underwater,
}

/// Why a fighter keeps using one move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockKind {
    /// The second turn of a two-turn move.
    Charging,
    /// 2 or 3 turns, then confusion.
    Rampage,
    /// Nobody sleeps while it lasts.
    Noise,
    /// Up to 5 hits, each stronger.
    Rolling,
    /// Two turns storing damage, then it hits back double.
    Storing,
}

/// A move the fighter is locked into; the choice made for it is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lock {
    pub kind: LockKind,
    pub data: MoveData,
    /// Its move slot, when it came from one.
    pub index: Option<usize>,
    pub target: Option<Slot>,
    /// Uses left after the next one (rampage, noise, rolling), or storing turns left.
    pub turns: u8,
}

/// Battle state that lasts one turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThisTurn {
    pub hurt: bool,
    pub shielded: bool,
    pub bracing: bool,
    /// An ally's helping move boosts its move.
    pub helped: bool,
    /// Bounces status moves back.
    pub bouncing: bool,
    /// Takes the next self-help move a foe uses.
    pub stealing: bool,
    /// HP lost to the last physical and the last special hit this turn, and who dealt each.
    pub physical: u32,
    pub physical_by: Option<Slot>,
    pub special: u32,
    pub special_by: Option<Slot>,
    /// Slots whose moves took HP from it this turn (bit `side * 2 + pos`).
    pub damaged_by: u8,
    /// It has had its go this turn.
    pub acted: bool,
    /// It couldn't use its move this turn.
    pub stopped: bool,
}

impl ThisTurn {
    pub fn damaged_by(&self, s: Slot) -> bool {
        self.damaged_by & (1 << (s.side * 2 + s.pos)) != 0
    }
}

/// What a fighter was before taking another's form, type or ability; it goes back on leaving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Original {
    pub types: [u8; 2],
    pub ability: u8,
    pub stats: Stats,
    /// Its own moves, when it took another's.
    pub moves: Option<Vec<MoveSlot>>,
}

/// Battle state that ends when the fighter leaves the field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Volatile {
    pub landed_types: Option<[u8; 2]>,
    pub water_heal: bool,
    pub floating: u8,
    pub lifted: u8,
    pub heal_barred: u8,
    pub silenced: u8,
    pub stages: [i8; STAGED_STATS],
    /// Turns of confusion left (0 = not confused).
    pub confusion: u8,
    /// Hit by a flinching move this turn: it can't act.
    pub flinched: bool,
    /// Extra critical-hit stages.
    pub crit_stages: u8,
    /// Identified: type immunities and evasion no longer help it.
    pub identified: bool,
    /// Turns of bad poison so far (1/16 more each turn).
    pub bad_poison_turns: u8,
    /// The last move it used.
    pub last_move: Option<u16>,
    /// The move slot of the last move it used (for repeats).
    pub last_index: Option<usize>,
    /// How new it is to the field: 2 when it comes in, one less before the first turn (for
    /// fighters sent out between turns) and at each turn's end.
    pub fresh: u8,
    /// HP of its decoy (0 = none).
    pub decoy: u16,
    pub lock: Option<Lock>,
    /// Out of reach between the turns of a two-turn move.
    pub hidden: Option<Hidden>,
    /// It must rest next turn.
    pub recharge: bool,
    /// Attack rises when hit.
    pub fuming: bool,
    /// Damage stored by a waiting move and who dealt the last of it.
    pub stored_damage: u32,
    pub stored_by: Option<Slot>,
    /// A move id it can't use, and turns left.
    pub sealed: Option<(u16, u8)>,
    /// The move slot it must repeat, and turns left.
    pub repeat: Option<(usize, u8)>,
    /// Turns it can only use damaging moves.
    pub provoked: u8,
    /// It can't use the same move twice in a row.
    pub no_repeats: bool,
    /// The fighter in this slot keeps it from switching.
    pub held_by: Option<Slot>,
    /// Bound for turns left by the move in this slot.
    pub bound: Option<(u8, Slot, u16)>,
    /// Loses HP each turn to the fighter in this slot.
    pub seeded_by: Option<Slot>,
    pub haunted_sleep: bool,
    pub hexed: bool,
    /// Turns until it faints (0 = no count).
    pub countdown: u8,
    /// Shielding moves used in a row (each lowers the next one's odds).
    pub shield_streak: u8,
    /// If it faints to a foe's move before its next move, the foe faints too.
    pub vengeful: bool,
    /// If it faints to a foe's move before its next move, that move loses all its PP.
    pub rancor: bool,
    /// The user in this slot never misses it, for turns left.
    pub sure_hit: Option<(Slot, u8)>,
    pub shrunk: bool,
    pub curled: bool,
    /// Hits in a row of a move that grows each hit.
    pub growth: u8,
    /// Stores held (0..=3).
    pub stored: u8,
    /// Its next electric move is doubled (turns left).
    pub powered: u8,
    pub rooted: bool,
    /// It falls asleep when this reaches 0 (turns left; 0 = no).
    pub drowsy: u8,
    /// Fire moves boosted by its ability.
    pub fire_boost: bool,
    pub dull_electric: bool,
    pub dull_fire: bool,
    /// Foes can't use moves it knows.
    pub sealing: bool,
    /// Its ability makes it loaf this turn.
    pub loafing: bool,
    /// The last move aimed at it.
    pub last_hit_by: Option<MoveData>,
    /// The type of the last move that took HP from it.
    pub last_hit_type: Option<u8>,
    /// A slot whose move was copied in: the slot and what it held.
    pub copied_slot: Option<(usize, MoveSlot)>,
    pub original: Option<Box<Original>>,
    pub turn: ThisTurn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fighter {
    /// For messages only.
    pub name: String,
    pub level: u8,
    pub stats: Stats,
    /// Type ids; equal for one type.
    pub types: [u8; 2],
    pub ability: u8,
    /// Up to 4.
    pub moves: Vec<MoveSlot>,
    pub hp: u16,
    pub ailment: Option<Ailment>,
    /// Turns of sleep left; kept while it's out of the field.
    pub sleep_turns: u8,
    /// 0..=255: raises its critical-hit chance by up to one base chance (1/16) on top.
    pub luck: u8,
    /// Percent taken off foes' hit chance against it.
    pub evasion_edge: u8,
    /// 0..=255: the bond with its owner, which powers the bond moves (255 by default).
    pub friendship: u8,
    /// Battle mass in hectograms; native Chao use a fixed 10 kg in this format.
    pub weight_hg: u16,
    pub volatile: Volatile,
}

impl Fighter {
    /// A fighter at full HP and PP. Moves past the fourth are dropped.
    pub fn new(
        name: &str,
        level: u8,
        stats: Stats,
        types: [u8; 2],
        ability: u8,
        moves: &[MoveData],
    ) -> Fighter {
        Fighter {
            name: name.to_string(),
            level: level.max(1),
            stats,
            types,
            ability,
            moves: moves
                .iter()
                .take(4)
                .map(|&data| MoveSlot { data, pp: data.pp })
                .collect(),
            hp: stats.hp,
            ailment: None,
            sleep_turns: 0,
            luck: 0,
            evasion_edge: 0,
            friendship: 255,
            weight_hg: 100,
            volatile: Volatile::default(),
        }
    }

    pub fn fainted(&self) -> bool {
        self.hp == 0
    }

    pub fn has_type(&self, type_id: u8) -> bool {
        self.battle_types().contains(&type_id)
    }

    pub fn battle_types(&self) -> [u8; 2] {
        if self.volatile.landed_types.is_none() {
            return self.types;
        }
        let kept = self
            .types
            .into_iter()
            .find(|&t| t != super::data::types::FLYING);
        match (self.types, kept) {
            (_, None) => [super::data::types::NORMAL; 2],
            ([a, b], Some(other))
                if a == super::data::types::FLYING || b == super::data::types::FLYING =>
            {
                [other; 2]
            }
            _ => self.types,
        }
    }

    pub fn grounded(&self) -> bool {
        self.volatile.rooted
            || (!self.has_type(super::data::types::FLYING)
                && self.volatile.floating == 0
                && self.volatile.lifted == 0
                && self.ability != super::abilities::IMMUNE_TO_GROUND
                && self.volatile.hidden != Some(Hidden::Air))
    }

    pub fn battle_weight(&self) -> u32 {
        match self.ability {
            super::abilities::DOUBLE_WEIGHT => u32::from(self.weight_hg) * 2,
            super::abilities::HALF_WEIGHT => u32::from(self.weight_hg) / 2,
            _ => u32::from(self.weight_hg),
        }
        .max(1)
    }

    pub fn stage(&self, stat: Stat) -> i8 {
        self.volatile.stages[stat as usize]
    }

    /// Moves up or down by `by` within -6..=6; returns how far it actually went.
    pub fn change_stage(&mut self, stat: Stat, by: i8) -> i8 {
        let old = self.stage(stat);
        let new = (old + by).clamp(STAGE_MIN, STAGE_MAX);
        self.volatile.stages[stat as usize] = new;
        new - old
    }

    /// Whether any move has PP left (else it can only use the out-of-PP move).
    pub fn has_pp(&self) -> bool {
        self.moves.iter().any(|m| m.pp > 0)
    }

    /// Loses up to `amount` HP; returns how much went.
    pub fn hurt(&mut self, amount: u32) -> u16 {
        let lost = amount.min(u32::from(self.hp)) as u16;
        self.hp -= lost;
        self.volatile.turn.hurt |= lost > 0;
        lost
    }

    /// Gains up to `amount` HP; returns how much came back.
    pub fn heal(&mut self, amount: u32) -> u16 {
        let room = self.stats.hp.saturating_sub(self.hp);
        let gained = amount.min(u32::from(room)) as u16;
        self.hp += gained;
        gained
    }

    /// Keeps what it is now so leaving the field can put it back (before a form, type or
    /// ability change).
    pub fn keep_original(&mut self, with_moves: bool) {
        let o = self.volatile.original.get_or_insert_with(|| {
            Box::new(Original {
                types: self.types,
                ability: self.ability,
                stats: self.stats,
                moves: None,
            })
        });
        if with_moves && o.moves.is_none() {
            o.moves = Some(self.moves.clone());
        }
    }

    /// Leaving the field: its own form, type, ability and moves come back, and stages,
    /// confusion and the rest of the volatile state go.
    pub fn withdraw(&mut self) {
        if let Some((index, slot)) = self.volatile.copied_slot
            && let Some(m) = self.moves.get_mut(index)
        {
            *m = slot;
        }
        if let Some(o) = self.volatile.original.take() {
            self.types = o.types;
            self.ability = o.ability;
            let hp = self.stats.hp;
            self.stats = o.stats;
            self.stats.hp = hp;
            if let Some(moves) = o.moves {
                self.moves = moves;
            }
        }
        self.volatile = Volatile::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_ratios_run_from_a_quarter_to_four_times() {
        assert_eq!(staged(100, 0), 100);
        assert_eq!(staged(100, 1), 150);
        assert_eq!(staged(100, 6), 400);
        assert_eq!(staged(100, -1), 66);
        assert_eq!(staged(100, -2), 50);
        assert_eq!(staged(100, -6), 25);
        assert_eq!(staged(99, -6), 24);
        // Out of range clamps.
        assert_eq!(staged(100, 9), 400);
    }

    #[test]
    fn stat_formula() {
        // Base 100, no IVs or EVs, level 50: 105 and HP 160.
        assert_eq!(stat_from_base(100, 0, 0, 50, false), 105);
        assert_eq!(stat_from_base(100, 0, 0, 50, true), 160);
        assert_eq!(stat_from_base(100, 31, 252, 50, false), 152);
        assert_eq!(stat_from_base(100, 31, 252, 100, true), 404);
    }

    #[test]
    fn stages_clamp_and_report_the_change() {
        let mut f = Fighter::new("a", 50, Stats::default(), [0, 0], 0, &[]);
        assert_eq!(f.change_stage(Stat::Attack, 2), 2);
        assert_eq!(f.change_stage(Stat::Attack, 6), 4);
        assert_eq!(f.change_stage(Stat::Attack, 1), 0);
        assert_eq!(f.change_stage(Stat::Evasion, -9), -6);
        f.withdraw();
        assert_eq!(f.stage(Stat::Attack), 0);
    }

    #[test]
    fn hp_changes_stay_in_bounds() {
        let stats = Stats {
            hp: 100,
            ..Stats::default()
        };
        let mut f = Fighter::new("a", 50, stats, [0, 0], 0, &[]);
        assert_eq!(f.hurt(30), 30);
        assert_eq!(f.heal(50), 30);
        assert_eq!(f.hurt(500), 100);
        assert!(f.fainted());
    }

    #[test]
    fn keeps_four_moves_with_full_pp() {
        let moves: Vec<MoveData> = (1..=6).map(|i| MoveData::plain(i, 0, 40)).collect();
        let f = Fighter::new("a", 0, Stats::default(), [0, 0], 0, &moves);
        assert_eq!(f.moves.len(), 4);
        assert!(f.moves.iter().all(|m| m.pp == 20));
        assert_eq!(f.level, 1);
    }
}

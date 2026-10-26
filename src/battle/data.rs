//! Battle data compiled from the imported source: moves, their targets and the type chart. The
//! engine only sees ids and numbers; names stay with the source reader and the battle screen.

/// Type ids (the source's type index).
pub mod types {
    pub const NORMAL: u8 = 0;
    pub const FIGHTING: u8 = 1;
    pub const FLYING: u8 = 2;
    pub const POISON: u8 = 3;
    pub const GROUND: u8 = 4;
    pub const ROCK: u8 = 5;
    pub const BUG: u8 = 6;
    pub const GHOST: u8 = 7;
    pub const STEEL: u8 = 8;
    pub const FIRE: u8 = 9;
    pub const WATER: u8 = 10;
    pub const GRASS: u8 = 11;
    pub const ELECTRIC: u8 = 12;
    pub const PSYCHIC: u8 = 13;
    pub const ICE: u8 = 14;
    pub const DRAGON: u8 = 15;
    pub const DARK: u8 = 16;
    pub const FAIRY: u8 = 17;
    /// No type, rather than a slot in the modern 18-type chart.
    pub const TYPELESS: u8 = 0xFF;
    /// Type ids 0..COUNT.
    pub const COUNT: u8 = 18;

    /// The damage category of synthetic test moves only: real moves carry their own.
    pub fn is_physical(type_id: u8) -> bool {
        type_id <= STEEL
    }

    pub fn is_special(type_id: u8) -> bool {
        (FIRE..COUNT).contains(&type_id)
    }
}

/// Engine move flag bits; the source compiler sets them from a move record's flags and from
/// its listed mechanics.
pub mod flags {
    pub const CONTACT: u32 = 1 << 0;
    pub const BLOCKED_BY_PROTECTION: u32 = 1 << 1;
    pub const REFLECTABLE: u32 = 1 << 2;
    pub const STEALABLE: u32 = 1 << 3;
    pub const COPYABLE: u32 = 1 << 4;
    pub const FLINCH_ITEM: u32 = 1 << 5;
    pub const SOUND: u32 = 1 << 6;
    pub const POWDER: u32 = 1 << 7;
    pub const BULLET: u32 = 1 << 8;
    pub const PUNCH: u32 = 1 << 9;
    pub const PULSE: u32 = 1 << 10;
    pub const BITE: u32 = 1 << 11;
    pub const DANCE: u32 = 1 << 12;
    pub const HEAL: u32 = 1 << 13;
    pub const BYPASS_DECOY: u32 = 1 << 14;
    pub const TARGET_DEFENSE: u32 = 1 << 15;
    pub const TARGET_ATTACK: u32 = 1 << 16;
    pub const IGNORE_DEFENSE_STAGES: u32 = 1 << 17;
    pub const ALWAYS_CRITICAL: u32 = 1 << 18;
    pub const IGNORE_EVASION: u32 = 1 << 19;
    pub const SWAP_DEFENSE_STATS: u32 = 1 << 20;
    /// A frozen user thaws and moves.
    pub const THAWS_USER: u32 = 1 << 21;
    /// Double damage on a shrunk target.
    pub const PUNISHES_SHRINK: u32 = 1 << 22;
    /// Reaches a target hiding in the air, underground or underwater.
    pub const REACHES_AIR: u32 = 1 << 23;
    pub const REACHES_UNDERGROUND: u32 = 1 << 24;
    pub const REACHES_UNDERWATER: u32 = 1 << 25;
    /// Double damage on a hiding target it reaches.
    pub const DOUBLE_ON_REACH: u32 = 1 << 26;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Status,
    Physical,
    Special,
}

/// Validated source-driven details, independent of an archive's effect-sequence numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MoveDetails {
    /// Explicit ordinary semantic handled by the engine, never a source effect sequence.
    pub special: u16,
    pub stages: [i8; 7],
    pub stage_chances: [u8; 7],
    pub inflict: Option<super::fighter::Ailment>,
    pub inflict_chance: u8,
    pub confuse_chance: u8,
    pub flinch_chance: u8,
    pub self_stats: bool,
    pub drain_percent: u8,
    pub recoil_percent: u8,
}

/// Who a move goes to (the record's target byte, one bit each).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// One chosen battler: a foe, or in doubles the ally too.
    Selected,
    /// Decided by the move's effect: the foe that last hurt the user (moves returning damage),
    /// else a random foe.
    Depends,
    /// The user (or a chosen battler).
    UserOrSelected,
    /// A random foe.
    RandomFoe,
    /// Both foes; in doubles it does 3/4 damage while both are up.
    BothFoes,
    /// The user.
    User,
    /// Everyone else on the field, the ally too; 3/4 damage while it hits more than one.
    AllOthers,
    /// The foes' side of the field rather than a battler.
    FoesField,
}

impl Target {
    pub fn from_rom(bits: u8) -> Target {
        match bits {
            0 => Target::Selected,
            b if b & 0x01 != 0 => Target::Depends,
            b if b & 0x02 != 0 => Target::UserOrSelected,
            b if b & 0x04 != 0 => Target::RandomFoe,
            b if b & 0x08 != 0 => Target::BothFoes,
            b if b & 0x10 != 0 => Target::User,
            b if b & 0x20 != 0 => Target::AllOthers,
            _ => Target::FoesField,
        }
    }

    pub fn to_rom(self) -> u8 {
        match self {
            Target::Selected => 0,
            Target::Depends => 0x01,
            Target::UserOrSelected => 0x02,
            Target::RandomFoe => 0x04,
            Target::BothFoes => 0x08,
            Target::User => 0x10,
            Target::AllOthers => 0x20,
            Target::FoesField => 0x40,
        }
    }
}

/// A move record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveData {
    pub id: u16,
    /// What it does besides damage (`effects`).
    pub effect: u8,
    /// Base power; 0 for moves that don't deal normal damage.
    pub power: u8,
    pub type_id: u8,
    pub category: Category,
    pub details: MoveDetails,
    /// Percent; 0 never misses.
    pub accuracy: u8,
    pub pp: u8,
    /// Percent chance of the effect on a damaging move.
    pub chance: u8,
    pub target: Target,
    pub priority: i8,
    pub flags: u32,
}

impl MoveData {
    /// A plain move for tests and placeholders: neutral target, 100% accuracy, 20 PP.
    pub fn plain(id: u16, type_id: u8, power: u8) -> MoveData {
        MoveData {
            id,
            effect: 0,
            power,
            type_id,
            category: if power == 0 {
                Category::Status
            } else if types::is_physical(type_id) {
                Category::Physical
            } else {
                Category::Special
            },
            details: MoveDetails::default(),
            accuracy: 100,
            pp: 20,
            chance: 0,
            target: Target::Selected,
            priority: 0,
            flags: 0,
        }
    }
}

/// One row of the type chart: `factor` tenths of damage (0, 5 or 20) for `attack` against a
/// battler of type `defend`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeEntry {
    pub attack: u8,
    pub defend: u8,
    pub factor: u8,
}

/// The catalog's type chart: a list of rows in table order. Rows from `identify_from` on are the
/// ones skipped against a battler that has been identified (its type immunities stop).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeChart {
    pub entries: Vec<TypeEntry>,
    pub identify_from: usize,
}

/// The catalog table's markers: a row whose first byte is `IDENTIFY_MARK` splits the table, one
/// starting with `END` ends it.
pub const IDENTIFY_MARK: u8 = 0xFE;
pub const END: u8 = 0xFF;

impl TypeChart {
    /// Reads the catalog table's 3-byte rows (attacking type, defending type, factor) up to the end
    /// marker. Without a split marker every row always applies.
    pub fn parse(bytes: &[u8]) -> TypeChart {
        let mut chart = TypeChart::default();
        let mut split = None;
        for row in bytes.as_chunks::<3>().0 {
            match row[0] {
                END => break,
                IDENTIFY_MARK => split = Some(chart.entries.len()),
                _ => chart.entries.push(TypeEntry {
                    attack: row[0],
                    defend: row[1],
                    factor: row[2],
                }),
            }
        }
        chart.identify_from = split.unwrap_or(chart.entries.len());
        chart
    }

    /// The factors (tenths) that apply to a move of `attack` hitting `defend`, in table order;
    /// a second type equal to the first counts once.
    pub fn factors(&self, attack: u8, defend: [u8; 2], identified: bool) -> Vec<u8> {
        let rows = if identified {
            &self.entries[..self.identify_from.min(self.entries.len())]
        } else {
            &self.entries[..]
        };
        let mut out = Vec::new();
        for e in rows.iter().filter(|e| e.attack == attack) {
            if e.defend == defend[0] {
                out.push(e.factor);
            }
            if e.defend == defend[1] && defend[0] != defend[1] {
                out.push(e.factor);
            }
        }
        out
    }

    /// The overall multiplier in hundredths (0, 25, 50, 100, 200, 400).
    pub fn multiplier(&self, attack: u8, defend: [u8; 2], identified: bool) -> u32 {
        self.factors(attack, defend, identified)
            .iter()
            .fold(100, |m, &f| m * u32::from(f) / 10)
    }

    /// Applies the factors to `damage` one at a time (rounding down each time,
    /// never below 1 unless a factor is 0).
    pub fn apply(&self, damage: i32, attack: u8, defend: [u8; 2], identified: bool) -> i32 {
        let mut d = damage;
        for f in self.factors(attack, defend, identified) {
            d = d * i32::from(f) / 10;
            if d == 0 && f != 0 {
                d = 1;
            }
        }
        d
    }
}

/// How a hit's type worked out, for the message after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effectiveness {
    Normal,
    Super,
    NotVery,
    None,
}

impl Effectiveness {
    pub fn from_multiplier(hundredths: u32) -> Effectiveness {
        match hundredths {
            0 => Effectiveness::None,
            m if m > 100 => Effectiveness::Super,
            m if m < 100 => Effectiveness::NotVery,
            _ => Effectiveness::Normal,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::types::*;
    use super::*;

    /// A small chart in the catalog's format: a few rows, the split marker, then the immunities
    /// identifying removes.
    pub fn chart_bytes() -> Vec<u8> {
        let rows: &[[u8; 3]] = &[
            [FIRE, GRASS, 20],
            [FIRE, WATER, 5],
            [FIRE, FIRE, 5],
            [WATER, FIRE, 20],
            [WATER, WATER, 5],
            [WATER, GRASS, 5],
            [GRASS, WATER, 20],
            [GRASS, FIRE, 5],
            [ELECTRIC, WATER, 20],
            [ELECTRIC, GROUND, 0],
            [FIGHTING, NORMAL, 20],
            [FIGHTING, FLYING, 5],
            [ICE, GRASS, 20],
            [ICE, FLYING, 20],
            [GROUND, FLYING, 0],
            [IDENTIFY_MARK, IDENTIFY_MARK, 0],
            [NORMAL, GHOST, 0],
            [FIGHTING, GHOST, 0],
            [END, END, 0],
        ];
        rows.iter().flatten().copied().collect()
    }

    pub fn chart() -> TypeChart {
        TypeChart::parse(&chart_bytes())
    }

    #[test]
    fn parses_rows_and_the_split() {
        let c = chart();
        assert_eq!(c.entries.len(), 17);
        assert_eq!(c.identify_from, 15);
        assert_eq!(
            c.entries[0],
            TypeEntry {
                attack: FIRE,
                defend: GRASS,
                factor: 20
            }
        );
        // No split marker: everything applies.
        let plain = TypeChart::parse(&[FIRE, GRASS, 20, END, 0, 0, NORMAL, GHOST, 0]);
        assert_eq!((plain.entries.len(), plain.identify_from), (1, 1));
    }

    #[test]
    fn multipliers_combine_both_types() {
        let c = chart();
        assert_eq!(c.multiplier(FIRE, [GRASS, GRASS], false), 200);
        assert_eq!(c.multiplier(ICE, [GRASS, FLYING], false), 400);
        assert_eq!(c.multiplier(FIRE, [WATER, FIRE], false), 25);
        assert_eq!(c.multiplier(FIRE, [GRASS, WATER], false), 100);
        assert_eq!(c.multiplier(ELECTRIC, [WATER, GROUND], false), 0);
        assert_eq!(c.multiplier(NORMAL, [NORMAL, NORMAL], false), 100);
        assert_eq!(
            Effectiveness::from_multiplier(c.multiplier(FIRE, [GRASS, WATER], false)),
            Effectiveness::Normal
        );
        assert_eq!(Effectiveness::from_multiplier(25), Effectiveness::NotVery);
        assert_eq!(Effectiveness::from_multiplier(400), Effectiveness::Super);
        assert_eq!(Effectiveness::from_multiplier(0), Effectiveness::None);
    }

    #[test]
    fn identified_targets_lose_the_rows_after_the_split() {
        let c = chart();
        assert_eq!(c.multiplier(NORMAL, [GHOST, GHOST], false), 0);
        assert_eq!(c.multiplier(NORMAL, [GHOST, GHOST], true), 100);
        assert_eq!(c.multiplier(FIGHTING, [NORMAL, GHOST], true), 200);
        // Rows before the split still apply.
        assert_eq!(c.multiplier(GROUND, [FLYING, FLYING], true), 0);
    }

    #[test]
    fn apply_rounds_each_step() {
        let c = chart();
        // 3 * 0.5 = 1 (1.5 down), then * 0.5 = 0 -> 1.
        assert_eq!(c.apply(3, FIRE, [WATER, FIRE], false), 1);
        assert_eq!(c.apply(37, FIRE, [GRASS, GRASS], false), 74);
        assert_eq!(c.apply(37, ELECTRIC, [GROUND, GROUND], false), 0);
    }

    #[test]
    fn targets_round_trip() {
        for bits in [0u8, 1, 2, 4, 8, 16, 32, 64] {
            assert_eq!(Target::from_rom(bits).to_rom(), bits);
        }
        assert_eq!(Target::from_rom(0x08), Target::BothFoes);
        assert_eq!(Target::from_rom(0x20), Target::AllOthers);
    }
}

//! Battle data that comes from the ROM: move records, their targets and the type chart. The
//! engine only sees ids and numbers; names stay with the ROM reader and the battle screen.

/// Type ids (the ROM's type table index).
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
    /// The unused type between the two halves: moves of it do no damage.
    pub const TYPELESS: u8 = 9;
    pub const FIRE: u8 = 10;
    pub const WATER: u8 = 11;
    pub const GRASS: u8 = 12;
    pub const ELECTRIC: u8 = 13;
    pub const PSYCHIC: u8 = 14;
    pub const ICE: u8 = 15;
    pub const DRAGON: u8 = 16;
    pub const DARK: u8 = 17;
    /// Type ids 0..COUNT.
    pub const COUNT: u8 = 18;

    /// Gen 3 splits physical and special by type: the ids below the typeless one are physical
    /// (attack against defense), the ones above it special.
    pub fn is_physical(type_id: u8) -> bool {
        type_id < TYPELESS
    }

    pub fn is_special(type_id: u8) -> bool {
        type_id > TYPELESS
    }
}

/// Move flag bits (the record's last byte).
pub mod flags {
    pub const CONTACT: u8 = 1 << 0;
    pub const BLOCKED_BY_PROTECTION: u8 = 1 << 1;
    pub const REFLECTABLE: u8 = 1 << 2;
    pub const STEALABLE: u8 = 1 << 3;
    pub const COPYABLE: u8 = 1 << 4;
    pub const FLINCH_ITEM: u8 = 1 << 5;
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
    /// Both foes; in doubles it does half damage while both are up.
    BothFoes,
    /// The user.
    User,
    /// Everyone else on the field, the ally too, at full damage.
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
    /// Percent; 0 never misses.
    pub accuracy: u8,
    pub pp: u8,
    /// Percent chance of the effect on a damaging move.
    pub chance: u8,
    pub target: Target,
    pub priority: i8,
    pub flags: u8,
}

impl MoveData {
    /// A plain move for tests and placeholders: neutral target, 100% accuracy, 20 PP.
    pub fn plain(id: u16, type_id: u8, power: u8) -> MoveData {
        MoveData {
            id,
            effect: 0,
            power,
            type_id,
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

/// The ROM's type chart: a list of rows in table order. Rows from `identify_from` on are the
/// ones skipped against a battler that has been identified (its type immunities stop).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeChart {
    pub entries: Vec<TypeEntry>,
    pub identify_from: usize,
}

/// The ROM table's markers: a row whose first byte is `IDENTIFY_MARK` splits the table, one
/// starting with `END` ends it.
pub const IDENTIFY_MARK: u8 = 0xFE;
pub const END: u8 = 0xFF;

impl TypeChart {
    /// Reads the ROM table's 3-byte rows (attacking type, defending type, factor) up to the end
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

    /// Applies the factors to `damage` one at a time as Gen 3 does (rounding down each time,
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

    /// A small chart in the ROM's format: a few rows, the split marker, then the immunities
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

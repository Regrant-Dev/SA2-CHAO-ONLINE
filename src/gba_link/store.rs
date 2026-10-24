//! What GBA link keeps in `<mod>/gba/partners.txt`, one record a line:
//!
//! - `partner <chao identity> <species> <type> <type> <move> <move> <move> <move> <level>
//!   <ability> <active move> <active move> <active move> <active move> <from a save>
//!   <save moves>`: the partner a Chao absorbed last and the four moves the Chao battles with,
//!   by its `ChaoData::identity` (the SA2 save is never changed for this). `<from a save>` is 1
//!   when that partner came from a GBA save, and `<save moves>` has bit `i` set when active move
//!   `i` was only taught by ones from a save. Lines without the last two fields load with
//!   neither; lines without the last seven also load with no ability recorded and the partner's
//!   moves as the active ones.
//! - `moves <chao identity> <active move> <active move> <active move> <active move> [<move>
//!   <only from saves>]...`: every move the Chao has learned (from partners, wild visitors, the
//!   gift and TM moves), oldest first, each with 1 when only partners from a GBA save taught it,
//!   and the four it battles with (0: an empty slot). A Chao with a `partner` line and no
//!   `moves` line knows the partner line's active moves.
//! - `types <chao identity> <active type> <active type> [<type> <only from saves>]...`: every
//!   type the Chao got from partners, oldest first, each with 1 when only partners from a GBA
//!   save gave it, and the one or two it battles as (the same twice for one, 255 twice for
//!   none).
//! - `abilities <chao identity> <active ability> [<ability> <only from saves>]...`: the same for
//!   abilities, with the one it battles with (0: none). A Chao with a `partner` line and no
//!   `types` or `abilities` line has its partner line's types and ability, active.
//! - `level <chao identity> <level> <xp>`: its battle level (1..=100) and the XP it has toward
//!   the next (`battle::levels`). A Chao without one is at level 5 with none.
//! - `import <key> <garden> <day>`: a creature brought into a garden, by `save::Mon::key`, and
//!   the day (Unix days); each comes to each garden once a day.
//! - `brought <garden> <key> <species> <type> <type> <move>... <level> <ability> <from a save>`:
//!   brought and not absorbed yet; it's there each time the garden loads. Lines without the last
//!   field load as not from a save. `waiting` lines (without it, with a base speed in place of
//!   the ability) load with no ability recorded.
//!
//! - `kept <key> <species> <level> <move> <move> <move> <move>`: a creature the mod keeps (the
//!   first-partner gift), offered like the GBA save's.
//! - `gift <species>`: the gift was taken (once only).
//!
//! Numbers are decimal except the identity and key (hex). Ability 0 is "not recorded": the
//! battle profile takes the species' first ability from the ROM then (`profile`).

use std::collections::BTreeMap;

use crate::battle::levels::Level;

/// Most partners waiting in one garden at once.
pub const PER_GARDEN: usize = 4;

/// A creature as GBA link keeps it: ids from the save and the ROM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partner {
    pub species: u16,
    pub types: [u8; 2],
    pub moves: [u16; 4],
    pub level: u8,
    /// Its ability id; 0 when not recorded.
    pub ability: u8,
    /// It came from a GBA save (not a wild visitor, the gift or the mod's own list).
    pub from_save: bool,
}

/// What a Chao has from absorbing: its last partner and the four moves it battles with (0 for
/// an empty slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Learned {
    pub partner: Partner,
    pub moves: [u16; 4],
    /// Bit `i`: `moves[i]` was only taught by partners from a GBA save.
    pub save_moves: u8,
}

/// Every move a Chao has learned, and the four it battles with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moves {
    /// Each move it learned, oldest first, and whether only partners from a GBA save taught it.
    pub known: Vec<(u16, bool)>,
    /// The ones it battles with, each one it knows (0: an empty slot).
    pub active: [u16; 4],
}

/// Why a move can't be switched on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveRefused {
    /// The Chao doesn't know it.
    Unknown,
    /// It's the only one it battles with.
    LastOne,
    /// It battles with four already.
    Full,
}

impl MoveRefused {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unknown => "It doesn't know that move",
            Self::LastOne => "A Chao needs at least one move",
            Self::Full => "4 moves already: turn one off first",
        }
    }
}

impl Moves {
    /// What a Chao known only by its partner record knows: the record's moves (its marks), all
    /// of them active.
    pub fn from_learned(l: &Learned) -> Moves {
        let known = l
            .moves
            .iter()
            .enumerate()
            .rev()
            .filter(|&(_, &id)| id != 0)
            .map(|(i, &id)| (id, l.save_moves & 1 << i != 0))
            .collect();
        Moves {
            known,
            active: l.moves,
        }
        .tidy()
    }

    pub fn knows(&self, id: u16) -> bool {
        id != 0 && self.known.iter().any(|&(k, _)| k == id)
    }

    /// Whether only partners from a GBA save taught it.
    pub fn only_from_saves(&self, id: u16) -> bool {
        self.known.iter().any(|&(k, marked)| k == id && marked)
    }

    pub fn is_active(&self, id: u16) -> bool {
        id != 0 && self.active.contains(&id)
    }

    /// The active moves' save marks (bit `i` for `active[i]`, as `Learned::save_moves`).
    pub fn active_marks(&self) -> u8 {
        self.active
            .iter()
            .enumerate()
            .filter(|&(_, &id)| self.only_from_saves(id))
            .fold(0, |marks, (i, _)| marks | 1 << i)
    }

    /// Adds `id` to the known moves (`from_save`: taught by a partner from a GBA save); a move
    /// it knew keeps its mark only when taught by one again. Whether it's new.
    fn add(&mut self, id: u16, from_save: bool) -> bool {
        if id == 0 {
            return false;
        }
        match self.known.iter_mut().find(|(k, _)| *k == id) {
            Some((_, marked)) => {
                *marked &= from_save;
                false
            }
            None => {
                self.known.push((id, from_save));
                true
            }
        }
    }

    /// Puts `id` in the first empty active slot; whether there was one.
    fn fill(&mut self, id: u16) -> bool {
        if self.is_active(id) {
            return true;
        }
        match self.active.iter_mut().find(|m| **m == 0) {
            Some(slot) => {
                *slot = id;
                true
            }
            None => false,
        }
    }

    /// A partner (`from_save`: from a GBA save) teaches `taught`: the new ones join the known
    /// moves and fill empty active slots; the active ones stay otherwise, and a known move
    /// switched off stays off. The moves new to it.
    pub fn learn(&mut self, taught: [u16; 4], from_save: bool) -> Vec<u16> {
        let mut new = Vec::new();
        for id in taught {
            if self.add(id, from_save) {
                new.push(id);
                self.fill(id);
            }
        }
        new
    }

    /// A TM move `id` is taught in place of active move `replacing` (or into an empty slot, or
    /// the last one): it's known (never only from a save) and active. Whether it's new.
    pub fn teach_tm(&mut self, id: u16, replacing: u16) -> bool {
        if id == 0 {
            return false;
        }
        let new = self.add(id, false);
        if let Some(m) = self.known.iter_mut().find(|(k, _)| *k == id) {
            m.1 = false;
        }
        if self.is_active(id) {
            return new;
        }
        match self.active.iter().position(|&m| m == replacing && m != 0) {
            Some(at) => self.active[at] = id,
            None => {
                if !self.fill(id) {
                    self.active[3] = id;
                }
            }
        }
        new
    }

    /// Switches move `id` on (into an empty slot) or off (at least one stays on). Whether it's
    /// on now.
    pub fn toggle(&mut self, id: u16) -> Result<bool, MoveRefused> {
        if !self.knows(id) {
            return Err(MoveRefused::Unknown);
        }
        if let Some(at) = self.active.iter().position(|&m| m == id) {
            if self.active.iter().filter(|&&m| m != 0).count() <= 1 {
                return Err(MoveRefused::LastOne);
            }
            self.active[at] = 0;
            return Ok(false);
        }
        if self.fill(id) {
            Ok(true)
        } else {
            Err(MoveRefused::Full)
        }
    }

    /// Without empty or repeated known moves, and active ones it doesn't know or has twice.
    fn tidy(mut self) -> Moves {
        let mut seen = Vec::new();
        self.known.retain(|&(id, _)| {
            let keep = id != 0 && !seen.contains(&id);
            seen.push(id);
            keep
        });
        for i in 0..self.active.len() {
            let id = self.active[i];
            if !self.knows(id) || self.active[..i].contains(&id) {
                self.active[i] = 0;
            }
        }
        self
    }
}

/// An active type slot with no type in it (`Traits::active_types`).
pub const NO_TYPE: u8 = 0xFF;

/// Every type and ability a Chao got from partners, and the ones it battles with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Traits {
    /// Each type it got, oldest first, and whether only partners from a GBA save gave it.
    pub types: Vec<(u8, bool)>,
    /// Each ability it got (never 0), oldest first, the same way.
    pub abilities: Vec<(u8, bool)>,
    /// The types it battles as: the same twice for one, `NO_TYPE` twice for none.
    pub active_types: [u8; 2],
    /// The ability it battles with (0: none).
    pub ability: u8,
}

impl Default for Traits {
    fn default() -> Self {
        Traits {
            types: Vec::new(),
            abilities: Vec::new(),
            active_types: [NO_TYPE; 2],
            ability: 0,
        }
    }
}

/// Why a type or ability can't be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraitRefused {
    /// The Chao never got it.
    Unknown,
    /// It's the only type it battles as.
    LastType,
    /// It battles as two types already.
    TwoTypes,
}

impl TraitRefused {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unknown => "It never got that from a partner",
            Self::LastType => "A Chao needs at least one type",
            Self::TwoTypes => "2 types already: turn one off first",
        }
    }
}

/// Adds `id` to `list` (`from_save`: given by a partner from a GBA save); one it had keeps its
/// mark only when given by one again. Whether it's new.
fn add_marked<T: PartialEq + Copy>(list: &mut Vec<(T, bool)>, id: T, from_save: bool) -> bool {
    match list.iter_mut().find(|(k, _)| *k == id) {
        Some((_, marked)) => {
            *marked &= from_save;
            false
        }
        None => {
            list.push((id, from_save));
            true
        }
    }
}

impl Traits {
    /// What a Chao known only by its partner record has: the partner's types and ability (its
    /// mark), active. None for no partner (species 0).
    pub fn from_partner(p: &Partner) -> Traits {
        let mut t = Traits::default();
        t.learn(p);
        t
    }

    /// The types `types` (the same twice for one) and `ability` (0: none), active, as another
    /// player's game sends them; never only from a save.
    pub fn chosen(types: [u8; 2], ability: u8) -> Traits {
        Traits::from_partner(&Partner {
            species: 1,
            types,
            moves: [0; 4],
            level: 0,
            ability,
            from_save: false,
        })
    }

    /// Partner `p` gives its types and ability: new ones join the lists. The ones it battles
    /// with only change when it had none yet (then they're the partner's).
    pub fn learn(&mut self, p: &Partner) {
        if p.species == 0 {
            return;
        }
        let had_types = !self.types.is_empty();
        for t in p.types {
            if t != NO_TYPE {
                add_marked(&mut self.types, t, p.from_save);
            }
        }
        if !had_types && p.types[0] != NO_TYPE {
            self.active_types = if p.types[1] == NO_TYPE {
                [p.types[0]; 2]
            } else {
                p.types
            };
        }
        if p.ability != 0 {
            let had = !self.abilities.is_empty();
            add_marked(&mut self.abilities, p.ability, p.from_save);
            if !had {
                self.ability = p.ability;
            }
        }
    }

    pub fn knows_type(&self, t: u8) -> bool {
        t != NO_TYPE && self.types.iter().any(|&(k, _)| k == t)
    }

    pub fn knows_ability(&self, a: u8) -> bool {
        a != 0 && self.abilities.iter().any(|&(k, _)| k == a)
    }

    /// Whether only partners from a GBA save gave type `t`.
    pub fn type_only_from_saves(&self, t: u8) -> bool {
        self.types.iter().any(|&(k, marked)| k == t && marked)
    }

    /// Whether only partners from a GBA save gave ability `a`.
    pub fn ability_only_from_saves(&self, a: u8) -> bool {
        self.abilities.iter().any(|&(k, marked)| k == a && marked)
    }

    pub fn is_type_active(&self, t: u8) -> bool {
        t != NO_TYPE && self.active_types.contains(&t)
    }

    /// The types it battles as, one or two (none when it has none).
    pub fn active_type_list(&self) -> Vec<u8> {
        let [a, b] = self.active_types;
        match (a != NO_TYPE, b != NO_TYPE && b != a) {
            (true, true) => vec![a, b],
            (true, false) => vec![a],
            (false, true) => vec![b],
            (false, false) => Vec::new(),
        }
    }

    /// Switches type `t` on (as a second type) or off (one stays on). Whether it's on now.
    pub fn toggle_type(&mut self, t: u8) -> Result<bool, TraitRefused> {
        if !self.knows_type(t) {
            return Err(TraitRefused::Unknown);
        }
        let active = self.active_type_list();
        if active.contains(&t) {
            return match active.iter().find(|&&a| a != t) {
                Some(&other) => {
                    self.active_types = [other; 2];
                    Ok(false)
                }
                None => Err(TraitRefused::LastType),
            };
        }
        self.active_types = match active[..] {
            [] => [t; 2],
            [a] => [a, t],
            _ => return Err(TraitRefused::TwoTypes),
        };
        Ok(true)
    }

    /// Battles with ability `a` from now on (0: none).
    pub fn choose_ability(&mut self, a: u8) -> Result<(), TraitRefused> {
        if a != 0 && !self.knows_ability(a) {
            return Err(TraitRefused::Unknown);
        }
        self.ability = a;
        Ok(())
    }

    /// Without repeated or empty entries, and active ones it doesn't have.
    fn tidy(mut self) -> Traits {
        let mut seen = Vec::new();
        self.types.retain(|&(t, _)| {
            let keep = t != NO_TYPE && !seen.contains(&t);
            seen.push(t);
            keep
        });
        let mut seen = Vec::new();
        self.abilities.retain(|&(a, _)| {
            let keep = a != 0 && !seen.contains(&a);
            seen.push(a);
            keep
        });
        let known: Vec<u8> = self
            .active_type_list()
            .into_iter()
            .filter(|&t| self.knows_type(t))
            .collect();
        self.active_types = match known[..] {
            [] => [NO_TYPE; 2],
            [a] => [a; 2],
            [a, b, ..] => [a, b],
        };
        if !self.knows_ability(self.ability) {
            self.ability = 0;
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub garden: u8,
    pub key: u64,
    pub partner: Partner,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Store {
    /// Chao identity -> what it has from its partners.
    pub partners: BTreeMap<u64, Learned>,
    /// Chao identity -> every move it learned (`moves_of` for the ones with a partner only).
    pub moves: BTreeMap<u64, Moves>,
    /// Chao identity -> every type and ability it got (`traits_of` for the ones with a partner
    /// only).
    pub traits: BTreeMap<u64, Traits>,
    /// Chao identity -> its battle level (`level` for the ones without).
    pub levels: BTreeMap<u64, Level>,
    /// (key, garden, day).
    pub imports: Vec<(u64, u8, u32)>,
    pub waiting: Vec<Waiting>,
    /// Creatures the mod keeps: (key, species, level, moves).
    pub kept: Vec<(u64, u16, u8, [u16; 4])>,
    /// The species taken as the gift, once it's been taken.
    pub gift: Option<u16>,
}

/// What a file has of one Chao: its last partner, every move, type and ability it got (and the
/// ones it battles with) and its battle level.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub partner: Option<Partner>,
    pub moves: Moves,
    pub traits: Traits,
    pub level: Level,
}

/// Why an import isn't allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// It's already been to this garden today.
    Today,
    /// It's waiting in this garden now.
    Here,
    /// The garden has `PER_GARDEN` waiting.
    Full,
}

impl Refused {
    pub fn message(self) -> &'static str {
        match self {
            Self::Today => "Already came to this garden today",
            Self::Here => "Already in this garden",
            Self::Full => "This garden has 4 partners waiting; let your Chao meet them first",
        }
    }
}

/// The moves a Chao knows after a partner with `taught` moves: those first, then the ones it
/// knew that still fit, four at most, no repeats and no empty slots between.
pub fn teach(known: [u16; 4], taught: [u16; 4]) -> [u16; 4] {
    let mut out = [0u16; 4];
    let mut n = 0;
    for id in taught.into_iter().chain(known) {
        if n < out.len() && id != 0 && !out[..n].contains(&id) {
            out[n] = id;
            n += 1;
        }
    }
    out
}

impl Store {
    pub fn parse(text: &str) -> Self {
        Self::parse_with_taught(text, &BTreeMap::new())
    }

    /// `parse`, with the TM moves an older trainer progress file taught (Chao identity -> the
    /// TM move in each active slot, 0 for none) added to the Chao that have no `moves` line
    /// yet.
    pub fn parse_with_taught(text: &str, taught: &BTreeMap<u64, [u16; 4]>) -> Self {
        let mut s = Self::default();
        // Chao identity -> (a `types` line was read, an `abilities` line was).
        let mut parts: BTreeMap<u64, (bool, bool)> = BTreeMap::new();
        for line in text.lines() {
            let mut w = line.split_whitespace();
            let kind = w.next();
            let rest: Vec<&str> = w.collect();
            match kind {
                Some("partner") if matches!(rest.len(), 9 | 14 | 16) => {
                    let ability = rest.get(9).map_or(Some(0), |v| v.parse().ok());
                    let from_save = rest.get(14).map_or(Some(false), |v| flag(v));
                    let save_moves = rest
                        .get(15)
                        .map_or(Some(0), |v| v.parse::<u8>().ok().filter(|m| *m < 16));
                    if let (
                        Ok(id),
                        Some(partner),
                        Some(ability),
                        Some(from_save),
                        Some(save_moves),
                    ) = (
                        u64::from_str_radix(rest[0], 16),
                        partner(&rest[1..9]),
                        ability,
                        from_save,
                        save_moves,
                    ) {
                        let partner = Partner {
                            ability,
                            from_save,
                            ..partner
                        };
                        let moves = if rest.len() >= 14 {
                            moves(&rest[10..14])
                        } else {
                            Some(teach([0; 4], partner.moves))
                        };
                        if let Some(moves) = moves {
                            s.partners.insert(
                                id,
                                Learned {
                                    partner,
                                    moves,
                                    save_moves,
                                },
                            );
                        }
                    }
                }
                Some("import") if rest.len() == 3 => {
                    if let (Ok(key), Ok(garden), Ok(day)) = (
                        u64::from_str_radix(rest[0], 16),
                        rest[1].parse(),
                        rest[2].parse(),
                    ) {
                        s.imports.push((key, garden, day));
                    }
                }
                Some(k @ ("brought" | "waiting"))
                    if rest.len() == 11 || (k == "brought" && rest.len() == 12) =>
                {
                    let ability = if k == "brought" {
                        rest[10].parse().ok()
                    } else {
                        rest[10].parse::<u8>().ok().map(|_| 0)
                    };
                    let from_save = rest.get(11).map_or(Some(false), |v| flag(v));
                    if let (Ok(garden), Ok(key), Some(partner), Some(ability), Some(from_save)) = (
                        rest[0].parse(),
                        u64::from_str_radix(rest[1], 16),
                        partner(&rest[2..10]),
                        ability,
                        from_save,
                    ) {
                        s.waiting.push(Waiting {
                            garden,
                            key,
                            partner: Partner {
                                ability,
                                from_save,
                                ..partner
                            },
                        });
                    }
                }
                Some("kept") if rest.len() == 7 => {
                    if let (Ok(key), Ok(species), Ok(level), Some(m)) = (
                        u64::from_str_radix(rest[0], 16),
                        rest[1].parse::<u16>(),
                        rest[2].parse::<u8>(),
                        moves(&rest[3..7]),
                    ) {
                        s.kept.push((key, species, level, m));
                    }
                }
                Some("moves") if rest.len() >= 5 && (rest.len() - 5).is_multiple_of(2) => {
                    let known: Option<Vec<(u16, bool)>> = rest[5..]
                        .chunks(2)
                        .map(|p| Some((p[0].parse().ok()?, flag(p[1])?)))
                        .collect();
                    if let (Ok(id), Some(active), Some(known)) =
                        (u64::from_str_radix(rest[0], 16), moves(&rest[1..5]), known)
                    {
                        s.moves.insert(id, Moves { known, active }.tidy());
                    }
                }
                Some("types") if rest.len() >= 3 && (rest.len() - 3).is_multiple_of(2) => {
                    let known: Option<Vec<(u8, bool)>> = rest[3..]
                        .chunks(2)
                        .map(|p| Some((p[0].parse().ok()?, flag(p[1])?)))
                        .collect();
                    if let (Ok(id), Ok(a), Ok(b), Some(known)) = (
                        u64::from_str_radix(rest[0], 16),
                        rest[1].parse::<u8>(),
                        rest[2].parse::<u8>(),
                        known,
                    ) {
                        let t = s.traits.entry(id).or_default();
                        t.types = known;
                        t.active_types = [a, b];
                        parts.entry(id).or_default().0 = true;
                    }
                }
                Some("abilities") if rest.len() >= 2 && rest.len().is_multiple_of(2) => {
                    let known: Option<Vec<(u8, bool)>> = rest[2..]
                        .chunks(2)
                        .map(|p| Some((p[0].parse().ok()?, flag(p[1])?)))
                        .collect();
                    if let (Ok(id), Ok(active), Some(known)) = (
                        u64::from_str_radix(rest[0], 16),
                        rest[1].parse::<u8>(),
                        known,
                    ) {
                        let t = s.traits.entry(id).or_default();
                        t.abilities = known;
                        t.ability = active;
                        parts.entry(id).or_default().1 = true;
                    }
                }
                Some("level") if rest.len() == 3 => {
                    if let (Ok(id), Ok(level), Ok(xp)) = (
                        u64::from_str_radix(rest[0], 16),
                        rest[1].parse::<u8>(),
                        rest[2].parse::<u32>(),
                    ) {
                        s.levels.insert(id, Level::new(level, xp));
                    }
                }
                Some("gift") if rest.len() == 1 => s.gift = rest[0].parse().ok(),
                _ => {}
            }
        }
        // A Chao with only one of the two lines has the other from its partner line.
        for (identity, (types, abilities)) in parts {
            let seed = s
                .partners
                .get(&identity)
                .map(|l| Traits::from_partner(&l.partner))
                .unwrap_or_default();
            if let Some(t) = s.traits.get_mut(&identity) {
                if !types {
                    t.types = seed.types;
                    t.active_types = seed.active_types;
                }
                if !abilities {
                    t.abilities = seed.abilities;
                    t.ability = seed.ability;
                }
                *t = std::mem::take(t).tidy();
            }
        }
        for (&identity, tms) in taught {
            if s.moves.contains_key(&identity) || tms.iter().all(|&m| m == 0) {
                continue;
            }
            let mut m = s.moves_of(identity);
            for (slot, &id) in tms.iter().enumerate() {
                if id == 0 {
                    continue;
                }
                // As the progress file put them: a TM move it battles with elsewhere changes
                // places with the move in the TM's slot.
                m.add(id, false);
                if let Some(j) = m.active.iter().position(|&a| a == id) {
                    m.active[j] = m.active[slot];
                }
                m.active[slot] = id;
            }
            s.set_moves(identity, m);
        }
        s
    }

    /// Takes the gift `species` at `level` with `moves` (`key` a new creature key): `false` when
    /// a gift was taken before.
    pub fn take_gift(&mut self, key: u64, species: u16, level: u8, moves: [u16; 4]) -> bool {
        if self.gift.is_some() {
            return false;
        }
        self.gift = Some(species);
        self.kept.push((key, species, level, moves));
        true
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let fields = |p: &Partner| {
            format!(
                "{} {} {} {} {} {} {} {}",
                p.species,
                p.types[0],
                p.types[1],
                p.moves[0],
                p.moves[1],
                p.moves[2],
                p.moves[3],
                p.level
            )
        };
        for (id, l) in &self.partners {
            let m = l.moves;
            out += &format!(
                "partner {id:016x} {} {} {} {} {} {} {} {}\n",
                fields(&l.partner),
                l.partner.ability,
                m[0],
                m[1],
                m[2],
                m[3],
                u8::from(l.partner.from_save),
                l.save_moves
            );
        }
        for (id, m) in &self.moves {
            let a = m.active;
            out += &format!("moves {id:016x} {} {} {} {}", a[0], a[1], a[2], a[3]);
            for &(move_id, marked) in &m.known {
                out += &format!(" {move_id} {}", u8::from(marked));
            }
            out += "\n";
        }
        for (id, t) in &self.traits {
            let [a, b] = t.active_types;
            out += &format!("types {id:016x} {a} {b}");
            for &(ty, marked) in &t.types {
                out += &format!(" {ty} {}", u8::from(marked));
            }
            out += &format!("\nabilities {id:016x} {}", t.ability);
            for &(ability, marked) in &t.abilities {
                out += &format!(" {ability} {}", u8::from(marked));
            }
            out += "\n";
        }
        for (id, l) in &self.levels {
            out += &format!("level {id:016x} {} {}\n", l.level, l.xp);
        }
        for (key, garden, day) in &self.imports {
            out += &format!("import {key:016x} {garden} {day}\n");
        }
        for w in &self.waiting {
            out += &format!(
                "brought {} {:016x} {} {} {}\n",
                w.garden,
                w.key,
                fields(&w.partner),
                w.partner.ability,
                u8::from(w.partner.from_save)
            );
        }
        for (key, species, level, m) in &self.kept {
            out += &format!(
                "kept {key:016x} {species} {level} {} {} {} {}\n",
                m[0], m[1], m[2], m[3]
            );
        }
        if let Some(species) = self.gift {
            out += &format!("gift {species}\n");
        }
        out
    }

    /// Drops import records from before `today`.
    pub fn forget_old(&mut self, today: u32) {
        self.imports.retain(|&(_, _, day)| day >= today);
    }

    /// Whether `key` may come to `garden` today.
    pub fn can_import(&self, key: u64, garden: u8, today: u32) -> Result<(), Refused> {
        let here = |w: &&Waiting| w.garden == garden;
        if self.waiting.iter().filter(here).any(|w| w.key == key) {
            return Err(Refused::Here);
        }
        if self
            .imports
            .iter()
            .any(|&(k, g, d)| k == key && g == garden && d == today)
        {
            return Err(Refused::Today);
        }
        if self.waiting.iter().filter(here).count() >= PER_GARDEN {
            return Err(Refused::Full);
        }
        Ok(())
    }

    /// Brings `partner` (`key`) to `garden`, if allowed.
    pub fn import(
        &mut self,
        key: u64,
        garden: u8,
        today: u32,
        partner: Partner,
    ) -> Result<(), Refused> {
        self.can_import(key, garden, today)?;
        self.imports.push((key, garden, today));
        self.waiting.push(Waiting {
            garden,
            key,
            partner,
        });
        Ok(())
    }

    /// A Chao (`identity`) absorbed the waiting partner `key` of `garden`: it's gone from the
    /// garden and is that Chao's partner now (replacing any before), which teaches it its moves.
    /// Returns what the Chao has now and the moves new to it.
    pub fn absorbed(&mut self, garden: u8, key: u64, identity: u64) -> Option<(Learned, Vec<u16>)> {
        let at = self
            .waiting
            .iter()
            .position(|w| w.garden == garden && w.key == key)?;
        let partner = self.waiting.remove(at).partner;
        Some(self.learn(identity, partner))
    }

    /// The waiting partner `key` of `garden` was absorbed elsewhere (another player's game
    /// taught a Chao in a shared garden): it waits no more, and teaches nothing here. Whether it
    /// was waiting.
    pub fn drop_waiting(&mut self, garden: u8, key: u64) -> bool {
        let before = self.waiting.len();
        self.waiting
            .retain(|w| !(w.garden == garden && w.key == key));
        self.waiting.len() != before
    }

    /// A Chao (`identity`) absorbed `partner`, which never waited here (a wild visitor): as
    /// `absorbed`. Its moves join the ones the Chao knows (`Moves::learn`), and its types and
    /// ability the ones it got (`Traits::learn`). Returns what the Chao has now and the moves
    /// new to it.
    pub fn learn(&mut self, identity: u64, partner: Partner) -> (Learned, Vec<u16>) {
        let mut t = self.traits_of(identity);
        t.learn(&partner);
        self.traits.insert(identity, t);
        let mut m = self.moves_of(identity);
        let new = m.learn(partner.moves, partner.from_save);
        let learned = Learned {
            partner,
            moves: m.active,
            save_moves: m.active_marks(),
        };
        self.partners.insert(identity, learned);
        self.moves.insert(identity, m);
        (learned, new)
    }

    /// Every move the Chao with `identity` learned (from its partner record when it has no list
    /// yet; none at all for one that learned none).
    pub fn moves_of(&self, identity: u64) -> Moves {
        self.moves
            .get(&identity)
            .cloned()
            .or_else(|| self.partners.get(&identity).map(Moves::from_learned))
            .unwrap_or_default()
    }

    /// Keeps `moves` as the Chao's (active ones it doesn't know left out); its partner record
    /// carries the active ones.
    pub fn set_moves(&mut self, identity: u64, moves: Moves) {
        let moves = moves.tidy();
        if let Some(l) = self.partners.get_mut(&identity) {
            l.moves = moves.active;
            l.save_moves = moves.active_marks();
        }
        self.moves.insert(identity, moves);
    }

    /// Every type and ability the Chao with `identity` got, and the ones it battles with (from
    /// its partner record when it has no lists yet; none for one without a partner).
    pub fn traits_of(&self, identity: u64) -> Traits {
        self.traits
            .get(&identity)
            .cloned()
            .or_else(|| {
                self.partners
                    .get(&identity)
                    .map(|l| Traits::from_partner(&l.partner))
            })
            .unwrap_or_default()
    }

    /// Keeps `traits` as the Chao's (active ones it doesn't have left out).
    pub fn set_traits(&mut self, identity: u64, traits: Traits) {
        self.traits.insert(identity, traits.tidy());
    }

    /// The Chao's battle level (a new Chao's when it has none).
    pub fn level(&self, identity: u64) -> Level {
        self.levels.get(&identity).copied().unwrap_or_default()
    }

    /// What this file has of the Chao with `identity` (`None` when it has nothing of it).
    pub fn record(&self, identity: u64) -> Option<Record> {
        let partner = self.partners.get(&identity).map(|l| l.partner);
        let known = partner.is_some()
            || self.moves.contains_key(&identity)
            || self.traits.contains_key(&identity)
            || self.levels.contains_key(&identity);
        known.then(|| Record {
            partner,
            moves: self.moves_of(identity),
            traits: self.traits_of(identity),
            level: self.level(identity),
        })
    }

    /// The Chao it has anything of (a partner, moves, types and abilities or a level).
    pub fn identities(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .partners
            .keys()
            .chain(self.moves.keys())
            .chain(self.traits.keys())
            .chain(self.levels.keys())
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    pub fn waiting_in(&self, garden: u8) -> impl Iterator<Item = &Waiting> {
        self.waiting.iter().filter(move |w| w.garden == garden)
    }
}

/// `<species> <type> <type> <move> <move> <move> <move> <level>`, with no ability recorded.
fn partner(f: &[&str]) -> Option<Partner> {
    let b = |i: usize| f.get(i).and_then(|v| v.parse::<u8>().ok());
    Some(Partner {
        species: f.first()?.parse().ok()?,
        types: [b(1)?, b(2)?],
        moves: moves(f.get(3..7)?)?,
        level: b(7)?,
        ability: 0,
        from_save: false,
    })
}

/// `0` or `1`.
fn flag(v: &str) -> Option<bool> {
    match v {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn moves(f: &[&str]) -> Option<[u16; 4]> {
    let n = |i: usize| f.get(i).and_then(|v| v.parse::<u16>().ok());
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(species: u16) -> Partner {
        Partner {
            species,
            types: [3, 7],
            moves: [1, 2, 0, 300],
            level: 42,
            ability: 9,
            from_save: false,
        }
    }

    #[test]
    fn round_trips_through_text() {
        let mut s = Store::default();
        s.partners.insert(
            0xDEAD_BEEF_0123_4567,
            Learned {
                partner: p(5),
                moves: [300, 1, 2, 44],
                save_moves: 0,
            },
        );
        s.partners.insert(
            0x77,
            Learned {
                partner: Partner {
                    from_save: true,
                    ..p(6)
                },
                moves: [1, 2, 3, 4],
                save_moves: 0b0101,
            },
        );
        s.import(0xAB, 2, 20_000, p(9)).unwrap();
        s.import(
            0xEF,
            2,
            20_000,
            Partner {
                from_save: true,
                ..p(10)
            },
        )
        .unwrap();
        s.import(
            0xCD,
            3,
            20_000,
            Partner {
                ability: 0,
                ..p(400)
            },
        )
        .unwrap();
        assert!(s.take_gift(0x55, 7, 5, [1, 2, 0, 0]));
        assert!(!s.take_gift(0x56, 8, 5, [1, 0, 0, 0]));
        assert_eq!(s.kept.len(), 1);
        s.set_moves(
            0x77,
            Moves {
                known: vec![(9, false), (1, true), (2, false), (3, true), (4, false)],
                active: [1, 2, 3, 4],
            },
        );
        // A Chao with TM moves and no partner, and levels.
        s.set_moves(
            0x88,
            Moves {
                known: vec![(40, false)],
                active: [0, 40, 0, 0],
            },
        );
        s.levels.insert(0x77, Level::new(23, 150));
        s.levels.insert(0x99, Level::new(100, 0));
        assert_eq!(s.partners[&0x77].save_moves, 0b0101);
        let text = s.to_text();
        assert!(text.contains("moves 0000000000000077 1 2 3 4 9 0 1 1 2 0 3 1 4 0\n"));
        assert!(text.contains("level 0000000000000077 23 150\n"));
        let back = Store::parse(&text);
        assert_eq!(back, s);
        // Junk lines are skipped.
        let junk = Store::parse(
            "partner zz 1\nwaiting 1\nbrought 1 2\nimport 12 1 x\nhello\n\
             partner 1 2 3 4 5 6 7 8 9 10 11 12 13 x\nmoves 1 2 3 4 5 6\nmoves 1 1 2 3 4 5 2\n\
             level 1 5\nlevel 1 x 0\n",
        );
        assert_eq!(junk, Store::default());
    }

    #[test]
    fn older_lines_still_load() {
        let s = Store::parse(
            "partner 00000000000000aa 25 13 13 84 0 45 0 12\n\
             waiting 2 00000000000000cd 400 3 7 1 2 0 300 42 90\n",
        );
        let l = s.partners[&0xAA];
        assert_eq!(l.partner.ability, 0);
        assert_eq!(l.partner.types, [13, 13]);
        assert_eq!(l.moves, [84, 45, 0, 0]);
        assert_eq!(
            s.waiting,
            vec![Waiting {
                garden: 2,
                key: 0xCD,
                partner: Partner {
                    ability: 0,
                    ..p(400)
                },
            }]
        );
        // Written back in the current form.
        let text = s.to_text();
        assert!(text.contains("partner 00000000000000aa 25 13 13 84 0 45 0 12 0 84 45 0 0 0 0\n"));
        assert!(text.contains("brought 2 00000000000000cd 400 3 7 1 2 0 300 42 0 0\n"));
        // The form before save marks.
        let older = Store::parse(
            "partner 00000000000000aa 25 13 13 84 0 45 0 12 7 84 45 0 0\n\
             brought 2 00000000000000cd 400 3 7 1 2 0 300 42 9\n",
        );
        assert!(!older.partners[&0xAA].partner.from_save);
        assert_eq!(older.partners[&0xAA].save_moves, 0);
        assert_eq!(older.partners[&0xAA].partner.ability, 7);
        assert!(!older.waiting[0].partner.from_save);
        assert_eq!(Store::parse(&text), s);
    }

    #[test]
    fn teaching_puts_new_moves_first_and_keeps_four() {
        assert_eq!(teach([0; 4], [5, 0, 6, 0]), [5, 6, 0, 0]);
        assert_eq!(teach([1, 2, 3, 0], [5, 6, 0, 0]), [5, 6, 1, 2]);
        assert_eq!(teach([1, 2, 3, 4], [5, 6, 7, 8]), [5, 6, 7, 8]);
        // A move it knew already moves up instead of taking two slots.
        assert_eq!(teach([1, 2, 3, 4], [3, 0, 0, 0]), [3, 1, 2, 4]);
        assert_eq!(teach([1, 2, 0, 0], [0; 4]), [1, 2, 0, 0]);
    }

    #[test]
    fn a_chao_without_a_moves_line_knows_its_partner_lines_moves() {
        let s = Store::parse("partner 00000000000000aa 25 13 13 84 0 45 0 12 0 84 45 7 0 1 5\n");
        let m = s.moves_of(0xAA);
        assert_eq!(m.active, [84, 45, 7, 0]);
        assert_eq!(m.known.len(), 3);
        assert!(m.only_from_saves(84) && m.only_from_saves(7) && !m.only_from_saves(45));
        assert_eq!(s.level(0xAA), Level::default());
        // Nothing is written for it until it changes.
        assert!(!s.to_text().contains("moves"));
        assert_eq!(s.moves_of(0xBB), Moves::default());
        assert_eq!(s.record(0xBB), None);
        assert!(s.record(0xAA).is_some());
        // Bad levels are kept in range.
        let s = Store::parse("level 1 0 5\nlevel 2 250 99999999\n");
        assert_eq!(s.level(1), Level::new(1, 5));
        assert_eq!(s.level(2), Level::new(100, 0));
    }

    #[test]
    fn tm_moves_from_an_older_progress_file_join_the_list() {
        let taught = BTreeMap::from([
            (0xAA, [0, 0, 9, 0]),
            (0xCC, [50, 0, 0, 0]),
            (0xDD, [7, 0, 0, 0]),
        ]);
        let s = Store::parse_with_taught(
            "partner 00000000000000aa 25 13 13 84 0 45 0 12 0 84 45 0 0 0 0\n\
             moves 00000000000000dd 3 0 0 0 3 0\n",
            &taught,
        );
        // In the TM's slot, as the progress file put it; the move there is still known.
        let m = s.moves_of(0xAA);
        assert_eq!(m.active, [84, 45, 9, 0]);
        assert!(m.knows(9) && !m.only_from_saves(9));
        assert_eq!(s.partners[&0xAA].moves, [84, 45, 9, 0]);
        // A TM-only Chao gets a list without a partner.
        assert_eq!(s.moves_of(0xCC).active, [50, 0, 0, 0]);
        assert!(!s.partners.contains_key(&0xCC));
        // A Chao with a list already keeps it.
        assert_eq!(s.moves_of(0xDD).active, [3, 0, 0, 0]);
        // Once written, the list is what loads.
        let again = Store::parse_with_taught(&s.to_text(), &taught);
        assert_eq!(again, s);
    }

    #[test]
    fn a_chao_keeps_every_move_it_learns() {
        let mut s = Store::default();
        let saved = Partner {
            from_save: true,
            moves: [5, 6, 0, 0],
            ..p(1)
        };
        let (l, new) = s.learn(77, saved);
        assert_eq!(new, [5, 6]);
        assert_eq!((l.moves, l.save_moves), ([5, 6, 0, 0], 0b0011));
        // Fewer than four active: the new ones fill in.
        let wild = Partner {
            moves: [7, 8, 9, 0],
            ..p(2)
        };
        let (l, new) = s.learn(77, wild);
        assert_eq!(new, [7, 8, 9]);
        assert_eq!((l.moves, l.save_moves), ([5, 6, 7, 8], 0b0011));
        let m = s.moves_of(77);
        assert_eq!(m.known.len(), 5);
        assert!(m.knows(9) && !m.is_active(9));
        // Four active: they stay; a move taught again by a wild one loses its save mark, and
        // one taught again by a save one keeps none.
        let (l, new) = s.learn(
            77,
            Partner {
                moves: [5, 0, 0, 0],
                ..p(3)
            },
        );
        assert!(new.is_empty());
        assert_eq!((l.moves, l.save_moves), ([5, 6, 7, 8], 0b0010));
        let (l, _) = s.learn(77, saved);
        assert_eq!(l.save_moves, 0b0010);
        assert_eq!(s.partners[&77].partner.species, 1);
    }

    #[test]
    fn choosing_the_active_moves() {
        let mut m = Moves::default();
        m.learn([1, 2, 3, 4], false);
        m.learn([5, 0, 0, 0], true);
        assert_eq!(m.active, [1, 2, 3, 4]);
        assert_eq!(m.toggle(5), Err(MoveRefused::Full));
        assert_eq!(m.toggle(99), Err(MoveRefused::Unknown));
        assert_eq!(m.toggle(2), Ok(false));
        assert_eq!(m.active, [1, 0, 3, 4]);
        assert_eq!(m.toggle(5), Ok(true));
        assert_eq!(m.active, [1, 5, 3, 4]);
        assert_eq!(m.active_marks(), 0b0010);
        for id in [1, 5, 3] {
            assert_eq!(m.toggle(id), Ok(false));
        }
        assert_eq!(m.toggle(4), Err(MoveRefused::LastOne));
        // A known move switched off stays off when it's taught again; a new one fills in.
        let mut m = Moves::default();
        m.learn([1, 2, 3, 4], false);
        m.toggle(4).unwrap();
        assert_eq!(m.learn([4, 7, 0, 0], false), [7]);
        assert_eq!(m.active, [1, 2, 3, 7]);
        // A TM move goes in place of the one picked, and is never only from a save.
        let mut m = Moves::default();
        m.learn([1, 2, 3, 4], true);
        assert!(m.teach_tm(9, 3));
        assert_eq!(m.active, [1, 2, 9, 4]);
        assert!(m.knows(3) && !m.only_from_saves(9));
        assert!(!m.teach_tm(1, 4));
        assert!(!m.only_from_saves(1));
        assert_eq!(m.active, [1, 2, 9, 4]);
        // In place of a move it doesn't battle with: an empty slot, else the last one.
        m.toggle(2).unwrap();
        m.teach_tm(10, 77);
        assert_eq!(m.active, [1, 10, 9, 4]);
        m.teach_tm(11, 0);
        assert_eq!(m.active, [1, 10, 9, 11]);
        // Kept in the store, the partner record follows.
        let mut s = Store::default();
        s.learn(
            5,
            Partner {
                moves: [1, 2, 0, 0],
                from_save: true,
                ..p(1)
            },
        );
        let mut m = s.moves_of(5);
        m.learn([3, 0, 0, 0], false);
        m.toggle(1).unwrap();
        m.active[3] = 444; // not known: left out
        s.set_moves(5, m);
        assert_eq!(s.partners[&5].moves, [0, 2, 3, 0]);
        assert_eq!(s.partners[&5].save_moves, 0b0010);
    }

    #[test]
    fn a_chao_keeps_every_type_and_ability_and_its_choice() {
        let mut s = Store::default();
        let saved = Partner {
            types: [3, 3],
            ability: 9,
            from_save: true,
            ..p(1)
        };
        s.learn(77, saved);
        let t = s.traits_of(77);
        assert_eq!(t.types, [(3, true)]);
        assert_eq!(t.abilities, [(9, true)]);
        // The first partner's are the active ones.
        assert_eq!((t.active_types, t.ability), ([3, 3], 9));
        // A later one adds to the lists without changing the choice; a type it had loses its
        // save mark when a wild one gives it again.
        let wild = Partner {
            types: [3, 7],
            ability: 12,
            ..p(2)
        };
        s.learn(77, wild);
        let mut t = s.traits_of(77);
        assert_eq!(t.types, [(3, false), (7, false)]);
        assert_eq!(t.abilities, [(9, true), (12, false)]);
        assert_eq!((t.active_types, t.ability), ([3, 3], 9));
        assert_eq!(s.partners[&77].partner.types, [3, 7]);
        // Choosing: a second type, then a third is refused; the last can't go off.
        assert_eq!(t.toggle_type(7), Ok(true));
        assert_eq!(t.active_types, [3, 7]);
        assert_eq!(t.active_type_list(), [3, 7]);
        assert_eq!(t.toggle_type(5), Err(TraitRefused::Unknown));
        assert_eq!(t.toggle_type(3), Ok(false));
        assert_eq!(t.active_types, [7, 7]);
        assert_eq!(t.toggle_type(7), Err(TraitRefused::LastType));
        assert_eq!(t.choose_ability(12), Ok(()));
        assert_eq!(t.choose_ability(13), Err(TraitRefused::Unknown));
        assert_eq!(t.choose_ability(0), Ok(()));
        assert_eq!(t.ability, 0);
        s.set_traits(77, t.clone());
        // No ability chosen stays none when another partner comes.
        s.learn(
            77,
            Partner {
                types: [1, 1],
                ability: 20,
                ..p(3)
            },
        );
        let after = s.traits_of(77);
        assert_eq!((after.active_types, after.ability), ([7, 7], 0));
        assert!(after.knows_type(1) && after.knows_ability(20));
        let mut three = Traits::default();
        for (types, ability) in [([1, 1], 4), ([2, 2], 5), ([3, 3], 6)] {
            three.learn(&Partner {
                types,
                ability,
                ..p(4)
            });
        }
        assert_eq!(three.toggle_type(2), Ok(true));
        assert_eq!(three.toggle_type(3), Err(TraitRefused::TwoTypes));
        // Kept and read back, with what doesn't fit left out.
        s.set_traits(
            88,
            Traits {
                types: vec![(4, false), (4, true), (6, true)],
                abilities: vec![(0, false), (30, true)],
                active_types: [6, 99],
                ability: 31,
            },
        );
        assert_eq!(s.traits[&88].types, [(4, false), (6, true)]);
        assert_eq!(s.traits[&88].active_types, [6, 6]);
        assert_eq!(s.traits[&88].ability, 0);
        let text = s.to_text();
        assert!(text.contains("types 0000000000000058 6 6 4 0 6 1\n"));
        assert!(text.contains("abilities 0000000000000058 0 30 1\n"));
        assert!(text.contains("types 000000000000004d 7 7 3 0 7 0 1 0\n"));
        assert_eq!(Store::parse(&text), s);
        assert!(s.record(88).is_some());
        assert!(s.identities().contains(&88));
    }

    #[test]
    fn older_files_have_the_partners_types_and_ability_active() {
        let s = Store::parse(
            "partner 00000000000000aa 25 13 10 84 0 45 0 12 9 84 45 0 0 1 0\n\
             partner 00000000000000bb 25 13 13 84 0 45 0 12\n",
        );
        let t = s.traits_of(0xAA);
        assert_eq!(t.types, [(13, true), (10, true)]);
        assert_eq!(t.abilities, [(9, true)]);
        assert_eq!((t.active_types, t.ability), ([13, 10], 9));
        // No ability recorded: none here (the battle profile looks it up in the ROM).
        let t = s.traits_of(0xBB);
        assert_eq!(
            (t.types.len(), t.active_types, t.abilities.len()),
            (1, [13, 13], 0)
        );
        assert_eq!(s.traits_of(0xCC), Traits::default());
        // Nothing is written for them until they change.
        assert!(!s.to_text().contains("types"));
        // A file with only one of the two lines takes the other from the partner line.
        let s = Store::parse(
            "partner 00000000000000aa 25 13 10 84 0 45 0 12 9 84 45 0 0 0 0\n\
             types 00000000000000aa 10 10 13 0 10 0 2 0\n\
             abilities 00000000000000dd 0\n\
             types 1 2\nabilities 1 x 3 1\n",
        );
        let t = s.traits_of(0xAA);
        assert_eq!(t.types.len(), 3);
        assert_eq!(
            (t.active_types, t.abilities.clone(), t.ability),
            ([10, 10], vec![(9, false)], 9)
        );
        assert_eq!(s.traits_of(0xDD), Traits::default());
        assert_eq!(s.traits.len(), 2);
    }

    #[test]
    fn once_a_day_per_garden_and_four_at_most() {
        let mut s = Store::default();
        let today = 20_100;
        s.import(1, 1, today, p(1)).unwrap();
        assert_eq!(s.import(1, 1, today, p(1)), Err(Refused::Here));
        // Another garden is fine.
        s.import(1, 2, today, p(1)).unwrap();
        // Absorbed: gone from the garden, but not again today.
        let (learned, new) = s.absorbed(1, 1, 77).unwrap();
        assert_eq!(learned.partner, p(1));
        assert_eq!(learned.moves, [1, 2, 300, 0]);
        assert_eq!(new, [1, 2, 300]);
        assert_eq!(s.partners.get(&77), Some(&learned));
        assert_eq!(s.can_import(1, 1, today), Err(Refused::Today));
        assert_eq!(s.can_import(1, 1, today + 1), Ok(()));
        let other = Partner {
            moves: [0, 0, 50, 0],
            ..p(2)
        };
        for key in 10..13 {
            s.import(key, 1, today, other).unwrap();
        }
        s.import(13, 1, today, other).unwrap();
        assert_eq!(s.waiting_in(1).count(), PER_GARDEN);
        assert_eq!(s.import(14, 1, today, other), Err(Refused::Full));
        s.forget_old(today + 1);
        assert!(s.imports.is_empty());
        // A later absorb replaces the partner and its moves join the ones known.
        let (learned, _) = s.absorbed(1, 10, 77).unwrap();
        assert_eq!(s.partners.len(), 1);
        assert_eq!(learned.partner.species, 2);
        assert_eq!(learned.moves, [1, 2, 300, 50]);
        assert_eq!(s.absorbed(1, 999, 77), None);
    }

    #[test]
    fn a_visitor_teaches_without_waiting() {
        let mut s = Store::default();
        s.import(1, 1, 20_100, p(1)).unwrap();
        let (learned, _) = s.learn(77, p(2));
        assert_eq!(learned.partner, p(2));
        assert_eq!(learned.moves, [1, 2, 300, 0]);
        let wild = Partner {
            moves: [7, 0, 0, 0],
            ..p(3)
        };
        assert_eq!(s.learn(77, wild).0.moves, [1, 2, 300, 7]);
        // The waiting partner and the day's imports are untouched.
        assert_eq!(s.waiting.len(), 1);
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.partners[&77].partner, wild);
    }

    #[test]
    fn a_partner_used_elsewhere_waits_no_more() {
        let mut s = Store::default();
        s.import(1, 1, 20_100, p(1)).unwrap();
        s.import(1, 2, 20_100, p(1)).unwrap();
        assert!(!s.drop_waiting(3, 1));
        assert!(s.drop_waiting(1, 1));
        assert!(!s.drop_waiting(1, 1));
        // Only that garden's; nothing learned, and not again today.
        assert_eq!(s.waiting_in(2).count(), 1);
        assert!(s.partners.is_empty());
        assert_eq!(s.can_import(1, 1, 20_100), Err(Refused::Today));
    }
}

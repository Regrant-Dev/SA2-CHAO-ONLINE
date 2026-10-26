//! What creature link keeps in `<mod>/creature/partners.txt`: the line `HEADER` first, then one
//! record a line:
//!
//! - `partner <chao identity> <species> <type> <type> <move> <move> <move> <move> <level>
//!   <ability> <active move> <active move> <active move> <active move>`: the partner a Chao
//!   absorbed last and the four moves the Chao battles with, by its `ChaoData::identity` (the
//!   SA2 save is never changed for this).
//! - `form <chao identity> <form>`: that partner's form (0 without the line).
//! - `moves <chao identity> <active move> <active move> <active move> <active move> [<move>]...`:
//!   every move the Chao has learned (from partners, wild visitors, the gift and milestone moves),
//!   oldest first, and the four it battles with (0: an empty slot). A Chao with a `partner` line
//!   and no `moves` line knows the partner line's active moves.
//! - `types <chao identity> <active type> <active type> [<type>]...`: every type the Chao got
//!   from partners, oldest first, and the one or two it battles as (the same twice for one, 255
//!   twice for none).
//! - `abilities <chao identity> <active ability> [<ability>]...`: the same for abilities, with
//!   the one it battles with (0: none). A Chao with a `partner` line and no `types` or
//!   `abilities` line has its partner line's types and ability, active.
//! - `level <chao identity> <level> <xp>`: its battle level (1..=100) and the XP it has toward
//!   the next (`battle::levels`). A Chao without one is at level 5 with none.
//! - `import <key> <garden> <day>`: a creature brought into a garden, by its key, and the day
//!   (Unix days); each comes to each garden once a day.
//! - `brought <garden> <key> <species> <type> <type> <move>... <level> <ability>`: brought and
//!   not absorbed yet; it's there each time the garden loads. `brought_form <key> <form>`: its
//!   form (0 without the line).
//! - `kept <key> <species> <level> <move> <move> <move> <move>`: a creature the mod keeps (the
//!   first-partner gift).
//! - `gift <species>`: the gift was taken (once only).
//!
//! Numbers are decimal except the identity and key (hex). Ability 0 is "not recorded": the
//! battle profile takes the species' first ability from the catalog then (`profile`). A file
//! that doesn't start with `HEADER` isn't read.

use std::collections::BTreeMap;

use crate::battle::levels::Level;

/// The first line of a partner file.
pub const HEADER: &str = "creature-partners 1";

/// Most partners waiting in one garden at once.
pub const PER_GARDEN: usize = 4;

/// A creature as creature link keeps it: ids from the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partner {
    pub form: u8,
    pub species: u16,
    pub types: [u8; 2],
    pub moves: [u16; 4],
    pub level: u8,
    /// Its ability id; 0 when not recorded.
    pub ability: u8,
}

/// What a Chao has from absorbing: its last partner and the four moves it battles with (0 for
/// an empty slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Learned {
    pub partner: Partner,
    pub moves: [u16; 4],
}

/// Every move a Chao has learned, and the four it battles with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moves {
    /// Each move it learned, oldest first.
    pub known: Vec<u16>,
    /// The ones it battles with, each one it knows (0: an empty slot).
    pub active: [u16; 4],
}

/// Why a move can't be switched on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveRefused {
    /// Creature battles can't use it (it can still be switched off).
    Unsupported,
    /// The Chao doesn't know it.
    Unknown,
    /// It's the only one it battles with.
    LastOne,
    /// It battles with four already.
    Full,
    /// The catalog isn't loaded, so whether battles can use it isn't known.
    NotReady,
}

impl MoveRefused {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unsupported => "Creature battles can't use that move",
            Self::Unknown => "It doesn't know that move",
            Self::LastOne => "A Chao needs at least one move",
            Self::Full => "4 moves already: turn one off first",
            Self::NotReady => "The creature link isn't ready yet",
        }
    }
}

impl Moves {
    /// What a Chao known only by its partner record knows: the record's moves, all of them
    /// active.
    pub fn from_learned(l: &Learned) -> Moves {
        let known = l
            .moves
            .iter()
            .rev()
            .copied()
            .filter(|&id| id != 0)
            .collect();
        Moves {
            known,
            active: l.moves,
        }
        .tidy()
    }

    pub fn knows(&self, id: u16) -> bool {
        id != 0 && self.known.contains(&id)
    }

    pub fn is_active(&self, id: u16) -> bool {
        id != 0 && self.active.contains(&id)
    }

    /// Adds `id` to the known moves. Whether it's new.
    fn add(&mut self, id: u16) -> bool {
        if id == 0 || self.known.contains(&id) {
            return false;
        }
        self.known.push(id);
        true
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

    /// A partner teaches `taught`: the new ones join the known moves and fill empty active
    /// slots; the active ones stay otherwise, and a known move switched off stays off. The
    /// moves new to it.
    pub fn learn(&mut self, taught: [u16; 4]) -> Vec<u16> {
        let mut new = Vec::new();
        for id in taught {
            if self.add(id) {
                new.push(id);
                self.fill(id);
            }
        }
        new
    }

    /// A milestone move `id` is taught in place of active move `replacing` (or into an empty slot, or
    /// the last one): it's known and active. Whether it's new.
    pub fn teach_milestone(&mut self, id: u16, replacing: u16) -> bool {
        if id == 0 {
            return false;
        }
        let new = self.add(id);
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

    /// Switches off the active moves `usable` refuses (they stay known), even the last one.
    /// The ones switched off.
    pub fn drop_unusable(&mut self, usable: impl Fn(u16) -> bool) -> Vec<u16> {
        let mut dropped = Vec::new();
        for slot in &mut self.active {
            if *slot != 0 && !usable(*slot) {
                dropped.push(*slot);
                *slot = 0;
            }
        }
        dropped
    }

    /// Without empty or repeated known moves, and active ones it doesn't know or has twice.
    fn tidy(mut self) -> Moves {
        let mut seen = Vec::new();
        self.known.retain(|&id| {
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
    /// Each type it got, oldest first.
    pub types: Vec<u8>,
    /// Each ability it got (never 0), oldest first.
    pub abilities: Vec<u8>,
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
    Unsupported,
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
            Self::Unsupported => "Creature battles can't use that ability",
            Self::Unknown => "It never got that from a partner",
            Self::LastType => "A Chao needs at least one type",
            Self::TwoTypes => "2 types already: turn one off first",
        }
    }
}

/// Adds `id` to `list` when it isn't there. Whether it's new.
fn add_new<T: PartialEq + Copy>(list: &mut Vec<T>, id: T) -> bool {
    if list.contains(&id) {
        return false;
    }
    list.push(id);
    true
}

impl Traits {
    /// What a Chao known only by its partner record has: the partner's types and ability,
    /// active. None for no partner (species 0).
    pub fn from_partner(p: &Partner) -> Traits {
        let mut t = Traits::default();
        t.learn(p);
        t
    }

    /// The types `types` (the same twice for one) and `ability` (0: none), active, as another
    /// player's game sends them.
    pub fn chosen(types: [u8; 2], ability: u8) -> Traits {
        Traits::from_partner(&Partner {
            form: 0,
            species: 1,
            types,
            moves: [0; 4],
            level: 0,
            ability,
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
                add_new(&mut self.types, t);
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
            add_new(&mut self.abilities, p.ability);
            if !had {
                self.ability = p.ability;
            }
        }
    }

    pub fn knows_type(&self, t: u8) -> bool {
        t != NO_TYPE && self.types.contains(&t)
    }

    pub fn knows_ability(&self, a: u8) -> bool {
        a != 0 && self.abilities.contains(&a)
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
        self.types.retain(|&t| {
            let keep = t != NO_TYPE && !seen.contains(&t);
            seen.push(t);
            keep
        });
        let mut seen = Vec::new();
        self.abilities.retain(|&a| {
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

impl Store {
    /// The store in `text` (nothing for an empty file); an error for one that doesn't start
    /// with `HEADER`. Lines that don't fit are skipped.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
        match lines.next().map(str::trim) {
            None => return Ok(Self::default()),
            Some(HEADER) => {}
            Some(_) => return Err(format!("not a \"{HEADER}\" partner file")),
        }
        let mut s = Self::default();
        // Chao identity -> (a `types` line was read, an `abilities` line was).
        let mut parts: BTreeMap<u64, (bool, bool)> = BTreeMap::new();
        let mut forms = BTreeMap::new();
        let mut waiting_forms = BTreeMap::new();
        for line in lines {
            let mut w = line.split_whitespace();
            let kind = w.next();
            let rest: Vec<&str> = w.collect();
            match kind {
                Some("partner") if rest.len() == 14 => {
                    if let (Ok(id), Some(partner), Ok(ability), Some(moves)) = (
                        u64::from_str_radix(rest[0], 16),
                        partner(&rest[1..9]),
                        rest[9].parse(),
                        moves(&rest[10..14]),
                    ) {
                        s.partners.insert(
                            id,
                            Learned {
                                partner: Partner { ability, ..partner },
                                moves,
                            },
                        );
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
                Some("brought") if rest.len() == 11 => {
                    if let (Ok(garden), Ok(key), Some(partner), Ok(ability)) = (
                        rest[0].parse(),
                        u64::from_str_radix(rest[1], 16),
                        partner(&rest[2..10]),
                        rest[10].parse(),
                    ) {
                        s.waiting.push(Waiting {
                            garden,
                            key,
                            partner: Partner { ability, ..partner },
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
                Some("moves") if rest.len() >= 5 => {
                    let known: Option<Vec<u16>> =
                        rest[5..].iter().map(|v| v.parse().ok()).collect();
                    if let (Ok(id), Some(active), Some(known)) =
                        (u64::from_str_radix(rest[0], 16), moves(&rest[1..5]), known)
                    {
                        s.moves.insert(id, Moves { known, active }.tidy());
                    }
                }
                Some("types") if rest.len() >= 3 => {
                    let known: Option<Vec<u8>> = rest[3..].iter().map(|v| v.parse().ok()).collect();
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
                Some("abilities") if rest.len() >= 2 => {
                    let known: Option<Vec<u8>> = rest[2..].iter().map(|v| v.parse().ok()).collect();
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
                Some(k @ ("form" | "brought_form")) if rest.len() == 2 => {
                    if let (Ok(id), Ok(form)) =
                        (u64::from_str_radix(rest[0], 16), rest[1].parse::<u8>())
                        && form <= 31
                    {
                        if k == "form" {
                            forms.insert(id, form);
                        } else {
                            waiting_forms.insert(id, form);
                        }
                    }
                }
                Some("gift") if rest.len() == 1 => s.gift = rest[0].parse().ok(),
                _ => {}
            }
        }
        for (id, form) in forms {
            if let Some(learned) = s.partners.get_mut(&id) {
                learned.partner.form = form;
            }
        }
        for waiting in &mut s.waiting {
            if let Some(&form) = waiting_forms.get(&waiting.key) {
                waiting.partner.form = form;
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
        Ok(s)
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
        let mut out = format!("{HEADER}\n");
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
            if l.partner.form != 0 {
                out += &format!("form {id:016x} {}\n", l.partner.form);
            }
            let m = l.moves;
            out += &format!(
                "partner {id:016x} {} {} {} {} {} {}\n",
                fields(&l.partner),
                l.partner.ability,
                m[0],
                m[1],
                m[2],
                m[3],
            );
        }
        for (id, m) in &self.moves {
            let a = m.active;
            out += &format!("moves {id:016x} {} {} {} {}", a[0], a[1], a[2], a[3]);
            for &move_id in &m.known {
                out += &format!(" {move_id}");
            }
            out += "\n";
        }
        for (id, t) in &self.traits {
            let [a, b] = t.active_types;
            out += &format!("types {id:016x} {a} {b}");
            for &ty in &t.types {
                out += &format!(" {ty}");
            }
            out += &format!("\nabilities {id:016x} {}", t.ability);
            for &ability in &t.abilities {
                out += &format!(" {ability}");
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
            if w.partner.form != 0 {
                out += &format!("brought_form {:016x} {}\n", w.key, w.partner.form);
            }
            out += &format!(
                "brought {} {:016x} {} {}\n",
                w.garden,
                w.key,
                fields(&w.partner),
                w.partner.ability,
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
            partner: Partner { ..partner },
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
        let partner = Partner { ..partner };
        let mut t = self.traits_of(identity);
        t.learn(&partner);
        self.traits.insert(identity, t);
        let mut m = self.moves_of(identity);
        let new = m.learn(partner.moves);
        let learned = Learned {
            partner,
            moves: m.active,
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
        }
        self.moves.insert(identity, moves);
    }

    /// Switches off every Chao's active moves `usable` refuses (`Moves::drop_unusable`). How
    /// many were.
    pub fn drop_unusable_moves(&mut self, usable: impl Fn(u16) -> bool) -> usize {
        let mut count = 0;
        for identity in self.identities() {
            let mut m = self.moves_of(identity);
            let dropped = m.drop_unusable(&usable).len();
            if dropped > 0 {
                count += dropped;
                self.set_moves(identity, m);
            }
        }
        count
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
        form: 0,
        species: f.first()?.parse().ok()?,
        types: [b(1)?, b(2)?],
        moves: moves(f.get(3..7)?)?,
        level: b(7)?,
        ability: 0,
    })
}

fn moves(f: &[&str]) -> Option<[u16; 4]> {
    let n = |i: usize| f.get(i).and_then(|v| v.parse::<u16>().ok());
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Store {
        Store::parse(&format!("{HEADER}\n{text}")).unwrap()
    }

    #[test]
    fn canonical_forms_survive_partner_and_waiting_progress_roundtrips() {
        let mut store = Store::default();
        let partner = Partner {
            species: 26,
            form: 1,
            ..p(26)
        };
        store.learn(0x123, partner);
        store.import(0x456, 1, 100, partner).unwrap();
        let text = store.to_text();
        let back = Store::parse(&text).unwrap();
        assert_eq!(back.partners[&0x123].partner.form, 1);
        assert_eq!(back.waiting[0].partner.form, 1);
        assert_eq!(back.partners[&0x123].partner.species, 26);
    }

    fn p(species: u16) -> Partner {
        Partner {
            form: 0,
            species,
            types: [3, 7],
            moves: [1, 2, 0, 300],
            level: 42,
            ability: 9,
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
            },
        );
        s.partners.insert(
            0x77,
            Learned {
                partner: p(6),
                moves: [1, 2, 3, 4],
            },
        );
        s.import(0xAB, 2, 20_000, p(9)).unwrap();
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
                known: vec![9, 1, 2, 3, 4],
                active: [1, 2, 3, 4],
            },
        );
        // A Chao with milestone moves and no partner, and levels.
        s.set_moves(
            0x88,
            Moves {
                known: vec![40],
                active: [0, 40, 0, 0],
            },
        );
        s.levels.insert(0x77, Level::new(23, 150));
        s.levels.insert(0x99, Level::new(100, 0));
        let text = s.to_text();
        assert!(text.starts_with("creature-partners 1\n"));
        assert!(text.contains("partner 0000000000000077 6 3 7 1 2 0 300 42 9 1 2 3 4\n"));
        assert!(text.contains("moves 0000000000000077 1 2 3 4 9 1 2 3 4\n"));
        assert!(text.contains("level 0000000000000077 23 150\n"));
        assert!(text.contains("brought 3 00000000000000cd 400 3 7 1 2 0 300 42 0\n"));
        let back = Store::parse(&text).unwrap();
        assert_eq!(back, s);
        // Junk lines are skipped.
        let junk = parse(
            "partner zz 1\nwaiting 1\nbrought 1 2\nimport 12 1 x\nhello\n\
             partner 1 2 3 4 5 6 7 8 9 10 11 12 13 x\nmoves 1 2 3 4 5 x\nmoves 1 1 2 3\n\
             level 1 5\nlevel 1 x 0\n",
        );
        assert_eq!(junk, Store::default());
    }

    #[test]
    fn only_a_partner_file_with_the_header_is_read() {
        assert_eq!(Store::parse("").unwrap(), Store::default());
        assert_eq!(Store::parse("\n  \n").unwrap(), Store::default());
        // A byte order mark and blank lines from an editor.
        let s = Store::parse(&format!("\u{feff}\r\n{HEADER}\r\nlevel 1 9 0\r\n")).unwrap();
        assert_eq!(s.level(1).level, 9);
        for other in [
            "level 1 9 0\n",
            "partner 00000000000000aa 25 13 13 84 0 45 0 12\n",
            "creature-partners 2\nlevel 1 9 0\n",
        ] {
            assert!(Store::parse(other).is_err(), "{other}");
        }
    }

    #[test]
    fn a_chao_without_a_moves_line_knows_its_partner_lines_moves() {
        let s = parse("partner 00000000000000aa 25 13 13 84 0 45 0 12 0 84 45 7 0\n");
        let m = s.moves_of(0xAA);
        assert_eq!(m.active, [84, 45, 7, 0]);
        assert_eq!(m.known.len(), 3);
        assert_eq!(s.level(0xAA), Level::default());
        // Nothing is written for it until it changes.
        assert!(!s.to_text().contains("moves"));
        assert_eq!(s.moves_of(0xBB), Moves::default());
        assert_eq!(s.record(0xBB), None);
        assert!(s.record(0xAA).is_some());
        // Bad levels are kept in range.
        let s = parse("level 1 0 5\nlevel 2 250 99999999\n");
        assert_eq!(s.level(1), Level::new(1, 5));
        assert_eq!(s.level(2), Level::new(100, 0));
    }

    #[test]
    fn a_chao_keeps_every_move_it_learns() {
        let mut s = Store::default();
        let first = Partner {
            moves: [5, 6, 0, 0],
            ..p(1)
        };
        let (l, new) = s.learn(77, first);
        assert_eq!(new, [5, 6]);
        assert_eq!(l.moves, [5, 6, 0, 0]);
        // Fewer than four active: the new ones fill in.
        let wild = Partner {
            moves: [7, 8, 9, 0],
            ..p(2)
        };
        let (l, new) = s.learn(77, wild);
        assert_eq!(new, [7, 8, 9]);
        assert_eq!(l.moves, [5, 6, 7, 8]);
        let m = s.moves_of(77);
        assert_eq!(m.known.len(), 5);
        assert!(m.knows(9) && !m.is_active(9));
        // Four active: they stay.
        let (l, new) = s.learn(
            77,
            Partner {
                moves: [5, 0, 0, 0],
                ..p(3)
            },
        );
        assert!(new.is_empty());
        assert_eq!(l.moves, [5, 6, 7, 8]);
        assert_eq!(s.partners[&77].partner.species, 3);
    }

    #[test]
    fn choosing_the_active_moves() {
        let mut m = Moves::default();
        m.learn([1, 2, 3, 4]);
        m.learn([5, 0, 0, 0]);
        assert_eq!(m.active, [1, 2, 3, 4]);
        assert_eq!(m.toggle(5), Err(MoveRefused::Full));
        assert_eq!(m.toggle(99), Err(MoveRefused::Unknown));
        assert_eq!(m.toggle(2), Ok(false));
        assert_eq!(m.active, [1, 0, 3, 4]);
        assert_eq!(m.toggle(5), Ok(true));
        assert_eq!(m.active, [1, 5, 3, 4]);
        for id in [1, 5, 3] {
            assert_eq!(m.toggle(id), Ok(false));
        }
        assert_eq!(m.toggle(4), Err(MoveRefused::LastOne));
        // A known move switched off stays off when it's taught again; a new one fills in.
        let mut m = Moves::default();
        m.learn([1, 2, 3, 4]);
        m.toggle(4).unwrap();
        assert_eq!(m.learn([4, 7, 0, 0]), [7]);
        assert_eq!(m.active, [1, 2, 3, 7]);
        // A milestone move goes in place of the one picked.
        let mut m = Moves::default();
        m.learn([1, 2, 3, 4]);
        assert!(m.teach_milestone(9, 3));
        assert_eq!(m.active, [1, 2, 9, 4]);
        assert!(m.knows(3));
        assert!(!m.teach_milestone(1, 4));
        assert_eq!(m.active, [1, 2, 9, 4]);
        // In place of a move it doesn't battle with: an empty slot, else the last one.
        m.toggle(2).unwrap();
        m.teach_milestone(10, 77);
        assert_eq!(m.active, [1, 10, 9, 4]);
        m.teach_milestone(11, 0);
        assert_eq!(m.active, [1, 10, 9, 11]);
        // Kept in the store, the partner record follows.
        let mut s = Store::default();
        s.learn(
            5,
            Partner {
                moves: [1, 2, 0, 0],
                ..p(1)
            },
        );
        let mut m = s.moves_of(5);
        m.learn([3, 0, 0, 0]);
        m.toggle(1).unwrap();
        m.active[3] = 444; // not known: left out
        s.set_moves(5, m);
        assert_eq!(s.partners[&5].moves, [0, 2, 3, 0]);
    }

    #[test]
    fn unusable_active_moves_are_switched_off_but_stay_known() {
        let mut s = Store::default();
        s.learn(
            5,
            Partner {
                moves: [1, 2, 3, 0],
                ..p(1)
            },
        );
        s.set_moves(
            6,
            Moves {
                known: vec![2],
                active: [2, 0, 0, 0],
            },
        );
        assert_eq!(s.drop_unusable_moves(|id| id != 2), 2);
        assert_eq!(s.moves_of(5).active, [1, 0, 3, 0]);
        assert_eq!(s.partners[&5].moves, [1, 0, 3, 0]);
        assert!(s.moves_of(5).knows(2));
        // Even a Chao's last one.
        assert_eq!(s.moves_of(6).active, [0; 4]);
        assert_eq!(s.drop_unusable_moves(|id| id != 2), 0);
    }

    #[test]
    fn a_chao_keeps_every_type_and_ability_and_its_choice() {
        let mut s = Store::default();
        let first = Partner {
            types: [3, 3],
            ability: 9,
            ..p(1)
        };
        s.learn(77, first);
        let t = s.traits_of(77);
        assert_eq!(t.types, [3]);
        assert_eq!(t.abilities, [9]);
        // The first partner's are the active ones.
        assert_eq!((t.active_types, t.ability), ([3, 3], 9));
        // A later one adds to the lists without changing the choice.
        let wild = Partner {
            types: [3, 7],
            ability: 12,
            ..p(2)
        };
        s.learn(77, wild);
        let mut t = s.traits_of(77);
        assert_eq!(t.types, [3, 7]);
        assert_eq!(t.abilities, [9, 12]);
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
                types: vec![4, 4, 6],
                abilities: vec![0, 30],
                active_types: [6, 99],
                ability: 31,
            },
        );
        assert_eq!(s.traits[&88].types, [4, 6]);
        assert_eq!(s.traits[&88].active_types, [6, 6]);
        assert_eq!(s.traits[&88].ability, 0);
        let text = s.to_text();
        assert!(text.contains("types 0000000000000058 6 6 4 6\n"));
        assert!(text.contains("abilities 0000000000000058 0 30\n"));
        assert!(text.contains("types 000000000000004d 7 7 3 7 1\n"));
        assert_eq!(Store::parse(&text).unwrap(), s);
        assert!(s.record(88).is_some());
        assert!(s.identities().contains(&88));
    }

    #[test]
    fn a_partner_line_alone_gives_its_types_and_ability_active() {
        let s = parse(
            "partner 00000000000000aa 25 13 10 84 0 45 0 12 9 84 45 0 0\n\
             partner 00000000000000bb 25 13 13 84 0 45 0 12 0 84 45 0 0\n",
        );
        let t = s.traits_of(0xAA);
        assert_eq!(t.types, [13, 10]);
        assert_eq!(t.abilities, [9]);
        assert_eq!((t.active_types, t.ability), ([13, 10], 9));
        // No ability recorded: none here (the battle profile looks it up in the catalog).
        let t = s.traits_of(0xBB);
        assert_eq!(
            (t.types.len(), t.active_types, t.abilities.len()),
            (1, [13, 13], 0)
        );
        assert_eq!(s.traits_of(0xCC), Traits::default());
        // Nothing is written for them until they change.
        assert!(!s.to_text().contains("types"));
        // A file with only one of the two lines takes the other from the partner line.
        let s = parse(
            "partner 00000000000000aa 25 13 10 84 0 45 0 12 9 84 45 0 0\n\
             types 00000000000000aa 10 10 13 10 2\n\
             abilities 00000000000000dd 0\n\
             types 1 2\nabilities 1 x 3\n",
        );
        let t = s.traits_of(0xAA);
        assert_eq!(t.types.len(), 3);
        assert_eq!(
            (t.active_types, t.abilities.clone(), t.ability),
            ([10, 10], vec![9], 9)
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

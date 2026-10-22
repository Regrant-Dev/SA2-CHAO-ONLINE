//! What GBA link keeps in `<mod>/gba/partners.txt`, one record a line:
//!
//! - `partner <chao identity> <species> <type> <type> <move> <move> <move> <move> <level>`: the
//!   partner a Chao absorbed last, by its `ChaoData::identity` (the SA2 save is never changed
//!   for this).
//! - `import <key> <garden> <day>`: a creature brought into a garden, by `save::Mon::key`, and
//!   the day (Unix days); each comes to each garden once a day.
//! - `waiting <garden> <key> <species> <type> <type> <move>... <level> <speed>`: brought and not
//!   absorbed yet; it's there each time the garden loads.
//!
//! - `kept <key> <species> <level> <move> <move> <move> <move>`: a creature the mod keeps (the
//!   first-partner gift), offered like the GBA save's.
//! - `gift <species>`: the gift was taken (once only).
//!
//! Numbers are decimal except the identity and key (hex).

use std::collections::BTreeMap;

/// Most partners waiting in one garden at once.
pub const PER_GARDEN: usize = 4;

/// A creature as GBA link keeps it: ids from the save, `speed` its base speed from the ROM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partner {
    pub species: u16,
    pub types: [u8; 2],
    pub moves: [u16; 4],
    pub level: u8,
    pub speed: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub garden: u8,
    pub key: u64,
    pub partner: Partner,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Store {
    /// Chao identity -> its partner.
    pub partners: BTreeMap<u64, Partner>,
    /// (key, garden, day).
    pub imports: Vec<(u64, u8, u32)>,
    pub waiting: Vec<Waiting>,
    /// Creatures the mod keeps: (key, species, level, moves).
    pub kept: Vec<(u64, u16, u8, [u16; 4])>,
    /// The species taken as the gift, once it's been taken.
    pub gift: Option<u16>,
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
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let mut w = line.split_whitespace();
            let kind = w.next();
            let rest: Vec<&str> = w.collect();
            match kind {
                Some("partner") if rest.len() == 9 => {
                    if let (Ok(id), Some(p)) =
                        (u64::from_str_radix(rest[0], 16), partner(&rest[1..], false))
                    {
                        s.partners.insert(id, p);
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
                Some("waiting") if rest.len() == 11 => {
                    if let (Ok(garden), Ok(key), Some(partner)) = (
                        rest[0].parse(),
                        u64::from_str_radix(rest[1], 16),
                        partner(&rest[2..], true),
                    ) {
                        s.waiting.push(Waiting {
                            garden,
                            key,
                            partner,
                        });
                    }
                }
                Some("kept") if rest.len() == 7 => {
                    let n = |i: usize| rest[i].parse::<u16>().ok();
                    if let (Ok(key), Some(species), Ok(level), Some(a), Some(b), Some(c), Some(d)) = (
                        u64::from_str_radix(rest[0], 16),
                        n(1),
                        rest[2].parse::<u8>(),
                        n(3),
                        n(4),
                        n(5),
                        n(6),
                    ) {
                        s.kept.push((key, species, level, [a, b, c, d]));
                    }
                }
                Some("gift") if rest.len() == 1 => s.gift = rest[0].parse().ok(),
                _ => {}
            }
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
        for (id, p) in &self.partners {
            out += &format!("partner {id:016x} {}\n", fields(p));
        }
        for (key, garden, day) in &self.imports {
            out += &format!("import {key:016x} {garden} {day}\n");
        }
        for w in &self.waiting {
            out += &format!(
                "waiting {} {:016x} {} {}\n",
                w.garden,
                w.key,
                fields(&w.partner),
                w.partner.speed
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
    /// garden and is that Chao's partner now (replacing any before). Returns it.
    pub fn absorbed(&mut self, garden: u8, key: u64, identity: u64) -> Option<Partner> {
        let at = self
            .waiting
            .iter()
            .position(|w| w.garden == garden && w.key == key)?;
        let partner = self.waiting.remove(at).partner;
        self.partners.insert(identity, partner);
        Some(partner)
    }

    pub fn waiting_in(&self, garden: u8) -> impl Iterator<Item = &Waiting> {
        self.waiting.iter().filter(move |w| w.garden == garden)
    }
}

fn partner(f: &[&str], with_speed: bool) -> Option<Partner> {
    let n = |i: usize| f.get(i).and_then(|v| v.parse::<u16>().ok());
    let b = |i: usize| f.get(i).and_then(|v| v.parse::<u8>().ok());
    Some(Partner {
        species: n(0)?,
        types: [b(1)?, b(2)?],
        moves: [n(3)?, n(4)?, n(5)?, n(6)?],
        level: b(7)?,
        speed: if with_speed { b(8)? } else { 0 },
    })
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
            speed: 90,
        }
    }

    #[test]
    fn round_trips_through_text() {
        let mut s = Store::default();
        s.partners
            .insert(0xDEAD_BEEF_0123_4567, Partner { speed: 0, ..p(5) });
        s.import(0xAB, 2, 20_000, p(9)).unwrap();
        s.import(0xCD, 3, 20_000, p(400)).unwrap();
        assert!(s.take_gift(0x55, 7, 5, [1, 2, 0, 0]));
        assert!(!s.take_gift(0x56, 8, 5, [1, 0, 0, 0]));
        assert_eq!(s.kept.len(), 1);
        let back = Store::parse(&s.to_text());
        assert_eq!(back, s);
        // Junk lines are skipped.
        let junk = Store::parse("partner zz 1\nwaiting 1\nimport 12 1 x\nhello\n");
        assert_eq!(junk, Store::default());
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
        assert_eq!(s.absorbed(1, 1, 77), Some(p(1)));
        assert_eq!(s.partners.get(&77), Some(&p(1)));
        assert_eq!(s.can_import(1, 1, today), Err(Refused::Today));
        assert_eq!(s.can_import(1, 1, today + 1), Ok(()));
        for key in 10..13 {
            s.import(key, 1, today, p(2)).unwrap();
        }
        s.import(13, 1, today, p(2)).unwrap();
        assert_eq!(s.waiting_in(1).count(), PER_GARDEN);
        assert_eq!(s.import(14, 1, today, p(2)), Err(Refused::Full));
        s.forget_old(today + 1);
        assert!(s.imports.is_empty());
        // A later absorb replaces the partner.
        s.absorbed(1, 10, 77);
        assert_eq!(s.partners.len(), 1);
        assert_eq!(s.absorbed(1, 999, 77), None);
    }
}

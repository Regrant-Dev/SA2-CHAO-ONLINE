//! What GBA link keeps in `<mod>/gba/partners.txt`, one record a line:
//!
//! - `partner <chao identity> <species> <type> <type> <move> <move> <move> <move> <level>
//!   <ability> <known move> <known move> <known move> <known move> <from a save> <save moves>`:
//!   the partner a Chao absorbed last and the four moves the Chao knows from its partners, by its
//!   `ChaoData::identity` (the SA2 save is never changed for this). `<from a save>` is 1 when that
//!   partner came from a GBA save, and `<save moves>` has bit `i` set when known move `i` was
//!   taught by one. Lines without the last two fields load with neither; lines without the last
//!   seven also load with no ability recorded and the partner's moves as the known ones.
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

/// What a Chao has from absorbing: its last partner and the four moves it knows (0 for an empty
/// slot), the newest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Learned {
    pub partner: Partner,
    pub moves: [u16; 4],
    /// Bit `i`: `moves[i]` was taught by a partner from a GBA save.
    pub save_moves: u8,
}

impl Learned {
    /// The save marks (as `save_moves`) of `moves`: those it has with a mark here.
    pub fn marks_for(&self, moves: [u16; 4]) -> u8 {
        let mut marks = 0;
        for (i, id) in moves.into_iter().enumerate() {
            let marked = self
                .moves
                .iter()
                .position(|&m| m == id && id != 0)
                .is_some_and(|at| self.save_moves & 1 << at != 0);
            if marked {
                marks |= 1 << i;
            }
        }
        marks
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

/// The moves a Chao knows after a partner with `taught` moves: those first, then the ones it
/// knew that still fit, four at most, no repeats and no empty slots between.
pub fn teach(known: [u16; 4], taught: [u16; 4]) -> [u16; 4] {
    teach_marked(known, 0, taught, false).0
}

/// `teach`, with the moves' save marks (bit `i` for move `i`, as `Learned::save_moves`): known
/// moves keep theirs, taught ones are marked when `from_save`. A move it knew that is taught
/// again takes the teacher's mark.
pub fn teach_marked(
    known: [u16; 4],
    known_marks: u8,
    taught: [u16; 4],
    from_save: bool,
) -> ([u16; 4], u8) {
    let mut out = [0u16; 4];
    let mut marks = 0u8;
    let mut n = 0;
    let known = known
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, known_marks & 1 << i != 0));
    for (id, marked) in taught.into_iter().map(|id| (id, from_save)).chain(known) {
        if n < out.len() && id != 0 && !out[..n].contains(&id) {
            out[n] = id;
            if marked {
                marks |= 1 << n;
            }
            n += 1;
        }
    }
    (out, marks)
}

impl Store {
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
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
    /// Returns what the Chao has now.
    pub fn absorbed(&mut self, garden: u8, key: u64, identity: u64) -> Option<Learned> {
        let at = self
            .waiting
            .iter()
            .position(|w| w.garden == garden && w.key == key)?;
        let partner = self.waiting.remove(at).partner;
        Some(self.learn(identity, partner))
    }

    /// A Chao (`identity`) absorbed `partner`, which never waited here (a wild visitor): as
    /// `absorbed`. Returns what the Chao has now.
    pub fn learn(&mut self, identity: u64, partner: Partner) -> Learned {
        let (known, marks) = self
            .partners
            .get(&identity)
            .map_or(([0; 4], 0), |l| (l.moves, l.save_moves));
        let (moves, save_moves) = teach_marked(known, marks, partner.moves, partner.from_save);
        let learned = Learned {
            partner,
            moves,
            save_moves,
        };
        self.partners.insert(identity, learned);
        learned
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
        let back = Store::parse(&s.to_text());
        assert_eq!(back, s);
        // Junk lines are skipped.
        let junk = Store::parse(
            "partner zz 1\nwaiting 1\nbrought 1 2\nimport 12 1 x\nhello\n\
             partner 1 2 3 4 5 6 7 8 9 10 11 12 13 x\n",
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
    fn save_marks_follow_the_moves() {
        // Taught by a save partner: marked; known ones keep their marks as they move down.
        assert_eq!(
            teach_marked([0; 4], 0, [5, 6, 0, 0], true),
            ([5, 6, 0, 0], 0b0011)
        );
        assert_eq!(
            teach_marked([5, 6, 0, 0], 0b0011, [7, 0, 0, 0], false),
            ([7, 5, 6, 0], 0b0110)
        );
        // Taught again by a wild one: the mark goes.
        assert_eq!(
            teach_marked([7, 5, 6, 0], 0b0110, [5, 0, 0, 0], false),
            ([5, 7, 6, 0], 0b0100)
        );
        // Pushed out: its mark goes with it.
        assert_eq!(
            teach_marked([1, 2, 3, 4], 0b1000, [8, 9, 0, 0], false),
            ([8, 9, 1, 2], 0)
        );
        let mut s = Store::default();
        let saved = Partner {
            from_save: true,
            moves: [5, 6, 0, 0],
            ..p(1)
        };
        assert_eq!(s.learn(77, saved).save_moves, 0b0011);
        let wild = Partner {
            moves: [7, 0, 0, 0],
            ..p(2)
        };
        let l = s.learn(77, wild);
        assert_eq!((l.moves, l.save_moves), ([7, 5, 6, 0], 0b0110));
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
        let learned = s.absorbed(1, 1, 77).unwrap();
        assert_eq!(learned.partner, p(1));
        assert_eq!(learned.moves, [1, 2, 300, 0]);
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
        // A later absorb replaces the partner and teaches its moves on top of the ones known.
        let learned = s.absorbed(1, 10, 77).unwrap();
        assert_eq!(s.partners.len(), 1);
        assert_eq!(learned.partner.species, 2);
        assert_eq!(learned.moves, [50, 1, 2, 300]);
        assert_eq!(s.absorbed(1, 999, 77), None);
    }

    #[test]
    fn a_visitor_teaches_without_waiting() {
        let mut s = Store::default();
        s.import(1, 1, 20_100, p(1)).unwrap();
        let learned = s.learn(77, p(2));
        assert_eq!(learned.partner, p(2));
        assert_eq!(learned.moves, [1, 2, 300, 0]);
        let wild = Partner {
            moves: [7, 0, 0, 0],
            ..p(3)
        };
        assert_eq!(s.learn(77, wild).moves, [7, 1, 2, 300]);
        // The waiting partner and the day's imports are untouched.
        assert_eq!(s.waiting.len(), 1);
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.partners[&77].partner, wild);
    }
}

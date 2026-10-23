//! The trainer ladder's order, locks and progress (re/chao-battle-online.md 2).
//!
//! The ladder is the GBA game's trainers in the order the game meets them: the route trainers
//! as warm-ups in tiers by strength, then the gym leaders one by one, the league's four and the
//! champion. Each leader, league trainer and the champion opens once the one before is beaten
//! (the first leader is open from the start); route tier k opens with leader k (tier 0 from
//! the start). Beating a leader earns their badge and the TM move their gym gives, which any of
//! the player's Chao can then be taught in place of one of its moves.
//!
//! Progress is the mod's own file, `<mod>/gba/trainers.txt`, one record a line (the Chao save
//! is never changed for it):
//!
//! - `beaten <trainer id>`: a trainer the player beat.
//! - `taught <chao identity> <move> <move> <move> <move>`: the TM moves taught to a Chao, by its
//!   `ChaoData::identity` (hex), one per move slot; 0 leaves the slot to the Chao's own move.
//!
//! Saving adds to what the file holds (it's written whole through a temporary file, and not at
//! all while it can't be read). The TM moves go along with the Chao's partner record to
//! friends (`Body::Partners`) and into exported Chao's `partners.txt`, so copies of it fight
//! with them too.
//!
//! No game access here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::gba_link::Profile;
use crate::gba_link::store::{Learned, Partner, Store};
use crate::net::protocol::SharedPartner;

/// The progress file, in the mod's `gba` folder.
pub const FILE: &str = "trainers.txt";
/// Move slots a Chao has.
const SLOTS: usize = 4;

/// One step of the ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    /// Route trainer tier (0..route tiers).
    Route(usize),
    /// Gym leader, in gym order.
    Leader(usize),
    /// League trainer, in battle order.
    League(usize),
    Champion,
}

/// The ladder's trainers by trainer id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Route trainer tiers, each in the order shown.
    pub route: Vec<Vec<u16>>,
    pub leaders: Vec<u16>,
    pub league: Vec<u16>,
    pub champion: u16,
}

impl Plan {
    pub fn from_rom(l: &crate::gba::ladder::Ladder) -> Plan {
        Plan {
            route: l.route.clone(),
            leaders: l.leaders.iter().map(|x| x.id).collect(),
            league: l.league.clone(),
            champion: l.champion,
        }
    }

    /// Every rung in ladder order: the route tiers, the leaders, the league, the champion.
    pub fn rungs(&self) -> Vec<Rung> {
        (0..self.route.len())
            .map(Rung::Route)
            .chain((0..self.leaders.len()).map(Rung::Leader))
            .chain((0..self.league.len()).map(Rung::League))
            .chain([Rung::Champion])
            .collect()
    }

    /// The trainer a rung is (none for a route tier, which holds many).
    pub fn trainer(&self, rung: Rung) -> Option<u16> {
        match rung {
            Rung::Route(_) => None,
            Rung::Leader(i) => self.leaders.get(i).copied(),
            Rung::League(i) => self.league.get(i).copied(),
            Rung::Champion => Some(self.champion),
        }
    }

    /// The trainer that has to be beaten before `rung` opens (none: open from the start).
    pub fn needs(&self, rung: Rung) -> Option<u16> {
        match rung {
            Rung::Route(0) | Rung::Leader(0) => None,
            Rung::Route(k) => self.leaders.get(k - 1).copied(),
            Rung::Leader(k) => self.leaders.get(k - 1).copied(),
            Rung::League(0) => self.leaders.last().copied(),
            Rung::League(k) => self.league.get(k - 1).copied(),
            Rung::Champion => self.league.last().copied(),
        }
    }

    pub fn open(&self, rung: Rung, progress: &Progress) -> bool {
        self.needs(rung)
            .is_none_or(|id| progress.beaten.contains(&id))
    }

    /// The rung a trainer is on.
    pub fn rung_of(&self, id: u16) -> Option<Rung> {
        if let Some(t) = self.route.iter().position(|t| t.contains(&id)) {
            return Some(Rung::Route(t));
        }
        self.rungs()
            .into_iter()
            .find(|&r| self.trainer(r) == Some(id))
    }

    /// Whether the trainer `id` can be battled now.
    pub fn can_battle(&self, id: u16, progress: &Progress) -> bool {
        self.rung_of(id).is_some_and(|r| self.open(r, progress))
    }

    /// The badges earned (leader indices, in order).
    pub fn badges(&self, progress: &Progress) -> Vec<usize> {
        (0..self.leaders.len())
            .filter(|&i| progress.beaten.contains(&self.leaders[i]))
            .collect()
    }
}

/// What the player has done on the ladder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub beaten: BTreeSet<u16>,
    /// Chao identity -> the TM move in each move slot (0: its own move).
    pub taught: BTreeMap<u64, [u16; SLOTS]>,
}

impl Progress {
    pub fn parse(text: &str) -> Progress {
        let mut p = Progress::default();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for line in text.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            match w[..] {
                ["beaten", id] => {
                    if let Ok(id) = id.parse() {
                        p.beaten.insert(id);
                    }
                }
                ["taught", identity, a, b, c, d] => {
                    let moves: Option<Vec<u16>> =
                        [a, b, c, d].iter().map(|m| m.parse().ok()).collect();
                    if let (Ok(identity), Some(moves)) = (u64::from_str_radix(identity, 16), moves)
                        && moves.iter().any(|&m| m != 0)
                    {
                        p.taught
                            .insert(identity, [moves[0], moves[1], moves[2], moves[3]]);
                    }
                }
                _ => {}
            }
        }
        p
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for id in &self.beaten {
            out.push_str(&format!("beaten {id}\n"));
        }
        for (identity, m) in &self.taught {
            out.push_str(&format!(
                "taught {identity:016x} {} {} {} {}\n",
                m[0], m[1], m[2], m[3]
            ));
        }
        out
    }

    /// Records a win; true the first time.
    pub fn beat(&mut self, id: u16) -> bool {
        self.beaten.insert(id)
    }

    /// Teaches the Chao with `identity` the TM move `move_id` in move slot `slot`.
    pub fn teach(&mut self, identity: u64, slot: usize, move_id: u16) {
        if slot >= SLOTS || move_id == 0 {
            return;
        }
        let moves = self.taught.entry(identity).or_insert([0; SLOTS]);
        // The same move in another slot gives way.
        for m in moves.iter_mut() {
            if *m == move_id {
                *m = 0;
            }
        }
        moves[slot] = move_id;
    }

    /// Adds `other`'s wins and TM moves (theirs win a slot).
    pub fn merge(&mut self, other: &Progress) {
        self.beaten.extend(&other.beaten);
        for (&identity, moves) in &other.taught {
            for (slot, &id) in moves.iter().enumerate() {
                self.teach(identity, slot, id);
            }
        }
    }

    /// A Chao's battle profile with its TM moves in their slots. A TM move it already knows
    /// in another slot changes places with the move in the TM's slot.
    pub fn apply(&self, identity: u64, mut profile: Profile) -> Profile {
        let Some(taught) = self.taught.get(&identity) else {
            return profile;
        };
        for (slot, &id) in taught.iter().enumerate() {
            if id == 0 {
                continue;
            }
            if let Some(j) = profile.moves.iter().position(|&m| m == id) {
                profile.moves[j] = profile.moves[slot];
            }
            profile.moves[slot] = id;
        }
        profile
    }
}

fn path() -> PathBuf {
    Path::new(crate::online::mod_path()).join("gba").join(FILE)
}

/// The progress file at `path` (`None` when there's none yet).
fn read(path: &Path) -> std::io::Result<Option<Progress>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(Progress::parse(&String::from_utf8_lossy(&bytes)))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Reads the progress file from the mod's `gba` folder (none yet, or unreadable: no progress).
pub fn load() -> Progress {
    match read(&path()) {
        Ok(p) => p.unwrap_or_default(),
        Err(e) => {
            crate::log!("battle: can't read {FILE}: {e}");
            Progress::default()
        }
    }
}

/// Adds `progress` to the progress file (left alone while it can't be read).
pub fn save(progress: &Progress) {
    let path = path();
    let mut all = match read(&path) {
        Ok(p) => p.unwrap_or_default(),
        Err(e) => {
            crate::log!("battle: can't read {FILE} to add to it: {e}");
            return;
        }
    };
    all.merge(progress);
    if let Err(e) = write(&path, &all.to_text()) {
        crate::log!("battle: can't write {FILE}: {e}");
    }
}

/// Writes `text` to `path` through a temporary file beside it.
fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("txt.tmp");
    let result = std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Our Chao that were taught TM moves (of `only`, if given), with their battle profiles (TM
/// moves in); none while `BattleTrainers` is off or without the ROM.
fn taught_profiles(only: Option<&[u64]>) -> Vec<(u64, Profile)> {
    if !super::trainers::enabled() {
        return Vec::new();
    }
    let progress = load();
    progress
        .taught
        .keys()
        .copied()
        .filter(|id| only.is_none_or(|o| o.contains(id)))
        .filter_map(|id| {
            let profile = crate::gba_link::battle_profile(id, true).ok()?;
            Some((id, progress.apply(id, profile)))
        })
        .collect()
}

/// What our Chao have from partners (`Body::Partners`) with their TM moves: a Chao's record
/// carries its battle moves, and a Chao taught a TM move without a partner gets one (Normal,
/// no ability).
pub fn shared_partners(mut list: Vec<SharedPartner>) -> Vec<SharedPartner> {
    for (identity, p) in taught_profiles(None) {
        let shared = SharedPartner {
            identity,
            species: p.partner.unwrap_or(0),
            types: p.types,
            ability: p.ability,
            moves: p.moves,
        };
        match list.iter_mut().find(|s| s.identity == identity) {
            Some(s) => *s = shared,
            None => list.push(shared),
        }
    }
    list
}

/// Puts the TM moves of the exported Chao with `identities` into the `partners.txt` beside
/// them in `dir` (the same way as `shared_partners`).
pub fn export_taught(dir: &Path, identities: &[u64]) {
    let taught = taught_profiles(Some(identities));
    if taught.is_empty() {
        return;
    }
    let path = dir.join("partners.txt");
    let mut store = match std::fs::read_to_string(&path) {
        Ok(text) => Store::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
        Err(e) => {
            crate::log!("battle: can't read {}: {e}", path.display());
            return;
        }
    };
    for (identity, p) in taught {
        let before = store.partners.get(&identity);
        let partner = Partner {
            species: p.partner.unwrap_or(0),
            types: p.types,
            moves: p.moves,
            level: before.map_or(0, |l| l.partner.level),
            ability: p.ability,
            from_save: before.is_some_and(|l| l.partner.from_save),
        };
        let save_moves = before.map_or(0, |l| l.marks_for(p.moves));
        store.partners.insert(
            identity,
            Learned {
                partner,
                moves: p.moves,
                save_moves,
            },
        );
    }
    if let Err(e) = write(&path, &store.to_text()) {
        crate::log!("battle: can't write {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan {
            route: vec![vec![10, 11], vec![12], vec![13, 14]],
            leaders: vec![100, 101],
            league: vec![200, 201],
            champion: 300,
        }
    }

    #[test]
    fn rungs_come_in_ladder_order() {
        let p = plan();
        assert_eq!(
            p.rungs(),
            [
                Rung::Route(0),
                Rung::Route(1),
                Rung::Route(2),
                Rung::Leader(0),
                Rung::Leader(1),
                Rung::League(0),
                Rung::League(1),
                Rung::Champion,
            ]
        );
        assert_eq!(p.trainer(Rung::Leader(1)), Some(101));
        assert_eq!(p.trainer(Rung::Champion), Some(300));
        assert_eq!(p.trainer(Rung::Route(0)), None);
        assert_eq!(p.rung_of(14), Some(Rung::Route(2)));
        assert_eq!(p.rung_of(201), Some(Rung::League(1)));
        assert_eq!(p.rung_of(999), None);
    }

    #[test]
    fn each_rung_opens_when_the_one_before_is_beaten() {
        let p = plan();
        let mut progress = Progress::default();
        let open = |progress: &Progress| -> Vec<Rung> {
            p.rungs()
                .into_iter()
                .filter(|&r| p.open(r, progress))
                .collect()
        };
        assert_eq!(open(&progress), [Rung::Route(0), Rung::Leader(0)]);
        assert!(p.can_battle(11, &progress) && !p.can_battle(12, &progress));
        assert!(!p.can_battle(101, &progress) && !p.can_battle(999, &progress));
        // Route trainers are warm-ups: beating them opens nothing.
        progress.beat(10);
        progress.beat(11);
        assert_eq!(open(&progress), [Rung::Route(0), Rung::Leader(0)]);
        progress.beat(100);
        assert_eq!(
            open(&progress),
            [
                Rung::Route(0),
                Rung::Route(1),
                Rung::Leader(0),
                Rung::Leader(1)
            ]
        );
        progress.beat(101);
        assert!(p.open(Rung::Route(2), &progress) && p.open(Rung::League(0), &progress));
        assert!(!p.open(Rung::League(1), &progress));
        progress.beat(200);
        assert!(p.open(Rung::League(1), &progress) && !p.open(Rung::Champion, &progress));
        progress.beat(201);
        assert!(p.open(Rung::Champion, &progress));
        assert_eq!(open(&progress).len(), p.rungs().len());
        assert_eq!(p.badges(&progress), [0, 1]);
    }

    #[test]
    fn a_later_leader_beaten_out_of_order_does_not_open_the_ones_between() {
        let p = plan();
        let mut progress = Progress::default();
        progress.beat(101);
        assert!(!p.open(Rung::Leader(1), &progress));
        assert!(p.open(Rung::Route(2), &progress));
        assert_eq!(p.badges(&progress), [1]);
    }

    #[test]
    fn progress_round_trips_and_skips_bad_lines() {
        let mut p = Progress::default();
        assert!(p.beat(265));
        assert!(!p.beat(265));
        p.beat(12);
        p.teach(0xABCD_0000_0000_0001, 2, 89);
        let text = p.to_text();
        assert_eq!(
            text,
            "beaten 12\nbeaten 265\ntaught abcd000000000001 0 0 89 0\n"
        );
        assert_eq!(Progress::parse(&text), p);
        let odd = format!("{text}beaten x\ntaught zz 1 2 3 4\ntaught 1 0 0 0 0\nother 5\n\n");
        assert_eq!(Progress::parse(&odd), p);
        // A byte order mark from an editor.
        assert_eq!(Progress::parse(&format!("\u{feff}{text}")), p);
        assert_eq!(Progress::parse(""), Progress::default());
    }

    #[test]
    fn teaching_puts_a_move_in_a_slot_once() {
        let mut p = Progress::default();
        p.teach(7, 1, 50);
        p.teach(7, 3, 60);
        assert_eq!(p.taught[&7], [0, 50, 0, 60]);
        // The same move again moves it.
        p.teach(7, 0, 60);
        assert_eq!(p.taught[&7], [60, 50, 0, 0]);
        // Bad slots and empty moves do nothing.
        p.teach(7, 4, 70);
        p.teach(7, 2, 0);
        assert_eq!(p.taught[&7], [60, 50, 0, 0]);
    }

    #[test]
    fn merging_keeps_both_wins_and_the_newer_moves() {
        let mut file = Progress::default();
        file.beat(1);
        file.teach(7, 0, 60);
        file.teach(7, 2, 70);
        file.teach(8, 1, 80);
        let mut session = Progress::default();
        session.beat(2);
        session.teach(7, 1, 60);
        file.merge(&session);
        assert_eq!(file.beaten.iter().copied().collect::<Vec<_>>(), [1, 2]);
        assert_eq!(file.taught[&7], [0, 60, 70, 0]);
        assert_eq!(file.taught[&8], [0, 80, 0, 0]);
    }

    #[test]
    fn saving_writes_through_a_temporary_file() {
        let dir = std::env::temp_dir().join(format!("sa2co-ladder-{}", std::process::id()));
        let path = dir.join(FILE);
        let mut p = Progress::default();
        p.beat(5);
        write(&path, &p.to_text()).unwrap();
        assert_eq!(read(&path).unwrap(), Some(p));
        assert!(!path.with_extension("txt.tmp").exists());
        assert_eq!(read(&dir.join("none.txt")).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn taught_moves_go_into_the_profile() {
        let profile = Profile {
            types: [0, 0],
            ability: 0,
            moves: [1, 2, 3, 4],
            partner: None,
        };
        let mut p = Progress::default();
        assert_eq!(p.apply(7, profile), profile);
        p.teach(7, 0, 9);
        assert_eq!(p.apply(7, profile).moves, [9, 2, 3, 4]);
        // A taught move the Chao knows already changes places.
        p.teach(7, 1, 4);
        assert_eq!(p.apply(7, profile).moves, [9, 4, 3, 2]);
        // Other Chao keep theirs.
        assert_eq!(p.apply(8, profile), profile);
    }
}

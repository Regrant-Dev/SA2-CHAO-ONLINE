//! The trainer ladder's order, locks and progress (re/chao-battle-online.md 2).
//!
//! The mod arranges the imported trainer roster into strength tiers, milestones and final
//! challenges. A milestone opens once its predecessor is beaten; the first milestone and
//! warm-up tier are available immediately. Teaching rewards are offered only when the source
//! profile defines them. Trainers and Chao keep their own levels, and trainers can be battled
//! again.
//!
//! Progress lives in `<mod>/creature/trainer-progress.txt`, one `beaten <trainer id>` line for
//! each trainer the player beat. The native Chao save is never changed for trainer progress, and
//! the milestone moves taught to Chao are kept with their other moves in `partners.txt`.
//!
//! Saving adds to what the file holds (it's written whole through a temporary file, and not at
//! all while it can't be read).
//!
//! No game access here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The progress file, in the mod's `creature` folder.
pub const FILE: &str = "trainer-progress.txt";

/// One step of the ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    /// Route trainer tier (0..route tiers).
    Route(usize),
    /// Milestone, in order.
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
    pub fn from_rom(l: &crate::creature::ladder::Ladder) -> Plan {
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
}

impl Progress {
    pub fn parse(text: &str) -> Progress {
        let mut p = Progress::default();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for line in text.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            if let ["beaten", id] = w[..]
                && let Ok(id) = id.parse()
            {
                p.beaten.insert(id);
            }
        }
        p
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for id in &self.beaten {
            out.push_str(&format!("beaten {id}\n"));
        }
        out
    }

    /// Records a win; true the first time.
    pub fn beat(&mut self, id: u16) -> bool {
        self.beaten.insert(id)
    }

    /// Adds `other`'s wins.
    pub fn merge(&mut self, other: &Progress) {
        self.beaten.extend(&other.beaten);
    }
}

fn path() -> PathBuf {
    Path::new(crate::online::mod_path())
        .join("creature")
        .join(FILE)
}

/// The progress file at `path` (`None` when there's none yet).
fn read(path: &Path) -> std::io::Result<Option<Progress>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(Progress::parse(&String::from_utf8_lossy(&bytes)))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Reads the progress file from the mod's `creature` folder (none yet, or unreadable: no progress).
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

/// Writes `text` to `path` through a temporary file beside it (also for `creature_link`'s
/// `partners.txt` and `market_refresh`'s file).
pub(crate) fn write(path: &Path, text: &str) -> std::io::Result<()> {
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
        let text = p.to_text();
        assert_eq!(text, "beaten 12\nbeaten 265\n");
        assert_eq!(Progress::parse(&text), p);
        let odd = format!("{text}beaten x\ntaught 1 0 0 9 0\nother 5\n\n");
        assert_eq!(Progress::parse(&odd), p);
        // A byte order mark from an editor.
        assert_eq!(Progress::parse(&format!("\u{feff}{text}")), p);
        assert_eq!(Progress::parse(""), Progress::default());
    }

    #[test]
    fn merging_keeps_both_wins() {
        let mut file = Progress::default();
        file.beat(1);
        let mut session = Progress::default();
        session.beat(2);
        file.merge(&session);
        assert_eq!(file.beaten.iter().copied().collect::<Vec<_>>(), [1, 2]);
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
}

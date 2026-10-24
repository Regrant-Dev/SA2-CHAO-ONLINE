//! Battling a wild visitor (re/shared-visitors.md section 6): from a garden, the info card's
//! battle button takes the player straight to the karate arena (`GoToChaoArea(10)`), where the
//! battle menu opens at the team screen against that one visitor, and back to the garden and
//! the place they stood once it ends.
//!
//! The visitor fights as a trainer's creature does (`trainers`): at its level, with its four
//! moves, its ability and its species' types and base stats from the player's own ROM, IV 15 in
//! every stat and a neutral nature (the numbers its info card shows). A win gives the Chao that
//! took part wild XP (`levels::wild_xp`) and the visitor stays; a loss or giving up gives
//! nothing and it runs away; backing out before the fight leaves it as it was. In a shared
//! garden the host keeps the visitor for the battler meanwhile and is told how it ended
//! (`Body::WildBattle`); alone, the garden's visit is kept while the player is away
//! (`gba_link::wild`).
//!
//! The game's own karate exit always goes to the Neutral garden, so it never runs for a trip:
//! game.rs ends the karate area with the same steps toward the garden the trip came from
//! (`arena::Karate::exit_to`).

// The trip only runs in the 32-bit game build.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::time::{Duration, Instant};

use crate::gba_link::store::Partner;
use crate::net::protocol::WildStep;

use super::data::MoveData;
use super::fighter::Fighter;
use super::team::Dex;
use super::trainers::{self, Creature};

/// A visitor fights with this IV in every stat and a neutral nature, as its card shows it.
pub const IV: u8 = 15;
/// Frames (of running game) the karate area gets to come up after leaving the garden.
const GOING_LIMIT: u32 = 30 * 60;
/// Frames in the karate area before the battle menu must have opened.
const OPEN_LIMIT: u32 = 30 * 60;
/// Frames the way back to the garden may take.
const BACK_LIMIT: u32 = 60 * 60;
/// A trip ends as given up this long after it set off, paused or not: a minute inside the host's
/// own limit for a reservation, so the game always gives up first.
const TRIP_LIMIT: Duration = Duration::from_secs(14 * 60);

/// A wild visitor to battle, as ids from the ROM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WildFoe {
    pub species: u16,
    pub level: u8,
    /// The ability id (0: none).
    pub ability: u8,
    pub shiny: bool,
    /// 0: an empty slot.
    pub moves: [u16; 4],
}

impl WildFoe {
    /// The visitor that teaches `partner` (what its info card shows).
    pub fn of(partner: &Partner, shiny: bool) -> WildFoe {
        WildFoe {
            species: partner.species,
            level: partner.level,
            ability: partner.ability,
            shiny,
            moves: partner.moves,
        }
    }
}

/// `foe` as a fighter named `name`, with its species' base stats, types and XP yield (none
/// when the dex has none of its moves).
pub fn build(
    dex: &Dex,
    foe: &WildFoe,
    name: &str,
    base: [u8; 6],
    types: [u8; 2],
    exp_yield: u8,
) -> Option<Creature> {
    let moves: Vec<MoveData> = foe
        .moves
        .iter()
        .filter(|&&id| id != 0)
        .filter_map(|&id| dex.move_data(id))
        .collect();
    if moves.is_empty() {
        return None;
    }
    let level = foe.level.clamp(1, 100);
    let stats = trainers::stats(base, IV, level, 0);
    Some(Creature {
        species: foe.species,
        exp_yield,
        fighter: Fighter::new(name, level, stats, types, foe.ability, &moves),
    })
}

/// `foe` as a fighter from the ROM.
pub fn creature(rom: &crate::gba::rom::Rom, dex: &Dex, foe: &WildFoe) -> Option<Creature> {
    let s = rom.species(foe.species)?;
    let name = crate::gba_link::ascii(&rom.species_name(foe.species)?);
    build(dex, foe, &name, s.base, s.types, s.exp_yield)
}

/// How a battle with a visitor ended for the visitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The player won: it stays in the garden.
    Won,
    /// The player lost or gave up: it runs away.
    RanAway,
    /// No battle was fought (backed out at the team screen, or the trip didn't get there): it
    /// stays as it was.
    Cancelled,
}

impl Ending {
    /// The visitor is still in the garden after it.
    pub fn stays(self) -> bool {
        self != Ending::RanAway
    }

    /// What the host is told (`Body::WildBattle`).
    pub fn step(self) -> WildStep {
        match self {
            Ending::Won => WildStep::Won,
            Ending::RanAway => WildStep::Lost,
            Ending::Cancelled => WildStep::Cancelled,
        }
    }

    /// The end of a trip the battle menu didn't see out (the area left another way, the menu
    /// stopped): a fight that started counts as given up.
    pub fn cut_short(fought: bool) -> Ending {
        if fought {
            Ending::RanAway
        } else {
            Ending::Cancelled
        }
    }
}

/// Which visitor a trip is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Battled {
    /// One of the player's own (a garden they have alone; `gba_link::wild` keeps the visit).
    Own,
    /// The shared visitor with this host id, reserved for the player.
    Shared(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// `GoToChaoArea(10)` was called: on the way to the karate area.
    Going,
    /// In the karate area (the battle menu opened, or about to).
    Karate { opened: bool },
    /// The battle is over: on the way back to the garden.
    Back,
}

/// Where the game is this frame, for a trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Here {
    /// The Chao World area (`CurrentChaoArea`; 0 outside Chao World).
    pub area: i32,
    /// Playing there (not loading, not paused): frames count only then.
    pub settled: bool,
}

/// What a trip asks of the game this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do {
    /// Tell how the battle ended (once).
    Report(Ending),
    /// Back in the garden: put the player where they stood, and say how it ended.
    PutBack(Option<Ending>),
    /// The trip is over.
    Done,
}

/// A trip from a garden to the karate arena and back.
#[derive(Clone, Debug, PartialEq)]
pub struct Trip {
    /// The garden it started in (1..=3), where the player stood and faced.
    pub origin: u8,
    pub at: [f32; 3],
    pub angle: i32,
    pub foe: WildFoe,
    pub battled: Battled,
    phase: Phase,
    /// Running frames in this phase.
    frames: u32,
    /// When it set off.
    since: Instant,
    fought: bool,
    ending: Option<Ending>,
    reported: bool,
}

impl Trip {
    pub fn new(origin: u8, at: [f32; 3], angle: i32, foe: WildFoe, battled: Battled) -> Trip {
        Trip {
            origin,
            at,
            angle,
            foe,
            battled,
            phase: Phase::Going,
            frames: 0,
            since: Instant::now(),
            fought: false,
            ending: None,
            reported: false,
        }
    }

    fn report(&mut self, ending: Ending, out: &mut Vec<Do>) {
        if !self.reported {
            self.reported = true;
            self.ending = Some(ending);
            out.push(Do::Report(ending));
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.frames = 0;
    }

    /// One frame of the trip.
    pub fn frame(&mut self, here: Here) -> Vec<Do> {
        let mut out = Vec::new();
        if here.settled {
            self.frames = self.frames.saturating_add(1);
        }
        if self.overdue() {
            // A battle still on in the arena finishes as given up (`Session`), and they go
            // back as usual.
            self.report(Ending::RanAway, &mut out);
        }
        let karate = here.area == super::arena::AREA_KARATE;
        let origin = here.area == i32::from(self.origin);
        match self.phase {
            Phase::Going if karate => self.set_phase(Phase::Karate { opened: false }),
            Phase::Going => {
                // It never got there: still in the garden, or somewhere else.
                let elsewhere = here.settled && !origin;
                if elsewhere || self.frames > GOING_LIMIT {
                    self.report(Ending::Cancelled, &mut out);
                    out.push(Do::Done);
                }
            }
            Phase::Karate { .. } if !karate => {
                // Left the karate area some other way (the pause menu's exit).
                self.report(Ending::cut_short(self.fought), &mut out);
                self.set_phase(Phase::Back);
            }
            Phase::Karate { .. } => {}
            Phase::Back if here.settled && origin => {
                out.push(Do::PutBack(self.ending));
                out.push(Do::Done);
            }
            Phase::Back => {
                let other_garden = here.settled && (1..=3).contains(&here.area) && !karate;
                if other_garden || self.frames > BACK_LIMIT {
                    out.push(Do::Done);
                }
            }
        }
        out
    }

    /// It has gone on too long: it counts as given up.
    pub fn overdue(&self) -> bool {
        self.since.elapsed() > TRIP_LIMIT
    }

    /// The karate menu waits for input and the battle menu hasn't opened for the trip yet.
    pub fn wants_menu(&self) -> bool {
        self.phase == Phase::Karate { opened: false }
    }

    /// The battle menu opened.
    pub fn opened(&mut self) {
        if let Phase::Karate { .. } = self.phase {
            self.phase = Phase::Karate { opened: true };
        }
    }

    /// The menu didn't open in time.
    pub fn open_overdue(&self) -> bool {
        self.phase == Phase::Karate { opened: false } && self.frames > OPEN_LIMIT
    }

    /// The fight began (the arena opened for it).
    pub fn fought(&mut self) {
        self.fought = true;
    }

    /// In the karate area for the battle (the menu open or about to).
    pub fn in_karate(&self) -> bool {
        matches!(self.phase, Phase::Karate { .. })
    }

    /// The battle menu ended with `ending` (none: it didn't say). In the karate area the trip
    /// goes back now: what to report (once), and the garden to go back to.
    pub fn ended(&mut self, ending: Option<Ending>) -> (Vec<Do>, Option<u8>) {
        let mut out = Vec::new();
        if !self.in_karate() {
            return (out, None);
        }
        self.report(ending.unwrap_or(Ending::cut_short(self.fought)), &mut out);
        self.set_phase(Phase::Back);
        (out, Some(self.origin))
    }
}

/// What the player is told back in the garden.
pub fn back_note(ending: Option<Ending>, name: &str) -> Option<String> {
    match ending? {
        Ending::Won => Some(format!("{name} stays in the garden")),
        Ending::RanAway => Some(format!("{name} ran away")),
        Ending::Cancelled => None,
    }
}

#[cfg(target_arch = "x86")]
pub use game::{
    ended, fought, on_frame, open_overdue, opened, overdue, start, trip_foe, underway, wants_menu,
};

#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}
#[cfg(not(target_arch = "x86"))]
pub fn start(_trip: Trip) -> bool {
    false
}
#[cfg(not(target_arch = "x86"))]
pub fn underway() -> bool {
    false
}
#[cfg(not(target_arch = "x86"))]
pub fn wants_menu() -> bool {
    false
}
#[cfg(not(target_arch = "x86"))]
pub fn trip_foe() -> Option<WildFoe> {
    None
}
#[cfg(not(target_arch = "x86"))]
pub fn opened() {}
#[cfg(not(target_arch = "x86"))]
pub fn open_overdue() -> bool {
    false
}
#[cfg(not(target_arch = "x86"))]
pub fn fought() {}
#[cfg(not(target_arch = "x86"))]
pub fn overdue() -> bool {
    false
}
#[cfg(not(target_arch = "x86"))]
pub fn ended(_ending: Option<Ending>) -> Option<u8> {
    None
}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::Mutex;

    use super::{Battled, Do, Ending, Here, Trip, WildFoe, back_note};
    use crate::game::{self as sa2, Vector};
    use crate::log;

    /// `GoToChaoArea(area)` 0x52B5B0 (cdecl).
    const GO_TO_CHAO_AREA: usize = 0x0052_B5B0;
    /// The player's EntityData1 +0x0C: facing (BAMS).
    const ENTITY_ROTATION_Y: usize = 0x0C;

    static TRIP: Mutex<Option<Trip>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut Option<Trip>) -> R) -> R {
        f(&mut TRIP.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Sets off on `trip` from the garden: false when one is underway already.
    pub fn start(trip: Trip) -> bool {
        if underway() {
            return false;
        }
        log!(
            "battle: to the karate arena to battle a wild visitor (species {}, level {})",
            trip.foe.species,
            trip.foe.level
        );
        with(|t| *t = Some(trip));
        // SAFETY: the game's cdecl GoToChaoArea, as the Stadium's own menu calls it (0x581935),
        // from OnFrame in a running garden.
        unsafe {
            let go: extern "C" fn(i32) = std::mem::transmute(GO_TO_CHAO_AREA);
            go(super::super::arena::AREA_KARATE);
        }
        true
    }

    /// A trip is on.
    pub fn underway() -> bool {
        with(|t| t.is_some())
    }

    pub fn wants_menu() -> bool {
        with(|t| t.as_ref().is_some_and(Trip::wants_menu))
    }

    /// The trip's visitor.
    pub fn trip_foe() -> Option<WildFoe> {
        with(|t| t.as_ref().map(|t| t.foe))
    }

    pub fn opened() {
        with(|t| t.as_mut().map(Trip::opened));
    }

    pub fn open_overdue() -> bool {
        with(|t| t.as_ref().is_some_and(Trip::open_overdue))
    }

    pub fn fought() {
        with(|t| t.as_mut().map(Trip::fought));
    }

    /// The trip has gone on too long (its battle counts as given up).
    pub fn overdue() -> bool {
        with(|t| t.as_ref().is_some_and(Trip::overdue))
    }

    /// The battle menu of the trip ended (`ending`; none: it didn't say). The garden to go back
    /// to, when the trip is in the karate area.
    pub fn ended(ending: Option<Ending>) -> Option<u8> {
        let (todo, back, battled) = with(|t| {
            let Some(trip) = t.as_mut() else {
                return (Vec::new(), None, Battled::Own);
            };
            let (todo, back) = trip.ended(ending);
            (todo, back, trip.battled)
        });
        for d in todo {
            if let Do::Report(e) = d {
                report(battled, e);
            }
        }
        back
    }

    fn report(battled: Battled, ending: Ending) {
        log!("battle: the wild visitor's battle ended: {ending:?}");
        crate::gba_link::wild_battle::over(battled, ending);
    }

    /// Every frame: the trip's way there and back.
    pub fn on_frame() {
        let here = Here {
            area: if sa2::current_level() == sa2::LEVEL_CHAO_WORLD {
                sa2::current_chao_area()
            } else {
                0
            },
            settled: !sa2::between_areas() && sa2::game_state() == sa2::GAME_STATE_INGAME,
        };
        let Some((todo, trip)) = with(|t| {
            let trip = t.as_mut()?;
            let todo = trip.frame(here);
            let copy = trip.clone();
            if todo.contains(&Do::Done) {
                *t = None;
            }
            Some((todo, copy))
        }) else {
            return;
        };
        for d in todo {
            match d {
                Do::Report(e) => report(trip.battled, e),
                Do::PutBack(ending) => put_back(&trip, ending),
                Do::Done => {}
            }
        }
    }

    /// Back in the garden: where the player stood, facing the same way.
    fn put_back(trip: &Trip, ending: Option<Ending>) {
        let [x, y, z] = trip.at;
        sa2::set_player_position(Vector { x, y, z });
        let entity = sa2::local_entity();
        if !entity.is_null() {
            // SAFETY: the player's live EntityData1, on the game thread.
            unsafe {
                (entity.add(ENTITY_ROTATION_Y) as *mut i32).write_unaligned(trip.angle);
            }
        }
        let name = crate::gba_link::rom()
            .ok()
            .and_then(|rom| rom.species_name(trip.foe.species))
            .map(|n| crate::gba_link::ascii(&n))
            .unwrap_or_else(|| "The visitor".into());
        if let Some(note) = back_note(ending, &name) {
            crate::hud::toast(note);
        }
        log!(
            "battle: back in garden {} after the wild battle",
            trip.origin
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::data::TypeChart;

    fn foe() -> WildFoe {
        WildFoe {
            species: 7,
            level: 12,
            ability: 3,
            shiny: true,
            moves: [1, 0, 2, 9],
        }
    }

    #[test]
    fn a_visitor_fights_at_its_level_with_its_moves() {
        let dex = Dex::for_tests(
            &[MoveData::plain(1, 0, 40), MoveData::plain(2, 0, 60)],
            TypeChart::default(),
        );
        let base = [45, 49, 49, 45, 65, 65];
        let c = build(&dex, &foe(), "Mon", base, [11, 3], 64).unwrap();
        assert_eq!((c.species, c.exp_yield), (7, 64));
        let f = &c.fighter;
        assert_eq!((f.name.as_str(), f.level), ("Mon", 12));
        assert_eq!(f.stats, trainers::stats(base, IV, 12, 0));
        assert_eq!(f.types, [11, 3]);
        assert_eq!(f.ability, 3);
        // The moves the dex knows, in order (move 9 isn't in it).
        let ids: Vec<u16> = f.moves.iter().map(|m| m.data.id).collect();
        assert_eq!(ids, [1, 2]);
        // None of its moves known: no fighter.
        let none = WildFoe {
            moves: [9, 0, 0, 0],
            ..foe()
        };
        assert!(build(&dex, &none, "Mon", base, [11, 3], 64).is_none());
    }

    #[test]
    fn a_visitor_from_the_rom() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        let Ok(rom) = crate::gba::rom::Rom::open(&path) else {
            return;
        };
        let dex = Dex::from_rom(&rom).unwrap();
        let species = rom.starters().unwrap()[0];
        let info = rom.species(species).unwrap();
        let foe = WildFoe {
            species,
            level: 10,
            ability: info.abilities[0],
            shiny: false,
            moves: rom.moves_at_level(species, 10),
        };
        let c = creature(&rom, &dex, &foe).unwrap();
        assert_eq!((c.species, c.exp_yield), (species, info.exp_yield));
        assert_eq!(c.fighter.level, 10);
        assert_eq!(c.fighter.types, info.types);
        // The stats its card shows.
        assert_eq!(c.fighter.stats, trainers::stats(info.base, IV, 10, 0));
        let known = foe.moves.iter().filter(|&&m| m != 0).count();
        assert_eq!(c.fighter.moves.len(), known);
        assert!(c.fighter.name.is_ascii() && !c.fighter.name.is_empty());
    }

    #[test]
    fn the_foe_is_what_the_card_shows() {
        let partner = Partner {
            species: 25,
            types: [12, 12],
            moves: [4, 5, 0, 0],
            level: 9,
            ability: 8,
            from_save: false,
        };
        let f = WildFoe::of(&partner, false);
        assert_eq!(
            f,
            WildFoe {
                species: 25,
                level: 9,
                ability: 8,
                shiny: false,
                moves: [4, 5, 0, 0],
            }
        );
    }

    #[test]
    fn endings_say_whether_it_stays() {
        assert!(Ending::Won.stays() && Ending::Cancelled.stays());
        assert!(!Ending::RanAway.stays());
        assert_eq!(Ending::Won.step(), WildStep::Won);
        assert_eq!(Ending::RanAway.step(), WildStep::Lost);
        assert_eq!(Ending::Cancelled.step(), WildStep::Cancelled);
        assert_eq!(Ending::cut_short(true), Ending::RanAway);
        assert_eq!(Ending::cut_short(false), Ending::Cancelled);
        assert_eq!(
            back_note(Some(Ending::Won), "Mon").unwrap(),
            "Mon stays in the garden"
        );
        assert_eq!(
            back_note(Some(Ending::RanAway), "Mon").unwrap(),
            "Mon ran away"
        );
        assert_eq!(back_note(Some(Ending::Cancelled), "Mon"), None);
        assert_eq!(back_note(None, "Mon"), None);
    }

    const KARATE: i32 = super::super::arena::AREA_KARATE;

    fn at(area: i32) -> Here {
        Here {
            area,
            settled: true,
        }
    }

    fn loading(area: i32) -> Here {
        Here {
            area,
            settled: false,
        }
    }

    fn trip() -> Trip {
        Trip::new(2, [1.0, 2.0, 3.0], 0x4000, foe(), Battled::Shared(5))
    }

    #[test]
    fn a_trip_there_a_battle_and_back() {
        let mut t = trip();
        // Leaving: the garden still runs a moment, then the load.
        assert!(t.frame(at(2)).is_empty());
        assert!(t.frame(loading(KARATE)).is_empty());
        assert!(t.in_karate() && t.wants_menu());
        t.opened();
        assert!(!t.wants_menu());
        t.fought();
        // Won: reported once, back to garden 2.
        let (todo, back) = t.ended(Some(Ending::Won));
        assert_eq!(todo, [Do::Report(Ending::Won)]);
        assert_eq!(back, Some(2));
        assert_eq!(t.ended(Some(Ending::RanAway)), (Vec::new(), None));
        // Loading, then the garden: put back.
        assert!(t.frame(loading(2)).is_empty());
        assert_eq!(t.frame(at(2)), [Do::PutBack(Some(Ending::Won)), Do::Done]);
    }

    #[test]
    fn a_menu_that_says_nothing_counts_by_whether_they_fought() {
        let mut t = trip();
        t.frame(at(KARATE));
        t.opened();
        let (todo, _) = t.ended(None);
        assert_eq!(todo, [Do::Report(Ending::Cancelled)]);
        let mut t = trip();
        t.frame(at(KARATE));
        t.opened();
        t.fought();
        let (todo, _) = t.ended(None);
        assert_eq!(todo, [Do::Report(Ending::RanAway)]);
    }

    #[test]
    fn leaving_the_arena_another_way_gives_up_a_fight() {
        let mut t = trip();
        t.frame(at(KARATE));
        t.opened();
        t.fought();
        // The pause menu's exit: to the Neutral garden.
        assert_eq!(t.frame(loading(1)), [Do::Report(Ending::RanAway)]);
        assert!(!t.in_karate());
        // Not the trip's garden: the trip is over without putting them back.
        assert_eq!(t.frame(at(1)), [Do::Done]);
        // Before the fight it doesn't count.
        let mut t = trip();
        t.frame(at(KARATE));
        assert_eq!(t.frame(at(1)), [Do::Report(Ending::Cancelled)]);
        // Back in the trip's own garden that way, they're put back with nothing to say.
        let mut t = Trip::new(1, [0.0; 3], 0, foe(), Battled::Own);
        t.frame(at(KARATE));
        t.frame(loading(1));
        assert_eq!(
            t.frame(at(1)),
            [Do::PutBack(Some(Ending::Cancelled)), Do::Done]
        );
    }

    #[test]
    fn a_trip_that_never_gets_there_is_cancelled() {
        let mut t = trip();
        for _ in 0..GOING_LIMIT {
            assert!(t.frame(at(2)).is_empty());
        }
        assert_eq!(t.frame(at(2)), [Do::Report(Ending::Cancelled), Do::Done]);
        // Loading frames don't count.
        let mut t = trip();
        for _ in 0..GOING_LIMIT * 2 {
            assert!(t.frame(loading(2)).is_empty());
        }
        // Somewhere else: over.
        assert_eq!(t.frame(at(7)), [Do::Report(Ending::Cancelled), Do::Done]);
    }

    #[test]
    fn the_menu_must_open_and_the_way_back_end() {
        let mut t = trip();
        t.frame(at(KARATE));
        for _ in 0..OPEN_LIMIT {
            t.frame(at(KARATE));
        }
        assert!(!t.open_overdue());
        t.frame(at(KARATE));
        assert!(t.open_overdue());
        t.opened();
        assert!(!t.open_overdue());
        t.ended(Some(Ending::Won));
        // Stuck on the way back: given up on (nothing more to report).
        for _ in 0..BACK_LIMIT {
            assert!(t.frame(at(KARATE)).is_empty());
        }
        assert_eq!(t.frame(at(KARATE)), [Do::Done]);
    }

    #[test]
    fn a_long_trip_counts_as_given_up() {
        let mut t = trip();
        t.frame(at(KARATE));
        t.opened();
        t.fought();
        let mut reports = t.frame(at(KARATE));
        assert!(reports.is_empty());
        // Paused or not, by the clock.
        let Some(long_ago) = Instant::now().checked_sub(TRIP_LIMIT + Duration::from_secs(1)) else {
            return;
        };
        t.since = long_ago;
        assert!(t.overdue());
        for _ in 0..3 {
            reports.extend(t.frame(Here {
                area: KARATE,
                settled: false,
            }));
        }
        assert_eq!(reports, [Do::Report(Ending::RanAway)]);
        // The battle still ends and they go back, without a second report.
        let (todo, back) = t.ended(Some(Ending::Won));
        assert!(todo.is_empty());
        assert_eq!(back, Some(2));
    }
}

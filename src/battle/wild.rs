//! A reserved wild visitor battles in an independently owned scene in its existing garden.
//! No Chao-area transition or Karate/DLC state is used. The host reservation and exactly-once
//! result stay active while the picker/fight runs, then input/camera return to the same place.

// The trip only runs in the 32-bit game build.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::time::{Duration, Instant};

use crate::creature_link::store::Partner;
use crate::net::protocol::WildStep;

use super::data::MoveData;
use super::fighter::Fighter;
use super::team::Dex;
use super::trainers::{self, Creature};

/// A visitor fights with this IV in every stat and a neutral nature, as its card shows it.
pub const IV: u8 = 15;
/// Frames (of running game) the owned scene gets to come up after leaving the garden.
const GOING_LIMIT: u32 = 30 * 60;
/// Frames in the owned scene before the battle menu must have opened.
const OPEN_LIMIT: u32 = 30 * 60;
/// Frames the way back to the garden may take.
const BACK_LIMIT: u32 = 60 * 60;
/// A trip ends as given up this long after it set off, paused or not: a minute inside the host's
/// own limit for a reservation, so the game always gives up first.
const TRIP_LIMIT: Duration = Duration::from_secs(14 * 60);

/// A wild visitor to battle, as ids from the catalog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WildFoe {
    pub form: u8,
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
            form: partner.form,
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
    exp_yield: u16,
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
        form: foe.form,
        species: foe.species,
        exp_yield,
        fighter: Fighter::new(name, level, stats, types, foe.ability, &moves),
    })
}

/// `foe` as a fighter from the catalog.
pub fn creature(rom: &crate::creature::rom::Rom, dex: &Dex, foe: &WildFoe) -> Option<Creature> {
    if !super::source_rules::ability_supported(foe.ability) {
        return None;
    }
    let s = rom.species_form(crate::creature::data::SpeciesKey {
        species: foe.species,
        form: foe.form,
    })?;
    let name = crate::creature_link::ascii(&rom.species_name(foe.species)?);
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
    /// One of the player's own (a garden they have alone; `creature_link::wild` keeps the visit).
    Own,
    /// The shared visitor with this host id, reserved for the player.
    Shared(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Preparing the owned scene without unloading the garden.
    Going,
    /// In the owned scene (the battle menu opened, or about to).
    Scene { opened: bool },
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

/// A trip from a garden to the owned scene and back.
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
        let origin = here.area == i32::from(self.origin);
        match self.phase {
            Phase::Going if origin && here.settled => {
                self.set_phase(Phase::Scene { opened: false })
            }
            Phase::Going => {
                // It never got there: still in the garden, or somewhere else.
                let elsewhere = here.settled && !origin;
                if elsewhere || self.frames > GOING_LIMIT {
                    self.report(Ending::Cancelled, &mut out);
                    out.push(Do::Done);
                }
            }
            Phase::Scene { .. } if !origin => {
                // Left the owned scene some other way (the pause menu's exit).
                self.report(Ending::cut_short(self.fought), &mut out);
                self.set_phase(Phase::Back);
            }
            Phase::Scene { .. } => {}
            Phase::Back if here.settled && origin => {
                out.push(Do::PutBack(self.ending));
                out.push(Do::Done);
            }
            Phase::Back => {
                let other_garden = here.settled && (1..=3).contains(&here.area) && !origin;
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

    /// The battle menu waits for input and the battle menu hasn't opened for the trip yet.
    pub fn wants_menu(&self) -> bool {
        self.phase == Phase::Scene { opened: false }
    }

    /// The battle menu opened.
    pub fn opened(&mut self) {
        if let Phase::Scene { .. } = self.phase {
            self.phase = Phase::Scene { opened: true };
        }
    }

    /// The menu didn't open in time.
    pub fn open_overdue(&self) -> bool {
        self.phase == Phase::Scene { opened: false } && self.frames > OPEN_LIMIT
    }

    /// The fight began (the arena opened for it).
    pub fn fought(&mut self) {
        self.fought = true;
    }

    /// In the owned scene for the battle (the menu open or about to).
    pub fn in_scene(&self) -> bool {
        matches!(self.phase, Phase::Scene { .. })
    }

    /// The battle menu ended with `ending` (none: it didn't say). In the owned scene the trip
    /// goes back now: what to report (once), and the garden to go back to.
    pub fn ended(&mut self, ending: Option<Ending>) -> (Vec<Do>, Option<u8>) {
        let mut out = Vec::new();
        if !self.in_scene() {
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

    /// The player's EntityData1 +0x0C: facing (BAMS).
    const ENTITY_ROTATION_Y: usize = 0x0C;

    static TRIP: Mutex<Option<Trip>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut Option<Trip>) -> R) -> R {
        f(&mut TRIP.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Sets off on `trip` from the garden: false when one is underway already.
    pub fn start(trip: Trip) -> bool {
        if underway() || !super::super::arena::available() {
            return false;
        }
        log!(
            "battle: opening a local scene to battle a wild visitor (species {}, level {})",
            trip.foe.species,
            trip.foe.level
        );
        with(|t| *t = Some(trip));
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
    /// to, when the trip is in the owned scene.
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
        crate::creature_link::wild_battle::over(battled, ending);
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
        let name = crate::creature_link::rom()
            .ok()
            .and_then(|rom| rom.species_name(trip.foe.species))
            .map(|n| crate::creature_link::ascii(&n))
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
            form: 0,
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
        let base = [50, 60, 70, 80, 90, 100];
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
    fn the_foe_is_what_the_card_shows() {
        let partner = Partner {
            form: 0,
            species: 25,
            types: [12, 12],
            moves: [4, 5, 0, 0],
            level: 9,
            ability: 8,
        };
        let f = WildFoe::of(&partner, false);
        assert_eq!(
            f,
            WildFoe {
                form: 0,
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
    fn a_local_scene_opens_and_returns_without_an_area_transition() {
        let mut t = trip();
        assert!(t.frame(at(2)).is_empty());
        assert!(t.in_scene() && t.wants_menu());
        t.opened();
        t.fought();
        assert!(!t.wants_menu());
        let (todo, origin) = t.ended(Some(Ending::Won));
        assert_eq!(todo, [Do::Report(Ending::Won)]);
        assert_eq!(origin, Some(2));
        assert_eq!(t.ended(Some(Ending::RanAway)), (Vec::new(), None));
        assert_eq!(t.frame(at(2)), [Do::PutBack(Some(Ending::Won)), Do::Done]);
    }
    #[test]
    fn cancellation_reports_once_and_never_repositions_into_a_different_garden() {
        let mut t = trip();
        t.frame(at(2));
        t.opened();
        assert_eq!(t.frame(loading(1)), [Do::Report(Ending::Cancelled)]);
        assert_eq!(t.frame(at(1)), [Do::Done]);
        let mut t = trip();
        t.frame(at(2));
        t.opened();
        t.fought();
        assert_eq!(t.frame(loading(1)), [Do::Report(Ending::RanAway)]);
        assert_eq!(t.frame(at(1)), [Do::Done]);
    }
    #[test]
    fn an_unopened_scene_times_out_without_loading_or_pausing_counting() {
        let mut t = trip();
        for _ in 0..GOING_LIMIT * 2 {
            assert!(t.frame(loading(2)).is_empty());
        }
        assert_eq!(t.frame(at(7)), [Do::Report(Ending::Cancelled), Do::Done]);
        let mut t = trip();
        t.frame(at(2));
        for _ in 0..OPEN_LIMIT {
            t.frame(at(2));
        }
        assert!(!t.open_overdue());
        t.frame(at(2));
        assert!(t.open_overdue());
        t.opened();
        assert!(!t.open_overdue());
    }
    #[test]
    fn closing_before_or_after_a_fight_preserves_the_result_policy() {
        for fought in [false, true] {
            let mut t = trip();
            t.frame(at(2));
            t.opened();
            if fought {
                t.fought();
            }
            let expected = Ending::cut_short(fought);
            assert_eq!(t.ended(None).0, [Do::Report(expected)]);
            assert_eq!(t.frame(at(2)), [Do::PutBack(Some(expected)), Do::Done]);
        }
    }
    #[test]
    fn the_clock_limit_releases_a_reservation_even_while_paused() {
        let mut t = trip();
        t.frame(at(2));
        t.opened();
        t.fought();
        t.since = Instant::now()
            .checked_sub(TRIP_LIMIT + Duration::from_secs(1))
            .unwrap();
        let mut reports = Vec::new();
        for _ in 0..3 {
            reports.extend(t.frame(loading(2)));
        }
        assert_eq!(reports, [Do::Report(Ending::RanAway)]);
        assert!(t.ended(Some(Ending::Won)).0.is_empty());
    }
}

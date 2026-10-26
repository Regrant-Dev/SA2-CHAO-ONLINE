//! The Black Market restocks every 10 minutes of play. Native evidence: re/black-market.md.
//!
//! The stock is only ever rebuilt by the market itself: when it opens (mode 2 of 0x58B5E0) it
//! restocks if its timer 0x1DBED8C has reached `FrameCount`. Every 10 minutes of unpaused play
//! the timer is set to now, so the next visit restocks through the game's own path (stock
//! generator, the jewel egg offer, special item 2, a new timer); in between, the timer is held
//! ahead so the game's own 3 to 7 minute schedule never restocks. Nothing but that timer is
//! written, with values the game itself writes there, and never while the market is open.
//!
//! Play toward the next mark and an earned restock not yet had are kept in the mod's folder
//! (`market-refresh.txt`), so they carry over to the next session.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};

/// Unpaused play between restocks.
const INTERVAL_MS: u64 = 10 * 60 * 1000;
/// Longest stretch one call counts (a load or a stall counts at most this much).
const MAX_STEP_MS: u64 = 250;
/// The furthest ahead (in `FrameCount` steps) mode 2 keeps the timer without resetting it.
const HOLD_STEPS: u32 = 25200;
/// The held timer is pushed ahead again once it's closer than this.
const MARGIN_STEPS: u32 = 3600;

static ENABLED: AtomicBool = AtomicBool::new(false);
static LOGGED: AtomicBool = AtomicBool::new(false);

/// `BlackMarketRefresh` in config.ini (read again each time the F9 menu opens: logged once and
/// when it changes).
pub fn set_enabled(on: bool) {
    let was = ENABLED.swap(on, Ordering::Relaxed);
    if was == on && LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    crate::log!(
        "market refresh: {}",
        if on {
            "the Black Market restocks every 10 minutes of play"
        } else {
            "off (the game's own restock timer)"
        }
    );
}

/// What the game shows this frame.
#[derive(Clone, Copy, Debug)]
struct View {
    /// In Chao World, where the market's data is loaded (0x52EE60 at its start; a value
    /// written before that is loaded over and written again).
    chao_world: bool,
    /// The Kindergarten's market room is open (or being opened or left).
    market_open: bool,
    /// The market's next restock, a `FrameCount` value (0x1DBED8C).
    next: u32,
    /// `FrameCount` (0x174B038).
    now: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    /// A restock became due; the timer was set to now.
    Armed,
    /// The market restocked on a visit after being armed.
    Restocked,
    /// Both: it restocked, and a mark passed during that visit armed the next one.
    RestockedAndArmed,
}

#[derive(Default, Debug)]
struct Refresh {
    played_ms: u64,
    /// 10 minutes have passed since the last restock was armed.
    due: bool,
    /// The timer was set to now and the market hasn't restocked yet.
    armed: bool,
    /// While armed, the market was opened.
    visited: bool,
}

impl Refresh {
    const fn new() -> Self {
        Self {
            played_ms: 0,
            due: false,
            armed: false,
            visited: false,
        }
    }

    /// One frame: counts `elapsed_ms` of play unless paused, then `apply`. Switched off, it
    /// starts over and writes nothing, which leaves the game's own timer to run.
    fn step(
        &mut self,
        enabled: bool,
        elapsed_ms: u64,
        paused: bool,
        view: View,
    ) -> (Option<u32>, Option<Event>) {
        if !enabled {
            *self = Self::new();
            return (None, None);
        }
        self.advance(elapsed_ms, paused);
        self.apply(view)
    }

    /// What carries over to the next session: play toward the next mark, and whether a
    /// restock is earned but not yet had (due, or armed and not yet visited).
    fn kept(&self) -> Kept {
        Kept {
            played_ms: self.played_ms,
            pending: self.due || (self.armed && !self.visited),
        }
    }

    /// Picks up from an earlier session: an earned restock is armed on the first Chao World
    /// frame.
    fn restore(&mut self, kept: Kept) {
        self.played_ms = kept.played_ms.min(INTERVAL_MS - 1);
        self.due = kept.pending;
    }

    fn advance(&mut self, elapsed_ms: u64, paused: bool) {
        if paused {
            return;
        }
        self.played_ms += elapsed_ms.min(MAX_STEP_MS);
        if self.played_ms >= INTERVAL_MS {
            self.played_ms -= INTERVAL_MS;
            // A restock still waiting for its visit covers this mark too.
            if !self.armed || self.visited {
                self.due = true;
            }
        }
    }

    /// The timer value to write this frame, if any, and what happened.
    fn apply(&mut self, view: View) -> (Option<u32>, Option<Event>) {
        if !view.chao_world {
            return (None, None);
        }
        if view.market_open {
            // The market's stock and selection are in use: nothing is written until it closes.
            self.visited |= self.armed;
            return (None, None);
        }
        let ahead = view.next.wrapping_sub(view.now);
        // As the market compares: `next > now` (unsigned).
        let future = view.next > view.now;
        let mut event = None;
        if self.armed {
            if !future {
                // Still waiting for a visit (or the visit was left before it restocked).
                self.visited = false;
                return (None, None);
            }
            if !self.visited {
                // Reloaded from the save before a visit: set it to now again.
                return (Some(view.now), None);
            }
            self.armed = false;
            self.visited = false;
            event = Some(Event::Restocked);
        }
        if self.due {
            self.due = false;
            self.armed = true;
            self.visited = false;
            let armed = if event.is_some() {
                Event::RestockedAndArmed
            } else {
                Event::Armed
            };
            return (Some(view.now), Some(armed));
        }
        if !(MARGIN_STEPS..=HOLD_STEPS).contains(&ahead) {
            return (Some(view.now.wrapping_add(HOLD_STEPS)), event);
        }
        (None, event)
    }
}

/// Logic steps a second (`FrameCount` counts 60 a second of play).
const STEPS_PER_SECOND: u32 = 60;

impl Refresh {
    /// Milliseconds of play to the next restock (`enabled`: ours, else the game's timer in
    /// `view`); `None` when the next visit restocks.
    fn until_restock(&self, enabled: bool, view: View) -> Option<u64> {
        if enabled {
            if self.due || self.armed {
                return None;
            }
            return Some(INTERVAL_MS - self.played_ms.min(INTERVAL_MS - 1));
        }
        (view.next > view.now)
            .then(|| u64::from(view.next - view.now) * 1000 / u64::from(STEPS_PER_SECOND))
    }
}

/// The market's line about the next restock.
fn restock_line(until: Option<u64>) -> String {
    match until {
        None => "New stock on your next visit".into(),
        Some(ms) => {
            let s = ms.div_ceil(1000);
            format!("Next restock in {}:{:02}", s / 60, s % 60)
        }
    }
}

/// The state kept between sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Kept {
    played_ms: u64,
    pending: bool,
}

impl Kept {
    const FILE: &str = "market-refresh.txt";

    fn parse(text: &str) -> Self {
        let mut kept = Self::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("played_ms", v)) => kept.played_ms = v.trim().parse().unwrap_or(0),
                Some(("pending", v)) => kept.pending = v.trim() == "1",
                _ => {}
            }
        }
        kept
    }

    fn to_text(self) -> String {
        format!(
            "played_ms={}\npending={}\n",
            self.played_ms,
            u8::from(self.pending)
        )
    }
}

/// The kept state is written at most this often while play counts, and whenever an earned
/// restock starts or stops waiting.
const KEEP_EVERY_MS: u64 = 60 * 1000;

#[cfg(target_arch = "x86")]
mod native {
    use super::*;
    use crate::game;
    use std::sync::Mutex;

    const NEXT_RESTOCK: usize = 0x01DB_ED8C;
    const STOCK_COUNT: usize = 0x01DB_ED90;
    const FRAME_COUNT: usize = 0x0174_B038;
    /// The Kindergarten's room (switch 0x54E280); 2 is the Black Market.
    const KINDERGARTEN_ROOM: usize = 0x01AE_D238;
    const AREA_KINDERGARTEN: i32 = 6;
    const ROOM_MARKET: u32 = 2;
    const GAME_STATE_PAUSED: i16 = 17;

    struct State {
        refresh: Refresh,
        last: Option<std::time::Instant>,
        /// What the file holds (`None` until it is read).
        kept: Option<Kept>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        refresh: Refresh::new(),
        last: None,
        kept: None,
    });

    fn path() -> std::path::PathBuf {
        std::path::Path::new(crate::online::mod_path()).join(Kept::FILE)
    }

    /// Reads the kept state once (none yet, or unreadable: a fresh start).
    fn load(state: &mut State) {
        let kept = match std::fs::read(path()) {
            Ok(bytes) => Kept::parse(&String::from_utf8_lossy(&bytes)),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    crate::log!("market refresh: can't read {}: {e}", Kept::FILE);
                }
                Kept::default()
            }
        };
        state.refresh.restore(kept);
        if kept.pending {
            crate::log!("market refresh: a restock from last time waits for the next visit");
        }
        state.kept = Some(state.refresh.kept());
    }

    /// Writes the kept state when its restock flag changed or after `KEEP_EVERY_MS` of play.
    fn keep(state: &mut State) {
        let now = state.refresh.kept();
        let Some(was) = state.kept else {
            return;
        };
        let stale = now.played_ms.abs_diff(was.played_ms) >= KEEP_EVERY_MS;
        if now.pending == was.pending && !stale {
            return;
        }
        state.kept = Some(now);
        if let Err(e) = crate::battle::ladder::write(&path(), &now.to_text()) {
            crate::log!("market refresh: can't write {}: {e}", Kept::FILE);
        }
    }

    pub fn on_frame() {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let enabled = ENABLED.load(Ordering::Relaxed);
        if enabled && state.kept.is_none() {
            load(&mut state);
        }
        let now = std::time::Instant::now();
        let elapsed = state
            .last
            .map_or(0, |last| now.duration_since(last).as_millis() as u64);
        state.last = Some(now);
        // Not `between_areas`: the market room may have no player.
        let chao_world = game::current_level() == game::LEVEL_CHAO_WORLD;
        // SAFETY: static game variables in the exe's data section, read on the game thread.
        let view = unsafe {
            View {
                chao_world,
                market_open: chao_world
                    && game::current_chao_area() == AREA_KINDERGARTEN
                    && (KINDERGARTEN_ROOM as *const u32).read_volatile() == ROOM_MARKET,
                next: (NEXT_RESTOCK as *const u32).read_volatile(),
                now: (FRAME_COUNT as *const u32).read_volatile(),
            }
        };
        let (write, event) = state.refresh.step(
            enabled,
            elapsed,
            game::game_state() == GAME_STATE_PAUSED,
            view,
        );
        if let Some(next) = write {
            // SAFETY: the market's own timer, which only the game thread reads and writes.
            unsafe { (NEXT_RESTOCK as *mut u32).write_volatile(next) };
        }
        if matches!(event, Some(Event::Restocked | Event::RestockedAndArmed)) {
            crate::log!(
                "market refresh: the Black Market restocked ({} offers)",
                // SAFETY: as above.
                unsafe { (STOCK_COUNT as *const u32).read_volatile() }
            );
        }
        if matches!(event, Some(Event::Armed | Event::RestockedAndArmed)) {
            crate::log!("market refresh: 10 minutes played; the next market visit restocks");
        }
        if enabled {
            keep(&mut state);
        }
        if view.market_open && game::game_state() != GAME_STATE_PAUSED {
            let until = state.refresh.until_restock(enabled, view);
            crate::hud::corner(&restock_line(until));
        }
    }
}

/// Called every frame (lib.rs OnFrame, which also runs while paused).
pub fn on_frame() {
    #[cfg(target_arch = "x86")]
    native::on_frame();
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60 * 1000;

    fn view(next: u32, now: u32) -> View {
        View {
            chao_world: true,
            market_open: false,
            next,
            now,
        }
    }

    fn play(r: &mut Refresh, ms: u64, paused: bool) {
        for _ in 0..ms / 16 {
            r.advance(16, paused);
        }
    }

    #[test]
    fn due_every_ten_minutes_of_unpaused_play() {
        let mut r = Refresh::default();
        play(&mut r, 9 * MINUTE, false);
        assert!(!r.due);
        play(&mut r, 30 * MINUTE, true);
        assert!(!r.due, "paused time doesn't count");
        play(&mut r, MINUTE + 100, false);
        assert!(r.due);
        // The remainder carries over, so the schedule stays steady.
        assert!(r.played_ms < 200);
        // A stall counts only a short step.
        let mut r = Refresh::default();
        r.advance(20 * MINUTE, false);
        assert_eq!(r.played_ms, MAX_STEP_MS);
    }

    #[test]
    fn holds_the_game_timer_until_due_then_arms_it() {
        let mut r = Refresh::default();
        // Expired, close, or from an earlier session: held at the furthest the market keeps.
        for next in [500, 1000, 1100, 50_000] {
            assert_eq!(r.apply(view(next, 1000)), (Some(1000 + HOLD_STEPS), None));
        }
        // A held timer is left alone.
        assert_eq!(r.apply(view(1000 + 20_000, 1000)), (None, None));
        assert_eq!(r.apply(view(1000 + HOLD_STEPS, 1000)), (None, None));
        r.due = true;
        assert_eq!(r.apply(view(5000, 1000)), (Some(1000), Some(Event::Armed)));
        // Armed: not touched while it waits for a visit.
        assert_eq!(r.apply(view(1000, 4000)), (None, None));
    }

    #[test]
    fn restock_waits_until_the_market_closes() {
        let mut r = Refresh {
            due: true,
            ..Refresh::default()
        };
        let open = View {
            market_open: true,
            ..view(9000, 1000)
        };
        assert_eq!(r.apply(open), (None, None));
        assert!(r.due && !r.armed, "deferred while open");
        // Closed: armed. The next visit's mode 2 restocks and sets a new timer.
        assert_eq!(r.apply(view(9000, 1000)), (Some(1000), Some(Event::Armed)));
        assert_eq!(r.apply(View { now: 1020, ..open }), (None, None));
        let after = view(1020 + 12_000, 1100);
        assert_eq!(r.apply(after), (None, Some(Event::Restocked)));
        assert!(!r.armed);
        // Then held again as usual.
        assert_eq!(r.apply(view(1020 + 12_000, 1020 + 11_000)).1, None);
        assert_eq!(
            r.apply(view(1020 + 12_000, 1020 + 11_000)).0,
            Some(1020 + 11_000 + HOLD_STEPS)
        );
    }

    #[test]
    fn armed_timer_survives_a_reload_and_waits_outside_chao_world() {
        let mut r = Refresh {
            due: true,
            ..Refresh::default()
        };
        let away = View {
            chao_world: false,
            ..view(0, 1000)
        };
        assert_eq!(r.apply(away), (None, None));
        assert!(r.due, "waits for Chao World");
        assert_eq!(r.apply(view(9000, 2000)).0, Some(2000));
        // The save's value came back before any visit: armed again.
        assert_eq!(r.apply(view(30_000, 2500)), (Some(2500), None));
        assert!(r.armed);
    }

    #[test]
    fn a_mark_while_armed_and_unvisited_is_merged() {
        let mut r = Refresh::default();
        play(&mut r, 10 * MINUTE + 100, false);
        assert_eq!(r.apply(view(9000, 1000)).1, Some(Event::Armed));
        // No visit for another 10 minutes: still one restock waiting.
        play(&mut r, 10 * MINUTE, false);
        assert!(!r.due && r.armed);
    }

    #[test]
    fn a_mark_during_a_visit_logs_the_restock_and_arms_the_next() {
        let mut r = Refresh::default();
        play(&mut r, 10 * MINUTE + 100, false);
        assert_eq!(r.apply(view(9000, 1000)).1, Some(Event::Armed));
        let open = View {
            market_open: true,
            ..view(1000, 1100)
        };
        assert_eq!(r.apply(open), (None, None));
        // Mode 2 restocked; the shop stays open past the next mark.
        play(&mut r, 10 * MINUTE, false);
        assert!(r.due);
        assert_eq!(
            r.apply(view(1100 + 12_000, 1200)),
            (Some(1200), Some(Event::RestockedAndArmed))
        );
        assert!(r.armed && !r.due);
    }

    #[test]
    fn play_and_an_earned_restock_carry_over_to_the_next_session() {
        let mut r = Refresh::default();
        play(&mut r, 8 * MINUTE, false);
        let kept = r.kept();
        assert_eq!(kept, Kept::parse(&kept.to_text()));
        assert!(!kept.pending);
        // Two short sessions add up to a mark.
        let mut next = Refresh::default();
        next.restore(kept);
        play(&mut next, 2 * MINUTE + 100, false);
        assert!(next.due);
        // Armed but never visited: the next session arms it again on its first frame.
        assert_eq!(next.apply(view(9000, 1000)).1, Some(Event::Armed));
        let kept = next.kept();
        assert!(kept.pending);
        let mut third = Refresh::default();
        third.restore(Kept::parse(&kept.to_text()));
        assert_eq!(
            third.apply(view(500_000, 40)),
            (Some(40), Some(Event::Armed))
        );
        // Once visited (restocked), nothing is waiting any more.
        let open = View {
            market_open: true,
            ..view(40, 60)
        };
        third.apply(open);
        assert!(!third.kept().pending);
        // Junk and a missing file are a fresh start.
        assert_eq!(Kept::parse("played_ms=x\nother"), Kept::default());
        let mut r = Refresh::default();
        r.restore(Kept {
            played_ms: u64::MAX,
            pending: false,
        });
        assert!(r.played_ms < INTERVAL_MS);
    }

    #[test]
    fn the_market_shows_the_time_to_the_next_restock() {
        // The game's timer: 3 minutes 30 seconds of steps ahead.
        let r = Refresh::new();
        let until = r.until_restock(false, view(1000 + 210 * 60, 1000));
        assert_eq!(restock_line(until), "Next restock in 3:30");
        // Run out (or reset to now): the next visit restocks.
        assert_eq!(r.until_restock(false, view(1000, 1000)), None);
        assert_eq!(restock_line(None), "New stock on your next visit");
        // Ours: what's left of the 10 minutes, whatever the game's timer says.
        let mut r = Refresh::new();
        play(&mut r, MINUTE, false);
        let until = r.until_restock(true, view(1000 + 25200, 1000)).unwrap();
        assert!((9 * MINUTE - 100..=9 * MINUTE).contains(&until), "{until}");
        assert_eq!(restock_line(Some(9 * MINUTE)), "Next restock in 9:00");
        assert_eq!(restock_line(Some(1)), "Next restock in 0:01");
        play(&mut r, 9 * MINUTE + 100, false);
        assert_eq!(r.until_restock(true, view(1000 + 25200, 1000)), None);
    }

    #[test]
    fn switched_off_writes_nothing_and_starts_over() {
        let mut r = Refresh::default();
        play(&mut r, 10 * MINUTE + 100, false);
        assert_eq!(r.apply(view(9000, 1000)).1, Some(Event::Armed));
        // Whatever the timer is, the game's own schedule runs (a held value is at most the
        // 7 minutes ahead the game itself sets).
        for next in [0, 1000, 1000 + HOLD_STEPS, 90_000] {
            assert_eq!(
                r.step(false, 20 * MINUTE, false, view(next, 1000)),
                (None, None)
            );
        }
        assert_eq!(r.played_ms, 0);
        assert!(!r.due && !r.armed);
        const { assert!(HOLD_STEPS <= 7 * 60 * 60) };
        // Back on: counting starts again and nothing is due yet.
        assert_eq!(
            r.step(true, 16, false, view(1000 + 20_000, 1000)),
            (None, None)
        );
    }
}

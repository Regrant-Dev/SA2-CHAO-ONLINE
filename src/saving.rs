//! Manual and five-minute Chao garden saves, on the game thread.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{game, hud, log};

const INTERVAL: Duration = Duration::from_secs(5 * 60);
const RETRY: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Clock {
    previous: Option<Instant>,
    was_in_garden: bool,
    elapsed: Duration,
    safe_since: Option<Instant>,
    attempted: Option<Instant>,
}

impl Clock {
    fn due(&mut self, now: Instant, in_garden: bool, safe: bool) -> bool {
        if self.was_in_garden
            && in_garden
            && let Some(previous) = self.previous
        {
            self.elapsed += now.saturating_duration_since(previous);
        }
        self.previous = Some(now);
        self.was_in_garden = in_garden;
        if safe {
            self.safe_since.get_or_insert(now);
        } else {
            self.safe_since = None;
        }
        let settled = self
            .safe_since
            .is_some_and(|at| now.duration_since(at) >= SETTLE);
        let retry = self
            .attempted
            .is_none_or(|at| now.duration_since(at) >= RETRY);
        if in_garden && safe && settled && retry && self.elapsed >= INTERVAL {
            self.attempted = Some(now);
            true
        } else {
            false
        }
    }

    fn saved(&mut self, now: Instant) {
        self.elapsed = Duration::ZERO;
        self.previous = Some(now);
        self.attempted = Some(now);
    }
}

static CLOCK: Mutex<Option<Clock>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Clock) -> R) -> R {
    let mut clock = CLOCK.lock().unwrap_or_else(|e| e.into_inner());
    f(clock.get_or_insert_with(Clock::default))
}

/// F9 action. Save only the player's Chao World data, through the game's own save path.
pub fn save_now() -> String {
    match save("manual") {
        Ok(()) => "Chao garden saved".into(),
        Err(error) => format!("Can't save Chao garden: {error}"),
    }
}

fn save(reason: &str) -> Result<(), String> {
    game::save_chao()?;
    with(|clock| clock.saved(Instant::now()));
    log!("save: Chao garden saved ({reason})");
    Ok(())
}

/// Call after online changes have been applied, before the F9 menu, on the game thread.
pub fn on_frame() {
    let now = Instant::now();
    let in_garden = game::in_garden();
    let safe =
        in_garden && matches!(game::game_state(), 16 | 17) && !game::local_entity().is_null();
    if with(|clock| clock.due(now, in_garden, safe)) {
        match save("five-minute autosave") {
            Ok(()) => hud::toast("Chao garden autosaved"),
            Err(error) => {
                log!("save: autosave failed: {error}; retrying in 30 seconds");
                hud::toast(format!("Chao autosave failed: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_minutes_in_gardens_triggers_one_save() {
        let now = Instant::now();
        let mut clock = Clock::default();
        assert!(!clock.due(now, true, true));
        assert!(!clock.due(now + INTERVAL - Duration::from_secs(1), true, true));
        assert!(clock.due(now + INTERVAL, true, true));
        clock.saved(now + INTERVAL);
        assert!(!clock.due(now + INTERVAL + Duration::from_secs(1), true, true));
    }

    #[test]
    fn time_outside_gardens_does_not_count_and_loading_defers_save() {
        let now = Instant::now();
        let mut clock = Clock::default();
        assert!(!clock.due(now, false, false));
        assert!(!clock.due(now + INTERVAL * 4, true, false));
        assert!(!clock.due(now + INTERVAL * 5, true, false));
        assert!(!clock.due(now + INTERVAL * 5 + SETTLE, true, true));
        assert!(clock.due(now + INTERVAL * 5 + SETTLE * 2, true, true));
    }

    #[test]
    fn manual_save_resets_autosave_and_failures_wait_before_retry() {
        let now = Instant::now();
        let mut clock = Clock::default();
        clock.due(now, true, true);
        assert!(clock.due(now + INTERVAL, true, true));
        assert!(!clock.due(now + INTERVAL + RETRY - Duration::from_secs(1), true, true));
        assert!(clock.due(now + INTERVAL + RETRY, true, true));
        clock.saved(now + INTERVAL + RETRY);
        assert!(!clock.due(now + INTERVAL + RETRY + Duration::from_secs(1), true, true));
    }
}

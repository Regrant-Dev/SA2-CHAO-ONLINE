//! Watchdog for freezes: the game thread marks which step it's in; a background thread logs the
//! step when no new mark comes for a while (a deadlock, or the game itself stuck).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use crate::log;

static BEAT: AtomicU32 = AtomicU32::new(0);
static STEP_PTR: AtomicUsize = AtomicUsize::new(0);
static STEP_LEN: AtomicUsize = AtomicUsize::new(0);

/// Logged when the game thread hasn't moved on for this long (loading screens and videos can
/// do this too).
const STUCK: Duration = Duration::from_secs(8);

/// Game thread: about to do `step`.
pub fn mark(step: &'static str) {
    STEP_LEN.store(0, Ordering::Relaxed);
    STEP_PTR.store(step.as_ptr() as usize, Ordering::Relaxed);
    STEP_LEN.store(step.len(), Ordering::Relaxed);
    BEAT.fetch_add(1, Ordering::Relaxed);
}

fn step() -> &'static str {
    let (ptr, len) = (
        STEP_PTR.load(Ordering::Relaxed),
        STEP_LEN.load(Ordering::Relaxed),
    );
    if ptr == 0 || len == 0 {
        return "?";
    }
    // SAFETY: `mark` only stores `&'static str`s; a torn read shows "?" (len 0) or a prefix.
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(ptr as *const u8, len) }).unwrap_or("?")
}

/// Starts the watchdog thread.
pub fn start() {
    let _ = std::thread::Builder::new()
        .name("sa2co-watchdog".into())
        .spawn(|| {
            let mut last = BEAT.load(Ordering::Relaxed);
            let mut still = Duration::ZERO;
            let mut reported = false;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let beat = BEAT.load(Ordering::Relaxed);
                if beat != last {
                    if reported {
                        log!("watchdog: the game thread moved on again");
                    }
                    (last, still, reported) = (beat, Duration::ZERO, false);
                    continue;
                }
                still += Duration::from_secs(1);
                if still >= STUCK && !reported && beat != 0 {
                    reported = true;
                    log!(
                        "watchdog: game thread hasn't moved for {}s, last at: {}",
                        still.as_secs(),
                        step()
                    );
                }
            }
        });
}

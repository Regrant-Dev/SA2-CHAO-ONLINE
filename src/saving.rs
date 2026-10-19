//! Manual and five-minute Chao garden saves, on the game thread.

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{game, hud, log};

const INTERVAL: Duration = Duration::from_secs(5 * 60);
const RETRY: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_secs(2);
/// The Steam save: unchanged 0x3040-byte header/icon region, 0xCA6C-byte Chao payload,
/// then padding. This is the layout built by the game's 0x532360.
const FILE_SIZE: usize = 0x10000;
const PAYLOAD_START: usize = 0x3040;
const PAYLOAD_SIZE: usize = 0xCA6C;

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
    let path = game::chao_save_path().ok_or("the Chao save path is unavailable")?;
    let snapshot = game::chao_save_snapshot()?;
    write_snapshot(path, &snapshot)?;
    with(|clock| clock.saved(Instant::now()));
    log!("save: Chao garden saved ({reason}) to {}", path.display());
    Ok(())
}

/// Keep the existing Steam header and padding, replacing only the game's checksummed region.
/// Complete a same-directory temporary file before atomically replacing the existing save.
fn write_snapshot(path: &Path, snapshot: &[u8]) -> Result<(), String> {
    if snapshot.len() != PAYLOAD_SIZE {
        return Err("the Chao save snapshot has an unexpected size".into());
    }
    let mut file = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if file.len() != FILE_SIZE {
        return Err(format!(
            "{} has an unexpected save format; left unchanged",
            path.display()
        ));
    }
    file[PAYLOAD_START..PAYLOAD_START + PAYLOAD_SIZE].copy_from_slice(snapshot);
    let name = path
        .file_name()
        .ok_or("invalid Chao save path")?
        .to_string_lossy();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temporary =
        path.with_file_name(format!(".{name}.sa2co-{}-{nonce}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        output.write_all(&file)?;
        output.sync_all()?;
        drop(output);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
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

    fn temporary_save() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("sa2-save-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        directory.join("SONIC2B__ALF")
    }

    #[test]
    fn saving_preserves_the_steam_header_and_padding_and_replaces_all_chao_data() {
        let path = temporary_save();
        let original: Vec<u8> = (0..FILE_SIZE).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &original).unwrap();
        let snapshot = vec![0xA5; PAYLOAD_SIZE];
        write_snapshot(&path, &snapshot).unwrap();
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(saved.len(), FILE_SIZE);
        assert_eq!(&saved[..PAYLOAD_START], &original[..PAYLOAD_START]);
        assert_eq!(
            &saved[PAYLOAD_START..PAYLOAD_START + PAYLOAD_SIZE],
            &snapshot
        );
        assert_eq!(
            &saved[PAYLOAD_START + PAYLOAD_SIZE..],
            &original[PAYLOAD_START + PAYLOAD_SIZE..]
        );
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_unrecognized_file_or_bad_snapshot_never_overwrites_the_save() {
        let path = temporary_save();
        std::fs::write(&path, b"unexpected format").unwrap();
        assert!(write_snapshot(&path, &vec![0; PAYLOAD_SIZE]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"unexpected format");
        let original = vec![0x5A; FILE_SIZE];
        std::fs::write(&path, &original).unwrap();
        assert!(write_snapshot(&path, &[0; 16]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

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

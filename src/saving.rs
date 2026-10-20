//! Manual, five-minute and change-driven Chao World saves, on the game thread. In a garden
//! the game's full pre-save pass runs first; anywhere else only its checksum (game.rs).

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{game, hud, log};

const INTERVAL: Duration = Duration::from_secs(5 * 60);
/// The least time between two automatic saves (or failed attempts).
const RETRY: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_secs(2);
/// After `changed()`, wait this long for more changes before saving (outside a garden).
const DEBOUNCE: Duration = Duration::from_secs(10);
/// The Steam save: unchanged 0x3040-byte header/icon region, 0xCA6C-byte Chao payload,
/// then padding. This is the layout built by the game's 0x532360.
const FILE_SIZE: usize = 0x10000;
const PAYLOAD_START: usize = 0x3040;
const PAYLOAD_SIZE: usize = game::CHAO_SAVE_SIZE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reason {
    Interval,
    Changed,
}

/// What the frame sees: whether a snapshot is possible, in a garden, and where the game is
/// (any change restarts the settle wait).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Frame {
    safe: bool,
    in_garden: bool,
    place: (i32, i16, i32),
}

#[derive(Default)]
struct Clock {
    previous: Option<Instant>,
    was_safe: bool,
    elapsed: Duration,
    place: (i32, i16, i32),
    safe_since: Option<Instant>,
    attempted: Option<Instant>,
    /// First unsaved `changed()` since the last save, and the latest.
    dirty_since: Option<Instant>,
    last_change: Option<Instant>,
}

impl Clock {
    fn due(&mut self, now: Instant, frame: Frame) -> Option<Reason> {
        if self.was_safe
            && frame.safe
            && let Some(previous) = self.previous
        {
            self.elapsed += now.saturating_duration_since(previous);
        }
        self.previous = Some(now);
        self.was_safe = frame.safe;
        if frame.place != self.place {
            self.place = frame.place;
            self.safe_since = None;
        }
        if frame.safe {
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
        if !(frame.safe && settled && retry) {
            return None;
        }
        let reason = if self.elapsed >= INTERVAL {
            Reason::Interval
        } else if !frame.in_garden
            && self
                .dirty_since
                .is_some_and(|at| now.duration_since(at) >= DEBOUNCE)
        {
            Reason::Changed
        } else {
            return None;
        };
        self.attempted = Some(now);
        Some(reason)
    }

    fn changed(&mut self, now: Instant) {
        self.dirty_since.get_or_insert(now);
        self.last_change = Some(now);
    }

    /// A save of the memory as it was at `taken` landed: changes after it are still unsaved.
    fn saved(&mut self, now: Instant, taken: Instant) {
        self.elapsed = Duration::ZERO;
        self.previous = Some(now);
        self.attempted = Some(now);
        match self.last_change {
            Some(at) if at > taken => self.dirty_since = Some(at),
            _ => self.dirty_since = None,
        }
    }
}

static CLOCK: Mutex<Option<Clock>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Clock) -> R) -> R {
    let mut clock = CLOCK.lock().unwrap_or_else(|e| e.into_inner());
    f(clock.get_or_insert_with(Clock::default))
}

/// The Chao save in memory changed outside the game's own saving (e.g. a server write-back
/// while the player is away from the garden). Outside a garden it is saved within ~30 s.
pub fn changed() {
    with(|clock| clock.changed(Instant::now()));
}

/// F9 action. Save only the player's Chao World data, through the game's own save path.
pub fn save_now() -> String {
    match save("manual") {
        Ok(()) => "Chao World saved".into(),
        Err(error) => format!("Can't save Chao World: {error}"),
    }
}

fn save(reason: &str) -> Result<(), String> {
    let path = game::chao_save_path().ok_or("the Chao save path is unavailable")?;
    let taken = Instant::now();
    let snapshot = game::chao_save_snapshot()?;
    if !checksum_ok(&snapshot) {
        return Err("the Chao save checksum doesn't match; left unchanged".into());
    }
    write_snapshot(path, &snapshot)?;
    with(|clock| clock.saved(Instant::now(), taken));
    log!("save: Chao World saved ({reason}) to {}", path.display());
    Ok(())
}

/// The game's checksum table (0x8A6FF8): a CRC-32-style table, but not a polynomial's.
#[rustfmt::skip]
const TABLE: [u32; 256] = [
    0x0000_0000, 0xC907_3096, 0x920E_612C, 0xA509_51BA, 0xFF6D_C419, 0xCA6A_F48F,
    0x9163_A535, 0xA664_95A3, 0xFEDB_8832, 0xCFDC_B8A4, 0x94D5_E91E, 0xA3D2_D988,
    0xF9B6_4C2B, 0xCCB1_7CBD, 0x97B8_2D07, 0xA0BF_1D91, 0xFDB7_1064, 0xC4B0_20F2,
    0x9FB9_7148, 0xA8BE_41DE, 0xF2DA_D47D, 0xC7DD_E4EB, 0x9CD4_B551, 0xABD3_85C7,
    0xF36C_9856, 0xC26B_A8C0, 0x9962_F97A, 0xAE65_C9EC, 0xF401_5C4F, 0xC106_6CD9,
    0x9A0F_3D63, 0xAD08_0DF5, 0xFB6E_20C8, 0xD269_105E, 0x8960_41E4, 0xBE67_7172,
    0xE403_E4D1, 0xD104_D447, 0x8A0D_85FD, 0xBD0A_B56B, 0xE5B5_A8FA, 0xD4B2_986C,
    0x8FBB_C9D6, 0xB8BC_F940, 0xE2D8_6CE3, 0xD7DF_5C75, 0x8CD6_0DCF, 0xBBD1_3D59,
    0xE6D9_30AC, 0xDFDE_003A, 0x84D7_5180, 0xB3D0_6116, 0xE9B4_F4B5, 0xDCB3_C423,
    0x87BA_9599, 0xB0BD_A50F, 0xE802_B89E, 0xD905_8808, 0x820C_D9B2, 0xB50B_E924,
    0xEF6F_7C87, 0xDA68_4C11, 0x8161_1DAB, 0xB666_2D3D, 0xF6DC_4190, 0xFFDB_7106,
    0xA4D2_20BC, 0x93D5_102A, 0xC9B1_8589, 0xFCB6_B51F, 0xA7BF_E4A5, 0x90B8_D433,
    0xC807_C9A2, 0xF900_F934, 0xA209_A88E, 0x950E_9818, 0xCF6A_0DBB, 0xFA6D_3D2D,
    0xA164_6C97, 0x9663_5C01, 0xCB6B_51F4, 0xF26C_6162, 0xA965_30D8, 0x9E62_004E,
    0xC406_95ED, 0xF101_A57B, 0xAA08_F4C1, 0x9D0F_C457, 0xC5B0_D9C6, 0xF4B7_E950,
    0xAFBE_B8EA, 0x98B9_887C, 0xC2DD_1DDF, 0xF7DA_2D49, 0xACD3_7CF3, 0x9BD4_4C65,
    0xCDB2_6158, 0xE4B5_51CE, 0xBFBC_0074, 0x88BB_30E2, 0xD2DF_A541, 0xE7D8_95D7,
    0xBCD1_C46D, 0x8BD6_F4FB, 0xD369_E96A, 0xE26E_D9FC, 0xB967_8846, 0x8E60_B8D0,
    0xD404_2D73, 0xE103_1DE5, 0xBA0A_4C5F, 0x8D0D_7CC9, 0xD005_713C, 0xE902_41AA,
    0xB20B_1010, 0x850C_2086, 0xDF68_B525, 0xEA6F_85B3, 0xB166_D409, 0x8661_E49F,
    0xDEDE_F90E, 0xEFD9_C998, 0xB4D0_9822, 0x83D7_A8B4, 0xD9B3_3D17, 0xECB4_0D81,
    0xB7BD_5C3B, 0x80BA_6CAD, 0xEDB8_8320, 0xA4BF_B3B6, 0xFFB6_E20C, 0xC8B1_D29A,
    0x92D5_4739, 0xA7D2_77AF, 0xFCDB_2615, 0xCBDC_1683, 0x9363_0B12, 0xA264_3B84,
    0xF96D_6A3E, 0xCE6A_5AA8, 0x940E_CF0B, 0xA109_FF9D, 0xFA00_AE27, 0xCD07_9EB1,
    0x900F_9344, 0xA908_A3D2, 0xF201_F268, 0xC506_C2FE, 0x9F62_575D, 0xAA65_67CB,
    0xF16C_3671, 0xC66B_06E7, 0x9ED4_1B76, 0xAFD3_2BE0, 0xF4DA_7A5A, 0xC3DD_4ACC,
    0x99B9_DF6F, 0xACBE_EFF9, 0xF7B7_BE43, 0xC0B0_8ED5, 0x96D6_A3E8, 0xBFD1_937E,
    0xE4D8_C2C4, 0xD3DF_F252, 0x89BB_67F1, 0xBCBC_5767, 0xE7B5_06DD, 0xD0B2_364B,
    0x880D_2BDA, 0xB90A_1B4C, 0xE203_4AF6, 0xD504_7A60, 0x8F60_EFC3, 0xBA67_DF55,
    0xE16E_8EEF, 0xD669_BE79, 0x8B61_B38C, 0xB266_831A, 0xE96F_D2A0, 0xDE68_E236,
    0x840C_7795, 0xB10B_4703, 0xEA02_16B9, 0xDD05_262F, 0x85BA_3BBE, 0xB4BD_0B28,
    0xEFB4_5A92, 0xD8B3_6A04, 0x82D7_FFA7, 0xB7D0_CF31, 0xECD9_9E8B, 0xDBDE_AE1D,
    0x9B64_C2B0, 0x9263_F226, 0xC96A_A39C, 0xFE6D_930A, 0xA409_06A9, 0x910E_363F,
    0xCA07_6785, 0xFD00_5713, 0xA5BF_4A82, 0x94B8_7A14, 0xCFB1_2BAE, 0xF8B6_1B38,
    0xA2D2_8E9B, 0x97D5_BE0D, 0xCCDC_EFB7, 0xFBDB_DF21, 0xA6D3_D2D4, 0x9FD4_E242,
    0xC4DD_B3F8, 0xF3DA_836E, 0xA9BE_16CD, 0x9CB9_265B, 0xC7B0_77E1, 0xF0B7_4777,
    0xA808_5AE6, 0x990F_6A70, 0xC206_3BCA, 0xF501_0B5C, 0xAF65_9EFF, 0x9A62_AE69,
    0xC16B_FFD3, 0xF66C_CF45, 0xA00A_E278, 0x890D_D2EE, 0xD204_8354, 0xE503_B3C2,
    0xBF67_2661, 0x8A60_16F7, 0xD169_474D, 0xE66E_77DB, 0xBED1_6A4A, 0x8FD6_5ADC,
    0xD4DF_0B66, 0xE3D8_3BF0, 0xB9BC_AE53, 0x8CBB_9EC5, 0xD7B2_CF7F, 0xE0B5_FFE9,
    0xBDBD_F21C, 0x84BA_C28A, 0xDFB3_9330, 0xE8B4_A3A6, 0xB2D0_3605, 0x87D7_0693,
    0xDCDE_5729, 0xEBD9_67BF, 0xB366_7A2E, 0x8261_4AB8, 0xD968_1B02, 0xEE6F_2B94,
    0xB40B_BE37, 0x810C_8EA1, 0xDA05_DF1B, 0xED02_EF8D,
];

/// The game's save check (0x52EEE0 / 0x549C40): a reflected CRC-32 loop (table above) over the
/// whole 0xCA6C-byte payload, starting from 0x6368616F ("chao") and ending XOR 0x686F6765
/// ("hoge"), taken with bytes 0xCA64/66/67/69/6B zeroed; 0xCA65/68/6A are random salt. The
/// result's bytes 0..3 go to 0xCA69, 0xCA64, 0xCA6B, 0xCA66; 0xCA67 gets salt afterwards.
fn checksum(payload: &[u8]) -> u32 {
    let mut crc = 0x6368_616Fu32;
    for (i, &byte) in payload.iter().enumerate() {
        let byte = if matches!(i, 0xCA64 | 0xCA66 | 0xCA67 | 0xCA69 | 0xCA6B) {
            0
        } else {
            byte
        };
        crc = (crc >> 8) ^ TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize];
    }
    crc ^ 0x686F_6765
}

fn checksum_ok(payload: &[u8]) -> bool {
    payload.len() == PAYLOAD_SIZE && {
        let stored = u32::from_le_bytes([
            payload[0xCA69],
            payload[0xCA64],
            payload[0xCA6B],
            payload[0xCA66],
        ]);
        stored == checksum(payload)
    }
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
    let scope = game::chao_save_scope();
    let frame = Frame {
        safe: scope.is_some(),
        in_garden: scope == Some(game::SaveScope::Garden),
        place: (
            game::game_mode(),
            game::current_level(),
            game::current_chao_area(),
        ),
    };
    match with(|clock| clock.due(now, frame)) {
        Some(Reason::Interval) => match save("five-minute autosave") {
            Ok(()) => hud::note("Chao World autosaved"),
            Err(error) => {
                log!("save: autosave failed: {error}; retrying in 30 seconds");
                hud::toast(format!("Chao autosave failed: {error}"));
            }
        },
        Some(Reason::Changed) => {
            if let Err(error) = save("online changes") {
                log!("save: saving online changes failed: {error}; retrying in 30 seconds");
            }
        }
        None => {}
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

    const GARDEN: Frame = Frame {
        safe: true,
        in_garden: true,
        place: (5, 90, 1),
    };
    const STAGE: Frame = Frame {
        safe: true,
        in_garden: false,
        place: (5, 13, 0),
    };
    const LOADING: Frame = Frame {
        safe: false,
        in_garden: false,
        place: (4, 13, 0),
    };

    #[test]
    fn five_safe_minutes_anywhere_trigger_one_save() {
        for frame in [GARDEN, STAGE] {
            let now = Instant::now();
            let mut clock = Clock::default();
            assert_eq!(clock.due(now, frame), None);
            assert_eq!(
                clock.due(now + INTERVAL - Duration::from_secs(1), frame),
                None
            );
            assert_eq!(clock.due(now + INTERVAL, frame), Some(Reason::Interval));
            clock.saved(now + INTERVAL, now + INTERVAL);
            assert_eq!(
                clock.due(now + INTERVAL + Duration::from_secs(1), frame),
                None
            );
        }
    }

    #[test]
    fn loading_time_does_not_count_and_a_new_place_must_settle() {
        let now = Instant::now();
        let mut clock = Clock::default();
        assert_eq!(clock.due(now, LOADING), None);
        assert_eq!(clock.due(now + INTERVAL * 4, LOADING), None);
        assert_eq!(clock.due(now + INTERVAL * 5, GARDEN), None);
        assert_eq!(
            clock.due(now + INTERVAL * 6, GARDEN),
            Some(Reason::Interval)
        );
        clock.saved(now + INTERVAL * 6, now + INTERVAL * 6);
        // A place change (same safety) restarts the settle wait.
        let later = now + INTERVAL * 12;
        assert_eq!(clock.due(later, STAGE), None);
        assert_eq!(clock.due(later + SETTLE / 2, STAGE), None);
        assert_eq!(clock.due(later + SETTLE, STAGE), Some(Reason::Interval));
    }

    #[test]
    fn manual_save_resets_autosave_and_failures_wait_before_retry() {
        let now = Instant::now();
        let mut clock = Clock::default();
        clock.due(now, GARDEN);
        assert_eq!(clock.due(now + INTERVAL, GARDEN), Some(Reason::Interval));
        let retry = now + INTERVAL + RETRY;
        assert_eq!(clock.due(retry - Duration::from_secs(1), GARDEN), None);
        assert_eq!(clock.due(retry, GARDEN), Some(Reason::Interval));
        clock.saved(retry, retry);
        assert_eq!(clock.due(retry + Duration::from_secs(1), GARDEN), None);
    }

    #[test]
    fn changes_outside_a_garden_save_after_the_debounce() {
        let now = Instant::now();
        let mut clock = Clock::default();
        assert_eq!(clock.due(now, STAGE), None);
        clock.changed(now + SETTLE);
        clock.changed(now + SETTLE + Duration::from_secs(5));
        let at = now + SETTLE + DEBOUNCE;
        assert_eq!(clock.due(at - Duration::from_millis(1), STAGE), None);
        assert_eq!(clock.due(at, STAGE), Some(Reason::Changed));
        clock.saved(at, at);
        assert_eq!(clock.due(at + RETRY * 2, STAGE), None);
    }

    #[test]
    fn changes_wait_while_in_a_garden_or_loading() {
        let now = Instant::now();
        let mut clock = Clock::default();
        clock.due(now, GARDEN);
        clock.changed(now);
        assert_eq!(clock.due(now + DEBOUNCE * 3, GARDEN), None);
        assert_eq!(clock.due(now + DEBOUNCE * 4, LOADING), None);
        assert_eq!(clock.due(now + DEBOUNCE * 5, STAGE), None);
        assert_eq!(
            clock.due(now + DEBOUNCE * 5 + SETTLE, STAGE),
            Some(Reason::Changed)
        );
    }

    #[test]
    fn changed_saves_are_at_most_every_thirty_seconds_and_keep_later_changes() {
        let now = Instant::now();
        let mut clock = Clock::default();
        clock.due(now, STAGE);
        clock.changed(now);
        let first = now + DEBOUNCE;
        assert_eq!(clock.due(first, STAGE), Some(Reason::Changed));
        // A change landing between the snapshot and the write stays dirty.
        clock.changed(first + Duration::from_millis(5));
        let saved = first + Duration::from_millis(10);
        clock.saved(saved, first);
        assert_eq!(
            clock.due(saved + RETRY - Duration::from_secs(1), STAGE),
            None
        );
        assert_eq!(clock.due(saved + RETRY, STAGE), Some(Reason::Changed));
        // A failed attempt also waits the full 30 seconds.
        assert_eq!(
            clock.due(saved + RETRY * 2 - Duration::from_secs(1), STAGE),
            None
        );
        assert_eq!(clock.due(saved + RETRY * 2, STAGE), Some(Reason::Changed));
        clock.saved(saved + RETRY * 2, saved + RETRY * 2);
        assert_eq!(clock.due(saved + RETRY * 4, STAGE), None);
    }

    /// Seal a payload the way 0x52EEE0 does (salt left as is).
    fn seal(payload: &mut [u8]) {
        let [a, b, c, d] = checksum(payload).to_le_bytes();
        payload[0xCA69] = a;
        payload[0xCA64] = b;
        payload[0xCA6B] = c;
        payload[0xCA66] = d;
    }

    #[test]
    fn the_checksum_accepts_sealed_saves_and_rejects_edits() {
        let mut payload: Vec<u8> = (0..PAYLOAD_SIZE).map(|i| (i * 7 % 253) as u8).collect();
        assert!(!checksum_ok(&payload));
        seal(&mut payload);
        assert!(checksum_ok(&payload));
        // The trailing salt byte 0xCA67 isn't covered.
        payload[0xCA67] ^= 0xFF;
        assert!(checksum_ok(&payload));
        payload[0x3AA4 - PAYLOAD_START + 0x80] ^= 1;
        assert!(!checksum_ok(&payload));
        payload[0x3AA4 - PAYLOAD_START + 0x80] ^= 1;
        payload[0xCA65] ^= 1;
        assert!(!checksum_ok(&payload));
        assert!(!checksum_ok(&payload[..16]));
    }

    #[test]
    fn the_checksum_of_a_zeroed_save_is_the_games() {
        // Computed separately from the dumped exe's table with the 0x549C40 loop. The same
        // computation matched the checksum stored in a real Steam save (2026-10-19).
        assert_eq!(checksum(&[0; PAYLOAD_SIZE]), 0xA0C6_5D6C);
    }
}

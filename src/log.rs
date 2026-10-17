//! The mod's log, `sa2-chao-online.log` in the mod folder.

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG: Mutex<Option<File>> = Mutex::new(None);

pub fn open(mod_path: &str) {
    // The last run's log stays as .old.log (after a freeze or crash, that's the one to read).
    let path = format!("{mod_path}\\sa2-chao-online.log");
    let _ = std::fs::rename(&path, format!("{mod_path}\\sa2-chao-online.old.log"));
    let file = File::create(path).ok();
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = file;
}

pub fn write(args: std::fmt::Arguments) {
    let mut guard = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(file) = guard.as_mut() {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
        let _ = writeln!(file, "[{h:02}:{m:02}:{s:02} UTC] {args}");
        let _ = file.flush();
    }
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(format_args!($($arg)*)) };
}

//! `sa2co-server`: a dedicated SA2 Chao Online host. It relays between up to 8 players and
//! isn't a player itself, so the lobby stays up without anyone's game hosting it.
//!
//! ```text
//! sa2co-server [port]          (default 43902, or SA2CO_PORT)
//! ```
//!
//! With `SA2CO_STATUS=<file>` it keeps a JSON file up to date (every few seconds) with who's
//! online, where they are and their parties, for a web page (the Boyz Hub) to show.
//!
//! With `SA2CO_DATA=<folder>` it keeps the shared gardens (their trees and the server's items:
//! re/chao-garden-items.md 7) in `gardens.txt` there, and the Chao players bring (shown asleep
//! while they're offline, net/roster.rs) in `chao/`, a file per player key, so they outlive a
//! restart.
//!
//! Players join it like any host: F9 > Multiplayer: Join, Host address `<server>:<port>`.
//! They need the mod version printed at start. Stops cleanly on Ctrl+C / SIGTERM.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sa2_chao_online::game::KNOWN_GAME_BUILD;
use sa2_chao_online::net::files::SkinStore;
use sa2_chao_online::net::protocol::{Body, PROTOCOL_VERSION, PlayerState};
use sa2_chao_online::net::session::{Event, Host, Identity};
use sa2_chao_online::net::udp::Endpoint;

const DEFAULT_PORT: u16 = 43902;
/// How often the server checks the network.
const TICK: Duration = Duration::from_millis(5);
/// How often the status file is rewritten.
const STATUS_EVERY: Duration = Duration::from_secs(3);
/// The shared skins folder is looked at again this often (new or changed skins go out).
const SKINS_EVERY: Duration = Duration::from_secs(60);
/// The shared gardens are written (when they changed) this often.
const GARDENS_EVERY: Duration = Duration::from_secs(10);
const GARDENS_FILE: &str = "gardens.txt";
const CHAO_DIR: &str = "chao";

static STOP: AtomicBool = AtomicBool::new(false);

fn main() {
    sa2_chao_online::log::to_stderr();
    let port = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("SA2CO_PORT").ok())
        .map(|p| match p.trim().parse::<u16>() {
            Ok(port) if port > 0 => port,
            _ => {
                eprintln!("invalid port {p:?}");
                std::process::exit(2);
            }
        })
        .unwrap_or(DEFAULT_PORT);

    let ep = match Endpoint::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))) {
        Ok(ep) => ep,
        Err(e) => {
            eprintln!("can't listen on UDP port {port}: {e}");
            std::process::exit(1);
        }
    };
    let me = Identity {
        name: "server".into(),
        mod_version: env!("CARGO_PKG_VERSION").into(),
        game_build: KNOWN_GAME_BUILD,
        key: [0; 16],
    };
    let mut host = Host::dedicated(ep, me, Instant::now());
    // Gardens' Chao run here instead of in a player's game (chao_sim; still being built).
    stop_on_signals();
    println!(
        "SA2 Chao Online server {} (protocol {PROTOCOL_VERSION}) on UDP port {port}",
        env!("CARGO_PKG_VERSION")
    );
    println!("Players need mod version {}.", env!("CARGO_PKG_VERSION"));

    // Skins everyone gets (SA2CO_SKINS: a folder of skin folders, e.g. a mounted volume).
    let skins_dir = std::env::var("SA2CO_SKINS")
        .ok()
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let mut skins_seen = None;
    let mut last_skins = Instant::now() - SKINS_EVERY;

    // Shared gardens kept across restarts (SA2CO_DATA: a folder, e.g. a mounted volume).
    let data_dir = std::env::var("SA2CO_DATA")
        .ok()
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let gardens_file = data_dir.as_ref().map(|d| d.join(GARDENS_FILE));
    let chao_dir = data_dir.as_ref().map(|d| d.join(CHAO_DIR));
    if let Some(file) = &gardens_file
        && let Ok(text) = std::fs::read_to_string(file)
    {
        host.load_garden_state(&text);
        println!("Shared gardens from {}", file.display());
    }
    if let Some(dir) = &chao_dir {
        load_chao(&mut host, dir);
    }
    let mut last_gardens = Instant::now();

    let status_file = std::env::var("SA2CO_STATUS").ok().filter(|f| !f.is_empty());
    let mut last_status = Instant::now() - STATUS_EVERY;
    let mut names: BTreeMap<u8, String> = BTreeMap::new();
    let mut states: BTreeMap<u8, PlayerState> = BTreeMap::new();
    while !STOP.load(Ordering::Relaxed) {
        if let Some(dir) = &skins_dir
            && last_skins.elapsed() >= SKINS_EVERY
        {
            last_skins = Instant::now();
            let seen = folder_signature(dir);
            if skins_seen != Some(seen) {
                skins_seen = Some(seen);
                let store = SkinStore::scan(dir);
                println!(
                    "Sharing {} skin file(s), {:.1} MB, from {}",
                    store.len(),
                    store.bytes() as f64 / 1e6,
                    dir.display()
                );
                host.set_files((!store.is_empty()).then(|| Arc::new(store)));
            }
        }
        for event in host.poll_events(Instant::now()) {
            match event {
                Event::Joined { id, name } => {
                    names.insert(id, name.clone());
                    println!("{name} joined ({} online)", names.len());
                }
                Event::Left { id } => {
                    let name = names.remove(&id).unwrap_or_else(|| format!("player {id}"));
                    states.remove(&id);
                    println!("{name} left ({} online)", names.len());
                }
                Event::Game {
                    from,
                    body: Body::PlayerState(s),
                } => {
                    states.insert(from, s);
                }
                _ => {}
            }
        }
        if let Some(file) = &gardens_file
            && last_gardens.elapsed() >= GARDENS_EVERY
        {
            last_gardens = Instant::now();
            save_gardens(&mut host, file);
            if let Some(dir) = &chao_dir {
                save_chao(&mut host, dir);
            }
        }
        if let Some(file) = &status_file
            && last_status.elapsed() >= STATUS_EVERY
        {
            last_status = Instant::now();
            if let Err(e) = write_status(file, &names, &states) {
                eprintln!("can't write {file}: {e}");
            }
        }
        std::thread::sleep(TICK);
    }
    if let Some(file) = &status_file {
        let _ = write_status(file, &BTreeMap::new(), &BTreeMap::new());
    }
    if let Some(file) = &gardens_file {
        save_gardens(&mut host, file);
    }
    if let Some(dir) = &chao_dir {
        save_chao(&mut host, dir);
    }
    println!("Stopping: telling {} player(s)", names.len());
    host.shutdown(Instant::now());
}

/// Writes the shared gardens to `file` if they changed (to a temporary file, then renamed).
fn save_gardens(host: &mut Host, file: &Path) {
    let Some(text) = host.garden_state() else {
        return;
    };
    let tmp = file.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, file)) {
        eprintln!("can't write {}: {e}", file.display());
    }
}

/// Reads the Chao kept for players from `dir` (one `<key>.txt` each).
fn load_chao(host: &mut Host, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut count = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".txt") {
            continue;
        }
        match std::fs::read_to_string(entry.path()) {
            Ok(text) => {
                host.load_chao_file(&name, &text);
                count += 1;
            }
            Err(e) => eprintln!("can't read {}: {e}", entry.path().display()),
        }
    }
    println!("Chao of {count} player(s) from {}", dir.display());
}

/// Writes the players' kept Chao that changed to `dir` (each to a temporary file, then
/// renamed), and removes those of players forgotten.
fn save_chao(host: &mut Host, dir: &Path) {
    let files = host.chao_files();
    if files.is_empty() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("can't make {}: {e}", dir.display());
        return;
    }
    for (name, text) in files {
        let file = dir.join(&name);
        let result = match text {
            Some(text) => {
                let tmp = file.with_extension("tmp");
                std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &file))
            }
            None => std::fs::remove_file(&file),
        };
        if let Err(e) = result {
            eprintln!("can't write {}: {e}", file.display());
        }
    }
}

/// A JSON string literal.
/// Files, sizes and change times under `dir` (cheap: no contents), to notice changes.
fn folder_signature(dir: &Path) -> u64 {
    fn walk(dir: &Path, depth: usize, h: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() && depth < 4 {
                walk(&path, depth + 1, h);
            } else if let Ok(m) = e.metadata() {
                let changed = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                for v in [m.len(), changed, path.as_os_str().len() as u64] {
                    *h = (*h ^ v).wrapping_mul(0x0100_0000_01B3);
                }
            }
        }
    }
    let mut h = 0xCBF2_9CE4_8422_2325;
    walk(dir, 0, &mut h);
    h
}

fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The status file: `{"updated": unix seconds, "version": "...", "players": [{"name", "place",
/// "room", "mode"}]}`. Written to a temporary file and renamed, so readers never see half.
fn write_status(
    file: &str,
    names: &BTreeMap<u8, String>,
    states: &BTreeMap<u8, PlayerState>,
) -> std::io::Result<()> {
    let updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let players: Vec<String> = names
        .iter()
        .map(|(id, name)| {
            let s = states.get(id);
            let place = s.map_or_else(
                || "Joining".to_string(),
                |s| sa2_chao_online::places::place(s.level, s.chao_area),
            );
            let (room, mode) = match s.map(|s| s.room) {
                Some((owner, kind)) if kind != 0 => (
                    names.get(&owner).map_or("null".to_string(), |n| json(n)),
                    json(if kind == 2 { "Co-op" } else { "Race" }),
                ),
                _ => ("null".to_string(), "null".to_string()),
            };
            format!(
                r#"{{"name":{},"place":{},"room":{room},"mode":{mode}}}"#,
                json(name),
                json(&place)
            )
        })
        .collect();
    let text = format!(
        r#"{{"updated":{updated},"version":{},"players":[{}]}}"#,
        json(env!("CARGO_PKG_VERSION")),
        players.join(",")
    );
    let tmp = format!("{file}.tmp");
    std::fs::write(&tmp, text + "\n")?;
    std::fs::rename(&tmp, file)
}

/// Ctrl+C and SIGTERM (docker stop) end the main loop so players get a clean goodbye.
#[cfg(unix)]
fn stop_on_signals() {
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    unsafe extern "C" {
        fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
    }
    extern "C" fn on_signal(_: i32) {
        STOP.store(true, Ordering::Relaxed);
    }
    // SAFETY: libc's signal(); the handler only stores to an atomic.
    unsafe {
        signal(SIGINT, on_signal);
        signal(SIGTERM, on_signal);
    }
}

/// Elsewhere the process just ends on Ctrl+C; clients notice within a few seconds.
#[cfg(not(unix))]
fn stop_on_signals() {}

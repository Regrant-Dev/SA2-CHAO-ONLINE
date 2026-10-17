//! `sa2co-server`: a dedicated SA2 Chao Online host. It relays between up to 8 players and
//! isn't a player itself, so the lobby stays up without anyone's game hosting it.
//!
//! ```text
//! sa2co-server [port]          (default 43902, or SA2CO_PORT)
//! ```
//!
//! With `SA2CO_STATUS=<file>` it keeps a JSON file up to date (every few seconds) with who's
//! online, where they are and their rooms, for a web page (the Boyz Hub) to show.
//!
//! Players join it like any host: F9 > Multiplayer: Join, Host address `<server>:<port>`.
//! They need the mod version printed at start. Stops cleanly on Ctrl+C / SIGTERM.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sa2_chao_online::game::KNOWN_GAME_BUILD;
use sa2_chao_online::net::protocol::{Body, PROTOCOL_VERSION, PlayerState};
use sa2_chao_online::net::session::{Event, Host, Identity};
use sa2_chao_online::net::udp::Endpoint;

const DEFAULT_PORT: u16 = 43902;
/// How often the server checks the network.
const TICK: Duration = Duration::from_millis(5);
/// How often the status file is rewritten.
const STATUS_EVERY: Duration = Duration::from_secs(3);

static STOP: AtomicBool = AtomicBool::new(false);

fn main() {
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
    };
    let mut host = Host::dedicated(ep, me, Instant::now());
    stop_on_signals();
    println!(
        "SA2 Chao Online server {} (protocol {PROTOCOL_VERSION}) on UDP port {port}",
        env!("CARGO_PKG_VERSION")
    );
    println!("Players need mod version {}.", env!("CARGO_PKG_VERSION"));

    let status_file = std::env::var("SA2CO_STATUS").ok().filter(|f| !f.is_empty());
    let mut last_status = Instant::now() - STATUS_EVERY;
    let mut names: BTreeMap<u8, String> = BTreeMap::new();
    let mut states: BTreeMap<u8, PlayerState> = BTreeMap::new();
    while !STOP.load(Ordering::Relaxed) {
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
    println!("Stopping: telling {} player(s)", names.len());
    host.shutdown(Instant::now());
}

/// A JSON string literal.
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

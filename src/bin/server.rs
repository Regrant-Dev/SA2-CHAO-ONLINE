//! `sa2co-server`: a dedicated SA2 Chao Online host. It relays between up to 8 players and
//! isn't a player itself, so the lobby stays up without anyone's game hosting it.
//!
//! ```text
//! sa2co-server [port]          (default 43902, or SA2CO_PORT)
//! ```
//!
//! Players join it like any host: F9 > Multiplayer: Join, Host address `<server>:<port>`.
//! They need the mod version printed at start. Stops cleanly on Ctrl+C / SIGTERM.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sa2_chao_online::game::KNOWN_GAME_BUILD;
use sa2_chao_online::net::protocol::PROTOCOL_VERSION;
use sa2_chao_online::net::session::{Event, Host, Identity};
use sa2_chao_online::net::udp::Endpoint;

const DEFAULT_PORT: u16 = 43902;
/// How often the server checks the network.
const TICK: Duration = Duration::from_millis(5);

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

    let mut names: BTreeMap<u8, String> = BTreeMap::new();
    while !STOP.load(Ordering::Relaxed) {
        for event in host.poll_events(Instant::now()) {
            match event {
                Event::Joined { id, name } => {
                    names.insert(id, name.clone());
                    println!("{name} joined ({} online)", names.len());
                }
                Event::Left { id } => {
                    let name = names.remove(&id).unwrap_or_else(|| format!("player {id}"));
                    println!("{name} left ({} online)", names.len());
                }
                _ => {}
            }
        }
        std::thread::sleep(TICK);
    }
    println!("Stopping: telling {} player(s)", names.len());
    host.shutdown(Instant::now());
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

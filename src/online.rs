//! Game-side glue for the lobby: config, the network thread, sending our state and Chao, and
//! handing other players' states, Chao, room picks, races and karate to the other modules.
//!
//! The network thread keeps the session alive (receive, heartbeats, timeouts, reconnect) even
//! while the game runs no frames, e.g. on loading screens. Game memory is only touched in
//! `on_frame`, on the game thread.
//!
//! Configured by `config.ini` in the mod folder (F9 menu or SA Mod Manager); without one,
//! multiplayer is off.

use std::collections::BTreeMap;
use std::fs;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::chao::{self, ChaoData};
use crate::game;
use crate::hud;
use crate::log;
use crate::net::protocol::{Body, CHAO_SLOTS, MAX_CARRIED_CHAO, MY_CHAO_INDEX, PlayerState};
use crate::net::session::{Client, Event, Host, Identity, Session};
use crate::net::udp::Endpoint;
use crate::puppet;
use crate::room;
use crate::timeline::Timeline;
use crate::together;
use crate::visitors;

pub const DEFAULT_PORT: u16 = 43_902;
/// Send our state every N frames (60 fps / 2 = 30 Hz).
const STATE_EVERY_FRAMES: u32 = 2;
/// Network thread poll interval.
const POLL_EVERY: Duration = Duration::from_millis(10);
/// Wait before rejoining after a disconnect (client mode).
const RECONNECT_AFTER: Duration = Duration::from_secs(3);

/// Settings when there is no `config.ini` yet (multiplayer off).
const DEFAULT_CONFIG: &str = "mode = off\nname = Player\nport = 43902\nhost = 127.0.0.1:43902\n";

#[derive(Debug, PartialEq)]
pub enum Mode {
    Off,
    Host { port: u16 },
    Client { host: SocketAddr },
}

#[derive(Debug, PartialEq)]
pub struct Config {
    pub mode: Mode,
    pub name: String,
    /// Names above other players.
    pub name_tags: bool,
    /// Friends' Chao take the CPU lanes in Chao Races.
    pub chao_race: bool,
    /// "My Chao": save slot of the Chao for races and karate (-1 = the first hatched one).
    pub my_chao: i32,
    /// Garden visits mode (index into `VISIT_MODES`).
    pub visits: usize,
}

/// Parses `key = value` lines; `;` and `#` start comments.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.split([';', '#']).next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=') {
            values.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let get = |k: &str| values.get(k).map(String::as_str).unwrap_or("");

    let name: String = get("name").chars().filter(|c| !c.is_control()).collect();
    let name = if name.is_empty() {
        "Player".to_string()
    } else {
        name
    };
    if name.len() > crate::net::protocol::MAX_NAME {
        return Err(format!(
            "name '{name}' is longer than {} characters",
            crate::net::protocol::MAX_NAME
        ));
    }

    let mode = match get("mode").to_ascii_lowercase().as_str() {
        "" | "off" => Mode::Off,
        "host" => {
            let port = match get("port") {
                "" => DEFAULT_PORT,
                p => p.parse().map_err(|_| format!("bad port '{p}'"))?,
            };
            Mode::Host { port }
        }
        "client" | "join" => {
            let host = get("host");
            // The port is optional: without one, the default port.
            let full = if host.contains(':') {
                host.to_string()
            } else {
                format!("{host}:{DEFAULT_PORT}")
            };
            let addr = full
                .to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
                .ok_or_else(|| {
                    format!("bad host address '{host}' (use a name or IP, :port optional)")
                })?;
            Mode::Client { host: addr }
        }
        other => return Err(format!("unknown mode '{other}'")),
    };
    let name_tags = !matches!(
        get("nametags").to_ascii_lowercase().as_str(),
        "off" | "0" | "false" | "no"
    );
    Ok(Config {
        mode,
        name,
        name_tags,
        my_chao: get("mychao").trim().parse().unwrap_or(-1),
        visits: parse_visits(get("visitwith")),
        chao_race: !matches!(
            get("chaorace").to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        ),
    })
}

/// A remote player and their buffered poses (see timeline.rs).
struct Remote {
    name: String,
    latest: Option<PlayerState>,
    timeline: Timeline,
    /// Chao this player brought to their current garden (they never change our save).
    /// Their garden Chao (visitors), then their chosen Chao (`MY_CHAO_INDEX`).
    chao: [Option<Box<ChaoData>>; CHAO_SLOTS],
}

fn to_pose(s: &PlayerState) -> puppet::Pose {
    puppet::Pose {
        character: s.character,
        position: s.position,
        rotation: s.rotation,
        anim: s.anim,
        frame: s.frame,
        flags: s.flags,
        upgrades: s.upgrades,
    }
}

/// Local clock in frames (60 per second) since the session started.
fn frames_since(start: Instant, now: Instant) -> f64 {
    now.duration_since(start).as_secs_f64() * 60.0
}

struct Online {
    session: Session,
    started: Instant,
    frame: u32,
    remote: BTreeMap<u8, Remote>,
    /// Garden area we were last in (-1 = not in a garden), to notice entering one.
    garden: i32,
    /// Send our Chao again (we entered a garden, or someone joined).
    resend_chao: bool,
    /// Flags of our last sent state (to log spin ball changes).
    last_flags: u8,
    /// "My Chao" as last sent to the others (live karate uses it on both sides).
    my_chao: Option<ChaoData>,
    /// The Chao we picked for a Chao Race, sent in place of My Chao while racing so friends'
    /// games show it in our lane.
    race_chao: Option<ChaoData>,
    /// Client mode: where to rejoin, and when (set after a disconnect).
    rejoin: Option<(SocketAddr, Identity, Option<Instant>)>,
}

static ONLINE: Mutex<Option<Online>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);
static MOD_PATH: OnceLock<String> = OnceLock::new();

/// Reads `config.ini` (F9 menu / SA Mod Manager), or an `online.ini` from older versions;
/// defaults (off) when neither exists.
fn load_config(mod_path: &str) -> Result<Config, String> {
    let dir = Path::new(mod_path);
    for file in ["config.ini", "online.ini"] {
        let path = dir.join(file);
        if let Ok(text) = fs::read_to_string(&path) {
            return parse_config(&text).map_err(|e| format!("{}: {e}", path.display()));
        }
    }
    parse_config(DEFAULT_CONFIG)
}

/// Opens the session for `config`; `None` when multiplayer is off.
fn start(config: &Config) -> Result<Option<Online>, String> {
    puppet::set_name_tags(config.name_tags);
    crate::race::set_enabled(config.chao_race);
    set_my_chao(config.my_chao);
    set_visits(config.visits);
    let me = Identity {
        name: config.name.clone(),
        mod_version: env!("CARGO_PKG_VERSION").into(),
        game_build: game::game_build(),
    };
    let now = Instant::now();
    let (session, rejoin) = match config.mode {
        Mode::Off => return Ok(None),
        Mode::Host { port } => {
            let ep = Endpoint::bind(SocketAddr::from(([0, 0, 0, 0], port)))
                .map_err(|e| format!("can't listen on port {port}: {e}"))?;
            log!("online: hosting on port {port} as '{}'", me.name);
            hud::toast(format!("Hosting on port {port} as {}", me.name));
            (Session::Host(Host::new(ep, me, now)), None)
        }
        Mode::Client { host } => {
            let ep = Endpoint::bind(SocketAddr::from(([0, 0, 0, 0], 0)))
                .map_err(|e| format!("can't open socket: {e}"))?;
            log!("online: connecting to {host} as '{}'", me.name);
            hud::toast(format!("Joining {host} as {}...", me.name));
            let session = Session::Client(Client::connect(ep, host, &me, now));
            (session, Some((host, me, None)))
        }
    };
    Ok(Some(Online {
        session,
        started: now,
        frame: 0,
        remote: BTreeMap::new(),
        garden: -1,
        resend_chao: false,
        last_flags: 0,
        my_chao: None,
        race_chao: None,
        rejoin,
    }))
}

/// Stops multiplayer (if running), telling the others we left.
pub fn stop() {
    let mut guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut online) = guard.take() {
        online.session.shutdown(Instant::now());
        puppet::set_remotes(Vec::new());
        log!("online: turned off");
        hud::toast("Multiplayer off");
    }
}

/// (Re)starts multiplayer from the saved settings. Errors are also shown as a toast.
pub fn restart() -> Result<(), String> {
    stop();
    let path = MOD_PATH.get().map(String::as_str).unwrap_or(".");
    let mut guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    match load_config(path).and_then(|c| start(&c)) {
        Ok(Some(online)) => {
            *guard = Some(online);
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(e) => {
            log!("online: {e}");
            hud::toast(format!("Multiplayer error: {e}"));
            Err(e)
        }
    }
}

/// Chao other players brought online, as karate opponents: ("Name's Chao", record).
pub fn karate_choices() -> Vec<(String, ChaoData)> {
    let guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(online) = guard.as_ref() else {
        return Vec::new();
    };
    let mut choices: Vec<(String, ChaoData)> = Vec::new();
    for remote in online.remote.values() {
        // Their chosen Chao first, then their garden Chao (each once).
        let chosen = remote.chao[usize::from(MY_CHAO_INDEX)].iter();
        for chao in chosen.chain(remote.chao[..MAX_CARRIED_CHAO].iter().flatten()) {
            if choices.iter().any(|(_, c)| c.bytes == chao.bytes) {
                continue;
            }
            let label = format!("{}'s {}", remote.name, crate::karate::chao_name(chao));
            choices.push((label, (**chao).clone()));
        }
    }
    choices
}

fn peer_of(id: u8, s: &PlayerState) -> together::Peer {
    together::Peer {
        id,
        level: s.level,
        mission: s.mission,
        chao_area: s.chao_area,
        flags: s.flags,
    }
}

/// Other players' stage/mission/flags for start-together, and whether we're connected.
pub fn peers() -> (Vec<together::Peer>, bool) {
    let guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(online) = guard.as_ref() else {
        return (Vec::new(), false);
    };
    let peers = online
        .remote
        .iter()
        .filter_map(|(&id, r)| r.latest.map(|s| peer_of(id, &s)))
        .collect();
    (peers, online.session.my_id().is_some())
}

/// One-line status for the menu, or `None` when multiplayer is off.
pub fn status() -> Option<String> {
    let guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    let online = guard.as_ref()?;
    let players = online.remote.len() + 1;
    Some(match (&online.session, online.session.my_id()) {
        (Session::Host(_), _) => format!("Hosting, {players} player(s)"),
        (_, Some(_)) => format!("Connected, {players} player(s)"),
        (_, None) => "Connecting...".to_string(),
    })
}

/// The mod folder (for the settings file).
pub fn mod_path() -> &'static str {
    MOD_PATH.get().map(String::as_str).unwrap_or(".")
}

pub fn init(mod_path: &str) {
    let _ = MOD_PATH.set(mod_path.to_string());
    log!(
        "online: game build {:02X?} (known {:02X?})",
        game::game_build(),
        game::KNOWN_GAME_BUILD
    );
    match load_config(mod_path).and_then(|c| start(&c)) {
        Ok(Some(online)) => *ONLINE.lock().unwrap_or_else(|e| e.into_inner()) = Some(online),
        Ok(None) => log!("online: mode = off (F9 opens the multiplayer menu)"),
        Err(e) => log!("online: {e}; networking off"),
    }
    let spawned = thread::Builder::new()
        .name("sa2-chao-online net".into())
        .spawn(|| {
            while !STOP.load(Ordering::Relaxed) {
                if let Some(online) = ONLINE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    pump(online, Instant::now());
                }
                thread::sleep(POLL_EVERY);
            }
        });
    if let Err(e) = spawned {
        log!("online: can't start network thread: {e}");
    }
}

/// Network thread: receive, handle lobby events, keep the session alive, rejoin.
fn pump(online: &mut Online, now: Instant) {
    // Online, the game keeps running in the background (others are playing).
    if online.session.my_id().is_some() {
        game::keep_running_in_background();
    }
    if let Some((host, me, at)) = &mut online.rejoin
        && at.is_some_and(|at| now >= at)
    {
        *at = None;
        match Endpoint::bind(SocketAddr::from(([0, 0, 0, 0], 0))) {
            Ok(ep) => {
                log!("online: reconnecting to {host}");
                online.session = Session::Client(Client::connect(ep, *host, me, now));
            }
            Err(e) => {
                log!("online: can't open socket: {e}");
                *at = Some(now + RECONNECT_AFTER);
            }
        }
    }

    for event in online.session.poll(now) {
        match event {
            Event::Connected { my_id } => {
                log!("online: connected, I am player {my_id}");
                hud::toast("Connected");
                online.resend_chao = true;
                RESEND_MY_CHAO.store(true, Ordering::Relaxed);
            }
            Event::Disconnected { reason } => {
                log!("online: disconnected: {reason}");
                online.remote.clear();
                if let Some((_, _, at)) = &mut online.rejoin {
                    *at = Some(now + RECONNECT_AFTER);
                    hud::toast(format!("Disconnected ({reason}), retrying..."));
                } else {
                    hud::toast(format!("Disconnected ({reason})"));
                }
            }
            Event::Joined { id, name } => {
                log!("online: player {id} '{name}' joined");
                hud::toast(format!("{name} joined"));
                online.remote.insert(
                    id,
                    Remote {
                        name,
                        latest: None,
                        timeline: Timeline::default(),
                        chao: Default::default(),
                    },
                );
                online.resend_chao = true;
                RESEND_MY_CHAO.store(true, Ordering::Relaxed);
            }
            Event::Left { id } => {
                let name = online
                    .remote
                    .remove(&id)
                    .map(|r| r.name)
                    .unwrap_or_default();
                log!("online: player {id} '{name}' left");
                hud::toast(format!("{name} left"));
            }
            Event::Game {
                from,
                body: Body::PlayerState(state),
            } => {
                if let Some(remote) = online.remote.get_mut(&from) {
                    let name = &remote.name;
                    let moved_area = remote
                        .latest
                        .is_none_or(|l| l.level != state.level || l.chao_area != state.chao_area);
                    if moved_area {
                        log!(
                            "online: player {from} '{name}' is in level {} (chao area {}) at ({:.0}, {:.0}, {:.0})",
                            state.level,
                            state.chao_area,
                            state.position[0],
                            state.position[1],
                            state.position[2]
                        );
                    }
                    // Don't blend across a stage change.
                    if moved_area {
                        remote.timeline.clear();
                    }
                    let clock = frames_since(online.started, now);
                    remote.timeline.push(state.time, to_pose(&state), clock);
                    remote.latest = Some(state);
                }
            }
            Event::Game {
                from,
                body: Body::Chao { index, data },
            } => {
                if let Some(remote) = online.remote.get_mut(&from) {
                    log!(
                        "online: player {from} '{}' brought chao {index}: {}",
                        remote.name,
                        data.summary()
                    );
                    remote.chao[usize::from(index)] = Some(data);
                }
            }
            Event::Game {
                from,
                body:
                    Body::RoomPick {
                        level,
                        mission,
                        character,
                    },
            } => {
                let name = online
                    .remote
                    .get(&from)
                    .map_or_else(|| format!("player {from}"), |r| r.name.clone());
                room::received(
                    room::Pick {
                        level,
                        mission,
                        character,
                    },
                    from,
                    &name,
                );
            }
            Event::Game {
                from,
                body:
                    Body::RacePick {
                        category,
                        course,
                        level,
                    },
            } => {
                let name = online
                    .remote
                    .get(&from)
                    .map_or_else(|| format!("player {from}"), |r| r.name.clone());
                room::received_race(
                    room::RacePick {
                        category,
                        course,
                        level,
                    },
                    from,
                    &name,
                );
            }
            Event::Game {
                from,
                body: Body::KaratePick { opponent },
            } => crate::karate_live::received_pick(from, opponent, online.session.my_id()),
            Event::Game {
                from,
                body: Body::KarateState(state),
            } => crate::karate_live::received_state(from, state),
            Event::Game {
                from,
                body: Body::KarateEnd { code, result, hp },
            } => crate::karate_live::received_end(from, code, result, hp),
            Event::Game {
                from,
                body: Body::KarateCheer { count },
            } => crate::karate_live::received_cheer(from, count),
        }
    }
}

/// Game thread: hand remote poses to the puppets and send our own state.
pub fn on_frame() {
    let mut guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(online) = guard.as_mut() else {
        room::update(None, Vec::new());
        return;
    };
    let now = Instant::now();

    // Rooms: everyone's room, from their latest state.
    let others = online
        .remote
        .iter()
        .map(|(&id, r)| room::Other {
            id,
            name: r.name.clone(),
            room: r
                .latest
                .and_then(|s| room::RoomId::from_wire(s.room.0, s.room.1)),
        })
        .collect();
    room::update(online.session.my_id(), others);

    // Draw everyone who is in the same stage (and the same garden in Chao World) as us.
    let (level, area, mission) = (
        game::current_level(),
        game::current_chao_area(),
        game::mission(),
    );
    let clock = frames_since(online.started, now);
    let poses = online
        .remote
        .values()
        .filter(|r| {
            // A racing Chao, not a character (handled by race.rs below).
            r.latest
                .is_some_and(|s| s.flags & game::POSE_RACE_CHAO == 0)
        })
        .filter(|r| {
            r.latest.is_some_and(|s| {
                s.level == level
                    && if level == game::LEVEL_CHAO_WORLD {
                        i32::from(s.chao_area) == area
                    } else {
                        s.mission == mission
                    }
            })
        })
        .filter_map(|r| r.timeline.sample(clock).map(|pose| (pose, r.name.clone())))
        .collect();
    // Nobody is drawn into a local two-player (split screen) game.
    puppet::set_remotes(if game::two_player() {
        Vec::new()
    } else {
        poses
    });

    // Shared garden: send our Chao when we enter a garden (or someone joins), and show the
    // Chao of players in the same garden as visitors.
    let garden = if game::in_garden() { area } else { -1 };
    if garden != online.garden {
        online.garden = garden;
        online.resend_chao = true;
    }
    if online.resend_chao && garden > 0 && online.session.my_id().is_some() {
        online.resend_chao = false;
        let mine = match VISITS.load(Ordering::Relaxed) {
            VISITS_OFF => Vec::new(),
            VISITS_MY_CHAO => my_chosen_chao().into_iter().collect(),
            _ => carried_chao(garden),
        };
        log!("online: bringing {} chao to garden {garden}", mine.len());
        for (index, chao) in mine.into_iter().enumerate() {
            let body = Body::Chao {
                index: index as u8,
                data: Box::new(chao),
            };
            online.session.send(body, true, now);
        }
    }
    // "My Chao" for races and karate: sent on connect, when someone joins, and when changed.
    // (Before the save is loaded there is no Chao yet: keep trying.)
    if online.session.my_id().is_some()
        && RESEND_MY_CHAO.load(Ordering::Relaxed)
        && let Some(chao) = my_chosen_chao()
    {
        RESEND_MY_CHAO.store(false, Ordering::Relaxed);
        online.my_chao = Some(chao.clone());
        {
            log!("online: my Chao for races and karate: {}", chao.summary());
            let body = Body::Chao {
                index: MY_CHAO_INDEX,
                data: Box::new(chao),
            };
            online.session.send(body, true, now);
        }
    }
    // At a Chao Race: friends' games show the Chao we picked for it in our lane, not My Chao.
    if online.session.my_id().is_some() {
        let racing = game::race_chao();
        if racing.is_some() && racing != online.race_chao {
            if let Some(chao) = racing.clone() {
                log!("online: racing with {}", chao.summary());
                let body = Body::Chao {
                    index: MY_CHAO_INDEX,
                    data: Box::new(chao),
                };
                online.session.send(body, true, now);
            }
            online.race_chao = racing;
        } else if racing.is_none() && online.race_chao.take().is_some() {
            // Left the race: back to My Chao.
            RESEND_MY_CHAO.store(true, Ordering::Relaxed);
        }
    }
    {
        let peers: Vec<(u8, together::Peer)> = online
            .remote
            .iter()
            .filter_map(|(&id, r)| r.latest.map(|s| (id, peer_of(id, &s))))
            .collect();
        let connected = online.session.my_id().is_some();
        let remote = &online.remote;
        crate::karate_live::on_frame(
            &peers,
            connected,
            |id| remote.get(&id).and_then(|r| chosen_chao(r).cloned()),
            online.my_chao.as_ref(),
        );
    }
    if let Some(chao) = crate::karate_live::take_outgoing_chao() {
        // Before the pick, so the opponent's game sets up the fight with this Chao.
        let body = Body::Chao {
            index: MY_CHAO_INDEX,
            data: Box::new(chao),
        };
        online.session.send(body, true, now);
    }
    if let Some(opponent) = crate::karate_live::take_outgoing_pick() {
        online
            .session
            .send(Body::KaratePick { opponent }, true, now);
    }

    if garden > 0 {
        for (&id, remote) in &online.remote {
            let Some(state) = remote.latest else { continue };
            if state.level != level || i32::from(state.chao_area) != garden {
                continue;
            }
            let [x, y, z] = state.position;
            for (index, chao) in remote.chao.iter().take(MAX_CARRIED_CHAO).enumerate() {
                if let Some(chao) = chao {
                    visitors::spawn_remote(
                        id,
                        index as u8,
                        (**chao).clone(),
                        game::Vector { x, y, z },
                    );
                }
            }
        }
    }

    // Friends' racing Chao drive their lanes in a room race.
    for (&id, remote) in &online.remote {
        if let Some(s) = remote.latest
            && s.flags & game::POSE_RACE_CHAO != 0
        {
            crate::race::set_target(id, s.position);
        }
    }

    // Friends' Chao for online races: everyone's first Chao, then everyone's second, in
    // player order, so with many players each friend gets a lane before anyone gets two.
    let mut ids: Vec<u8> = online.remote.keys().copied().collect();
    ids.sort_unstable();
    // Round 1: each friend's chosen Chao; round 2: another of their garden Chao.
    let racers = (0..2)
        .flat_map(|round| ids.iter().map(move |id| (round, id)))
        .filter_map(|(round, id)| {
            let r = &online.remote[id];
            let first = chosen_chao(r)?;
            let chao = if round == 0 {
                first
            } else {
                r.chao[..MAX_CARRIED_CHAO]
                    .iter()
                    .flatten()
                    .map(|c| &**c)
                    .find(|c| c.bytes != first.bytes)?
            };
            let label = format!("{}'s {}", r.name, crate::karate::chao_name(chao));
            Some((label, chao.clone(), *id))
        })
        .collect();
    crate::race::set_racers(racers);

    if let Some(pick) = room::take_race_outgoing() {
        let body = Body::RacePick {
            category: pick.category,
            course: pick.course,
            level: pick.level,
        };
        online.session.send(body, true, now);
    }
    if let Some(pick) = room::take_outgoing() {
        let body = Body::RoomPick {
            level: pick.level,
            mission: pick.mission,
            character: pick.character,
        };
        online.session.send(body, true, now);
    }

    online.frame = online.frame.wrapping_add(1);
    if online.frame.is_multiple_of(STATE_EVERY_FRAMES) && online.session.my_id().is_some() {
        let me = game::local_player();
        let state = PlayerState {
            time: online.frame,
            level: game::current_level(),
            mission: game::mission(),
            chao_area: game::current_chao_area().clamp(-128, 127) as i8,
            character: me.map_or(0, |m| m.character),
            position: me.map_or([0.0; 3], |m| [m.position.x, m.position.y, m.position.z]),
            rotation: me.map_or([0; 3], |m| m.rotation),
            anim: me.map_or(0, |m| m.anim.max(0) as u16),
            frame: me.map_or(0.0, |m| if m.frame.is_finite() { m.frame } else { 0.0 }),
            flags: me.map_or(0, |m| m.flags)
                | together::flags()
                | crate::race::flags()
                | crate::karate_live::flags(),
            room: room::wire(),
            upgrades: me.map_or(0, |m| m.upgrades),
        };
        // In a room race, our racing Chao goes out instead of our (absent) character.
        let mut state = state;
        // In a local two-player game we're hidden from the others (no level = drawn nowhere).
        if game::two_player() {
            state.level = -1;
            state.chao_area = -1;
        }
        if room::in_room()
            && let Some((position, rotation_y, progress)) = crate::race::my_racer()
        {
            state.position = position;
            state.rotation = [0, rotation_y, 0];
            state.frame = if progress.is_finite() { progress } else { 0.0 };
            state.anim = 0;
            state.flags |= game::POSE_RACE_CHAO;
        }
        if (state.flags ^ online.last_flags) & game::POSE_BALL != 0 {
            log!(
                "online: spin ball {}",
                if state.flags & game::POSE_BALL != 0 {
                    "on"
                } else {
                    "off"
                }
            );
        }
        online.last_flags = state.flags;
        online.session.send(Body::PlayerState(state), false, now);
        let (karate_state, karate_end) = crate::karate_live::take_outgoing();
        if let Some(state) = karate_state {
            online.session.send(Body::KarateState(state), false, now);
        }
        let cheers = crate::karate_live::take_cheers();
        if cheers > 0 {
            online
                .session
                .send(Body::KarateCheer { count: cheers }, true, now);
        }
        if let Some((code, result, hp)) = karate_end {
            online
                .session
                .send(Body::KarateEnd { code, result, hp }, true, now);
        }
    }
}

/// A player's Chao for races and karate: their chosen one, else their first garden Chao.
fn chosen_chao(r: &Remote) -> Option<&ChaoData> {
    r.chao[usize::from(MY_CHAO_INDEX)]
        .as_deref()
        .or(r.chao[0].as_deref())
}

/// Our hatched, valid Chao: (save slot, record) (read-only).
pub fn hatched_chao() -> Vec<(usize, ChaoData)> {
    (0..game::GARDEN_SLOTS)
        .filter_map(|slot| game::read_chao_slot(slot).map(|c| (slot, c)))
        .filter(|(_, c)| c.chao_type() != chao::TYPE_EGG && c.validate().is_ok())
        .collect()
}

/// Our chosen Chao ("My Chao": a save slot, or the first hatched one).
fn my_chosen_chao() -> Option<ChaoData> {
    let slot = MY_CHAO_SLOT.load(Ordering::Relaxed);
    let hatched = hatched_chao();
    hatched
        .iter()
        .find(|(s, _)| Some(*s) == usize::try_from(slot).ok())
        .or(hatched.first())
        .map(|(_, c)| c.clone())
}

/// F9 / config: which save slot is "My Chao" (-1 = the first hatched one). Sent right away.
pub fn set_my_chao(slot: i32) {
    MY_CHAO_SLOT.store(slot, Ordering::Relaxed);
    RESEND_MY_CHAO.store(true, Ordering::Relaxed);
}

/// Garden visits: which of our Chao visit the others in a shared garden.
pub const VISIT_MODES: [&str; 3] = ["Off", "My Chao", "Garden Chao"];
const VISITS_OFF: u8 = 0;
const VISITS_MY_CHAO: u8 = 1;
const VISITS_GARDEN: u8 = 2;
static VISITS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(VISITS_GARDEN);

/// F9 / config: garden visits mode (index into `VISIT_MODES`). Applies on the next garden.
pub fn set_visits(mode: usize) {
    VISITS.store(mode.min(VISIT_MODES.len() - 1) as u8, Ordering::Relaxed);
}

/// Parses a `VisitWith` value ("Off", "MyChao", "Garden").
pub fn parse_visits(text: &str) -> usize {
    match text.trim().to_ascii_lowercase().replace(' ', "").as_str() {
        "off" | "none" => usize::from(VISITS_OFF),
        "mychao" => usize::from(VISITS_MY_CHAO),
        _ => usize::from(VISITS_GARDEN),
    }
}

static MY_CHAO_SLOT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static RESEND_MY_CHAO: AtomicBool = AtomicBool::new(true);

/// F9 karate opponent choices while in a room at Karate: (`None` = first free, or a player
/// id, label).
pub fn karate_opponents() -> Vec<(Option<u8>, String)> {
    if !room::in_room() || !crate::karate_live::in_karate_area() {
        return Vec::new();
    }
    let guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(online) = guard.as_ref() else {
        return Vec::new();
    };
    let peers: Vec<(u8, together::Peer)> = online
        .remote
        .iter()
        .filter_map(|(&id, r)| r.latest.map(|s| (id, peer_of(id, &s))))
        .collect();
    let mut out = vec![(None, "first free player".to_string())];
    for (id, free) in crate::karate_live::candidates(&peers) {
        let name = online.remote.get(&id).map_or("?", |r| r.name.as_str());
        let label = if free {
            name.to_string()
        } else {
            format!("{name} (busy)")
        };
        out.push((Some(id), label));
    }
    out
}

/// Up to `MAX_CARRIED_CHAO` hatched, valid Chao living in `garden` (read-only).
fn carried_chao(garden: i32) -> Vec<ChaoData> {
    (0..game::GARDEN_SLOTS)
        .filter_map(game::read_chao_slot)
        .filter(|c| {
            i32::from(c.garden()) == garden
                && c.chao_type() != chao::TYPE_EGG
                && c.validate().is_ok()
        })
        .take(MAX_CARRIED_CHAO)
        .collect()
}

pub fn shutdown() {
    STOP.store(true, Ordering::Relaxed);
    if let Some(online) = ONLINE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        online.session.shutdown(Instant::now());
        log!("online: shut down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_off() {
        assert_eq!(
            parse_config(DEFAULT_CONFIG).unwrap(),
            Config {
                mode: Mode::Off,
                name: "Player".into(),
                name_tags: true,
                chao_race: true,
                my_chao: -1,
                visits: 2,
            }
        );
    }

    #[test]
    fn parses_host_and_client() {
        let c = parse_config("mode = host\nname = Regrant ; me\nport = 4000").unwrap();
        assert_eq!(
            c,
            Config {
                mode: Mode::Host { port: 4000 },
                name: "Regrant".into(),
                name_tags: true,
                chao_race: true,
                my_chao: -1,
                visits: 2,
            }
        );
        let c = parse_config("MODE=client\nhost=10.0.0.5:27015").unwrap();
        assert_eq!(
            c.mode,
            Mode::Client {
                host: "10.0.0.5:27015".parse().unwrap()
            }
        );
    }

    #[test]
    fn host_port_is_optional() {
        let c = parse_config("mode=join\nhost=10.0.0.5").unwrap();
        assert_eq!(
            c.mode,
            Mode::Client {
                host: format!("10.0.0.5:{DEFAULT_PORT}").parse().unwrap()
            }
        );
        // Names are looked up (localhost resolves everywhere).
        let c = parse_config("mode=join\nhost=localhost:4000").unwrap();
        assert!(
            matches!(c.mode, Mode::Client { host } if host.port() == 4000 && host.ip().is_loopback())
        );
    }

    #[test]
    fn reads_mod_manager_config_ini() {
        let text = "[Multiplayer]\nMode=Join\nName=Regrant\nHost=10.0.0.5:27015\nPort=27015\n";
        let c = parse_config(text).unwrap();
        assert_eq!(c.name, "Regrant");
        assert_eq!(
            c.mode,
            Mode::Client {
                host: "10.0.0.5:27015".parse().unwrap()
            }
        );
        let c = parse_config("[Multiplayer]\nMode=Host\nPort=4000\n").unwrap();
        assert_eq!(c.mode, Mode::Host { port: 4000 });
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse_config("mode = server").is_err());
        assert!(parse_config("mode = host\nport = 99999").is_err());
        assert!(parse_config("mode = client\nhost = nowhere").is_err());
        assert!(parse_config("name = abcdefghijklmnopqrstuvwxyz").is_err());
    }
}

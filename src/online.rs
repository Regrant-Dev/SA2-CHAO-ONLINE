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
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::chao::{self, ChaoData};
use crate::game;
use crate::hud;
use crate::log;
use crate::net::files::SkinStore;
use crate::net::protocol::{
    Body, BroughtChao, CHAO_SLOTS, ChaoLock, GardenChao, HELD_OWNER, MAX_CARRIED_CHAO,
    MY_CHAO_INDEX, PlayerKey, PlayerState, STATUS_GROUNDED, STATUS_HOLDING,
};
use crate::net::session::{Client, Event, Host, Identity, Session};
use crate::net::udp::Endpoint;
use crate::puppet;
use crate::room;
use crate::skin_sync::SkinSync;
use crate::timeline::Timeline;
use crate::together;
use crate::visitors;

pub const DEFAULT_PORT: u16 = 43_902;
/// Send our state every N frames (60 fps / 2 = 30 Hz).
const STATE_EVERY_FRAMES: u32 = 2;
/// Below this ground speed (units a frame, the game's own) we stand still: our puppet's skin
/// stands too (`STATUS_STILL`). A skin walks from 0.15 by default.
const STILL_SPEED: f32 = 0.05;
/// A player whose state came this recently is sending (not idling, e.g. tabbed out).
const ALIVE: Duration = Duration::from_secs(2);
/// How often our visitors' looks are checked for changes.
const LOOK_EVERY_FRAMES: u32 = 60;
/// A friend counts for keeping the garden while their updates are this fresh.
const KEEPER_FRESH: Duration = Duration::from_millis(1500);
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
    /// When others may change our Chao (`OthersChangeChao`).
    pub lock: ChaoLock,
    /// Visitors 1..=4: a save slot, `VISITOR_AUTO` or `VISITOR_NONE`.
    pub visitors: [i32; MAX_CARRIED_CHAO],
    /// Mode of rooms we create.
    pub room_mode: crate::room::Mode,
    /// Our player key (`PlayerKey`, 32 hex digits), once made.
    pub key: Option<PlayerKey>,
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
            let addr = resolve(&full).ok_or_else(|| {
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
        // Older files' `VisitWith` (visits on/off) is ignored: online, we always bring.
        lock: ChaoLock::parse(get("otherschangechao")),
        visitors: std::array::from_fn(|i| parse_visitor(get(&format!("visitor{}", i + 1)))),
        room_mode: crate::room::Mode::parse(get("roommode")),
        key: crate::net::roster::key_from_hex(get("playerkey").trim()),
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
    /// When `latest` came.
    heard: Instant,
    timeline: Timeline,
    /// Chao this player brought to their current garden (they never change our save).
    /// Their garden Chao (visitors), then their chosen Chao (`MY_CHAO_INDEX`).
    chao: [Option<Box<ChaoData>>; CHAO_SLOTS],
    /// Last complete owner report, retained across our own garden changes. A paused owner's
    /// already-known visitors can appear immediately when we re-enter their garden.
    garden: Option<GardenSnapshot>,
    /// The skin they wear ("" for none).
    skin: String,
}

struct GardenSnapshot {
    area: i8,
    states: [Option<GardenChao>; MAX_CARRIED_CHAO],
    heard: Instant,
}

impl GardenSnapshot {
    fn new(area: i8, states: &[GardenChao], now: Instant) -> Self {
        let mut slots = [None; MAX_CARRIED_CHAO];
        for state in states {
            if let Some(slot) = slots.get_mut(usize::from(state.index)) {
                *slot = Some(*state);
            }
        }
        Self {
            area,
            states: slots,
            heard: now,
        }
    }

    fn available(&self, area: i32, owner_active: bool, now: Instant) -> bool {
        let limit = if owner_active {
            Duration::from_secs(8)
        } else {
            Duration::from_secs(300)
        };
        i32::from(self.area) == area && now.duration_since(self.heard) < limit
    }
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
        action: s.action,
        status: s.status,
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
    /// Our visitors' looks as last sent (index -> type and animal bytes).
    sent_looks: std::collections::HashMap<u8, Vec<u8>>,
    /// "My Chao" as last sent to the others (live karate uses it on both sides).
    my_chao: Option<ChaoData>,
    /// The Chao we picked for a Chao Race, sent in place of My Chao while racing so friends'
    /// games show it in our lane.
    race_chao: Option<ChaoData>,
    /// Client mode: where to rejoin, and when (set after a disconnect).
    rejoin: Option<(SocketAddr, Identity, Option<Instant>)>,
    /// The host's shared skins, coming down in the background.
    skins: SkinSync,
    /// Our skin as last told to the others, and when.
    skin_sent: Option<(String, Instant)>,
    /// What we bring as last sent to the host (`Brought`: awake, then garden, row, copy
    /// number and identity of each); `None` sends it again.
    brought: Option<BroughtSeen>,
}

/// What a `Brought` said: our save loaded, our lock, then each Chao's garden, row, copy number
/// and identity.
type BroughtSeen = (bool, ChaoLock, Vec<(u8, u8, u8, u64)>);

/// Our skin goes out again this often (for players who joined since).
const SKIN_WORN_EVERY: Duration = Duration::from_secs(10);

static ONLINE: Mutex<Option<Online>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);
static MOD_PATH: OnceLock<String> = OnceLock::new();

/// Reads `config.ini` (F9 menu / SA Mod Manager), or an `online.ini` from older versions;
/// defaults (off) when neither exists.
/// Longest we wait for a host name to be looked up (this runs when settings are applied, and
/// the game waits for it).
const RESOLVE_LIMIT: Duration = Duration::from_secs(3);

/// `host:port` as an address: an IP at once, a name looked up on a thread of its own and given
/// up after `RESOLVE_LIMIT`. Our socket is IPv4, so an IPv4 address is preferred ("localhost"
/// can give ::1 first).
fn resolve(full: &str) -> Option<SocketAddr> {
    if let Ok(addr) = full.parse::<SocketAddr>() {
        return Some(addr);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let name = full.to_string();
    thread::Builder::new()
        .name("sa2-chao-online resolve".into())
        .spawn(move || {
            let found = name.to_socket_addrs().ok().and_then(|a| {
                let all: Vec<SocketAddr> = a.collect();
                all.iter().find(|a| a.is_ipv4()).or(all.first()).copied()
            });
            let _ = tx.send(found);
        })
        .ok()?;
    match rx.recv_timeout(RESOLVE_LIMIT) {
        Ok(found) => found,
        Err(_) => {
            log!("online: looking up '{full}' took too long");
            None
        }
    }
}

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
    set_lock(config.lock);
    for (i, slot) in config.visitors.into_iter().enumerate() {
        set_visitor(i, slot);
    }
    crate::room::set_mode(config.room_mode);
    let me = Identity {
        name: config.name.clone(),
        mod_version: env!("CARGO_PKG_VERSION").into(),
        game_build: game::game_build(),
        key: config.key.unwrap_or_else(player_key),
    };
    let now = Instant::now();
    let (session, rejoin) = match config.mode {
        Mode::Off => return Ok(None),
        Mode::Host { port } => {
            let ep = Endpoint::bind(SocketAddr::from(([0, 0, 0, 0], port)))
                .map_err(|e| format!("can't listen on port {port}: {e}"))?;
            log!("online: hosting on port {port} as '{}'", me.name);
            hud::toast(format!("Hosting on port {port} as {}", me.name));
            let mut host = Host::new(ep, me, now);
            // Our skins go to whoever joins.
            let store = SkinStore::scan(&skins_dir());
            if !store.is_empty() {
                log!("online: sharing {} skin file(s)", store.len());
                host.set_files(Some(Arc::new(store)));
            }
            (Session::Host(host), None)
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
        sent_looks: std::collections::HashMap::new(),
        my_chao: None,
        race_chao: None,
        rejoin,
        skins: SkinSync::new(skins_dir()),
        skin_sent: None,
        brought: None,
    }))
}

static KEY: OnceLock<PlayerKey> = OnceLock::new();

/// Our player key, if it has been read or made yet (for writing config.ini again).
pub fn known_key() -> Option<PlayerKey> {
    KEY.get().copied()
}

/// Our player key (config.ini `PlayerKey`): the host knows our Chao by it from one session to
/// the next. Made once, at random, and written to config.ini (which updates never replace).
pub fn player_key() -> PlayerKey {
    *KEY.get_or_init(|| {
        let dir = Path::new(mod_path());
        let config = dir.join("config.ini");
        let text = ["config.ini", "online.ini"]
            .iter()
            .find_map(|f| fs::read_to_string(dir.join(f)).ok());
        if let Some(key) = text
            .as_deref()
            .and_then(|t| parse_config(t).ok())
            .and_then(|c| c.key)
        {
            return key;
        }
        let key = new_key();
        let mut text = text.unwrap_or_else(|| DEFAULT_CONFIG.to_string());
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "PlayerKey={}\n",
            crate::net::roster::key_hex(&key)
        ));
        match fs::write(&config, text) {
            Ok(()) => log!("online: made our player key (config.ini PlayerKey)"),
            Err(e) => log!("online: can't write our player key to config.ini: {e}"),
        }
        key
    })
}

/// 128 random bits (the standard library's hasher keys come from the system's randomness).
fn new_key() -> PlayerKey {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut key = [0; 16];
    for (i, half) in key.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_usize(i);
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        half.copy_from_slice(&h.finish().to_le_bytes());
    }
    key
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
/// Our player id in the lobby.
pub fn my_id() -> Option<u8> {
    let guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().and_then(|online| online.session.my_id())
}

/// The other players: (id, name, latest state).
pub fn others() -> Vec<(u8, String, Option<PlayerState>)> {
    let guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map_or_else(Vec::new, |online| {
        online
            .remote
            .iter()
            .map(|(&id, r)| (id, r.name.clone(), r.latest))
            .collect()
    })
}

/// A remote kart's selected skin; read outside the kart/cache locks.
pub fn worn_skin(id: u8) -> Option<String> {
    let guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref()?.remote.get(&id).map(|r| r.skin.clone())
}

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
                    // A bug in one message's handling must not stop networking for good (the
                    // panic hook logs it).
                    let pumped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        pump(online, Instant::now());
                    }));
                    if pumped.is_err() {
                        log!("online: the network thread recovered from a panic");
                    }
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
                // A new host hasn't had our garden's ground.
                crate::land_share::reset();
                for b in online.skins.start(now) {
                    online.session.send(b, true, now);
                }
                online.skin_sent = None;
                online.resend_chao = true;
                online.brought = None;
                RESEND_MY_CHAO.store(true, Ordering::Relaxed);
            }
            Event::Disconnected { reason } => {
                log!("online: disconnected: {reason}");
                online.remote.clear();
                crate::held::clear();
                crate::garden::set_locked(Vec::new());
                online.skins.stop();
                crate::garden::player_left(None);
                // A different version won't get any better by retrying.
                if reason.starts_with("rejected: protocol") || reason.starts_with("rejected: mod") {
                    online.rejoin = None;
                }
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
                        heard: Instant::now(),
                        timeline: Timeline::default(),
                        chao: Default::default(),
                        garden: None,
                        skin: String::new(),
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
                crate::garden::player_left(Some(id));
                crate::carried::player_left(id);
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
                        if remote.garden.as_ref().is_some_and(|g| {
                            state.level != game::LEVEL_CHAO_WORLD || g.area != state.chao_area
                        }) {
                            remote.garden = None;
                        }
                    }
                    let clock = frames_since(online.started, now);
                    remote.timeline.push(state.time, to_pose(&state), clock);
                    remote.latest = Some(state);
                    remote.heard = now;
                }
            }
            Event::Game {
                from,
                body: Body::Chao { index, data },
            } => {
                // A visitor already here takes on its new look (animal parts, evolution).
                crate::garden::received_look(from, index, &data);
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
            Event::Game {
                from,
                body: Body::GardenKeeper { area, term },
            } => {
                if in_my_garden(online, from) {
                    crate::keeper::received(from, area, term);
                }
            }
            Event::Game {
                from,
                body: Body::GardenChao { owner, list },
            } => {
                if from == owner
                    && let Some(remote) = online.remote.get_mut(&owner)
                    && let Some(state) = remote.latest
                    && state.level == game::LEVEL_CHAO_WORLD
                    && (1..=3).contains(&state.chao_area)
                {
                    remote.garden = Some(GardenSnapshot::new(state.chao_area, &list, now));
                }
                // Only from a friend in our garden: elsewhere their Chao aren't visiting us (and
                // ours of theirs go home).
                let here = online
                    .remote
                    .get(&from)
                    .and_then(|r| r.latest)
                    .is_some_and(|s| {
                        s.level == game::LEVEL_CHAO_WORLD
                            && i32::from(s.chao_area) == game::current_chao_area()
                    });
                if here {
                    crate::garden::received(from, owner, list);
                }
            }
            Event::Game {
                from,
                body: Body::GardenItems { area, items },
            } => crate::items::received(from, area, items),
            Event::Game {
                from,
                body: Body::GardenTrees { area, trees, .. },
            } => crate::trees::received_trees(from, area, trees),
            Event::Game {
                body: Body::ServerGarden { area, trees, toys },
                ..
            } => {
                crate::trees::received_server(area, trees);
                crate::garden_toys::received_server(area, toys);
            }
            Event::Game {
                body: Body::ServerItems { area, items },
                ..
            } => crate::items::received_server(area, items),
            Event::Game {
                body: Body::ServerChao { area, owner, list },
                ..
            } => {
                // A held Chao's stream: only for the ones the host told us of.
                let list = if owner >= HELD_OWNER {
                    crate::held::received_state(area, owner, list)
                } else {
                    list
                };
                crate::garden::received_server(area, owner, list);
            }
            Event::Game {
                from,
                body: Body::HeldChao { owner, index, chao },
            } => {
                if from == crate::garden::SERVER {
                    crate::held::received(owner, index, chao);
                }
            }
            Event::Game {
                from,
                body:
                    Body::SaveChao {
                        identity,
                        nth,
                        spans,
                    },
            } => {
                if from == crate::garden::SERVER {
                    crate::held::received_save(identity, nth, spans);
                }
            }
            // To the host only.
            Event::Game {
                body: Body::Brought { .. },
                ..
            } => {}
            // The Chao locked against us (only the host says so).
            Event::Game {
                from,
                body: Body::ChaoLocked { chao },
            } => {
                if from == crate::garden::SERVER {
                    crate::garden::set_locked(chao);
                }
            }
            Event::Game {
                from,
                body:
                    Body::ChaoLive {
                        owner,
                        index,
                        bytes,
                    },
            } => {
                if from == crate::garden::SERVER && game::in_garden() {
                    crate::garden::received_live(owner, index, bytes);
                }
            }
            // To the host only.
            Event::Game {
                body: Body::GardenLand { .. } | Body::ChaoTouched { .. },
                ..
            } => {}
            Event::Game {
                from,
                body: Body::CarriedChao { chao, offset },
            } => crate::carried::received(from, chao, offset),
            Event::Game {
                from,
                body: Body::ToyMode { area, toy, mode },
            } => {
                let keeper = crate::keeper::current() == Some(from);
                crate::garden_toys::received(keeper, area, toy, mode);
            }
            Event::Game {
                from,
                body: Body::GardenEggs { area, eggs },
            } => {
                if i32::from(area) == game::current_chao_area() {
                    crate::eggs::received(from, eggs);
                }
            }
            Event::Game {
                from,
                body:
                    Body::EggHeld {
                        owner,
                        index,
                        held,
                        position,
                    },
            } => {
                if Some(owner) == online.session.my_id() {
                    crate::eggs::received_held(from, index, held, position);
                }
            }
            Event::Game {
                body:
                    Body::TreeAction {
                        area,
                        slot,
                        action,
                        kind,
                    },
                ..
            } => crate::trees::received_action(area, slot, action, kind),
            Event::Game {
                from,
                body:
                    Body::RaceStatus {
                        level,
                        mission,
                        progress,
                        finish,
                    },
            } => {
                crate::stage_race::received_status(from, level, mission, progress, finish);
                if crate::bosses::should_share_clear(
                    level,
                    room::shares_room(from),
                    room::mode() == room::Mode::Coop,
                    finish.is_some(),
                ) {
                    crate::bosses::received_clear(level, mission);
                }
            }
            Event::Game {
                body:
                    Body::Route {
                        level,
                        mission,
                        points,
                    },
                ..
            } => crate::stage_race::received_route(level, mission, points),
            Event::Game {
                from,
                body: Body::KartPick { course },
            } => {
                let name = online
                    .remote
                    .get(&from)
                    .map_or_else(|| format!("player {from}"), |r| r.name.clone());
                crate::kart::received_pick(course, from, &name);
            }
            Event::Game {
                from,
                body:
                    Body::KartState {
                        position,
                        rotation,
                        yaw,
                        wheel,
                        kind,
                        accel,
                        tilt,
                        frame,
                    },
            } => {
                let kart = crate::kart::MyKart {
                    position,
                    rotation,
                    yaw,
                    wheel,
                    kind,
                    accel,
                    tilt,
                };
                crate::kart::received_state(from, kart, frame);
            }
            Event::Game {
                from,
                body:
                    Body::HuntSet {
                        level,
                        mission,
                        pieces,
                    },
            } => {
                if room::shares_room(from) {
                    crate::hunt::received_set(from, level, mission, pieces);
                }
            }
            Event::Game {
                body:
                    Body::HuntTaken {
                        level,
                        mission,
                        sig,
                        slot,
                        player,
                    },
                ..
            } => crate::hunt::received_taken(level, mission, sig, slot, player),
            // For the host's referee only (net/referee.rs).
            Event::Game {
                body: Body::HuntClaim { .. },
                ..
            } => {}
            Event::Game {
                from,
                body: Body::Chat { text },
            } => {
                let name = online
                    .remote
                    .get(&from)
                    .map_or_else(|| format!("player {from}"), |r| r.name.clone());
                log!("chat: {name}: {text}");
                crate::chat::received(&name, &text);
            }
            Event::Game {
                body: Body::ItemOwner { id, owner },
                ..
            } => crate::items::received_owner(id, owner),
            // A Chao the server runs borrowed one of our items (only the server says so).
            Event::Game {
                from,
                body: Body::ItemSteer { item, gone, .. },
            } => {
                if from == crate::garden::SERVER {
                    crate::items::received_steer(item, gone);
                }
            }
            // Claims are for the host's referee (net/referee.rs), which doesn't pass them on.
            Event::Game {
                body: Body::ItemClaim { .. },
                ..
            } => {}
            Event::Game {
                from,
                body:
                    Body::ChaoChanges {
                        owner,
                        index,
                        changes,
                    },
            } => {
                // Changes to our own Chao, from the server (it runs the garden) or someone in our
                // garden (they touched it): to our save. Not someone else's while our lock says
                // no (the host drops those too).
                let me = online.session.my_id();
                let server = from == crate::garden::SERVER && game::in_garden();
                let area = game::current_chao_area().clamp(0, 3) as u8;
                if Some(owner) == me
                    && (server || (in_my_garden(online, from) && !lock().locks(area, area)))
                {
                    crate::garden::received_changes(owner, index, changes);
                }
            }
            Event::Game {
                from,
                body: Body::ChaoSounds { sounds },
            } => {
                if in_my_garden(online, from)
                    || (from == crate::garden::SERVER && game::in_garden())
                {
                    crate::garden::received_sounds(from, sounds);
                }
            }
            Event::Game {
                from,
                body:
                    Body::ChaoHeld {
                        owner,
                        index,
                        offset,
                    },
            } => {
                // Every follower needs the holder, including a player currently petting the
                // Chao locally. The brain also uses the report to drive its real object.
                if in_my_garden(online, from) {
                    crate::garden::received_held(from, owner, index, offset);
                }
            }
            Event::Game {
                from,
                body:
                    Body::StageKills {
                        level,
                        mission,
                        objects,
                    },
            } => {
                // Co-op messages count only from players in our room.
                if room::shares_room(from) {
                    crate::coop::received(level, mission, objects);
                }
            }
            Event::Game {
                from,
                body:
                    Body::StageRings {
                        level,
                        mission,
                        rings,
                    },
            } => {
                if room::shares_room(from) {
                    crate::coop::received_group_rings(level, mission, rings);
                }
            }
            Event::Game {
                from,
                body:
                    body @ (Body::BossClaim { .. }
                    | Body::BossState(_)
                    | Body::BossHit { .. }
                    | Body::BossAttacks { .. }),
            } => {
                if room::shares_room(from) {
                    crate::boss_follow::received(from, body);
                }
            }
            Event::Game {
                from,
                body: Body::GardenUnlocks { gardens },
            } => {
                if room::shares_room(from) {
                    crate::unlocks::received(gardens);
                }
            }
            Event::Game {
                from,
                body:
                    Body::UpgradeGot {
                        level,
                        mission,
                        index,
                        character,
                        mask,
                    },
            } => {
                if room::shares_room(from) {
                    crate::coop::received_upgrade(level, mission, (index, character, mask));
                }
            }
            Event::Game {
                from,
                body:
                    Body::EnemyStates {
                        level,
                        mission,
                        enemies,
                    },
            } => {
                if room::shares_room(from) {
                    crate::enemies::received(from, level, mission, enemies);
                }
            }
            Event::Game {
                from,
                body:
                    Body::LockOn {
                        level,
                        mission,
                        gun,
                        locked,
                        fired,
                    },
            } => {
                if room::shares_room(from) {
                    crate::lockon::received(from, (level, mission, gun, locked, fired));
                }
            }
            Event::Game {
                from,
                body:
                    Body::Shot {
                        level,
                        mission,
                        kind,
                        position,
                        direction,
                    },
            } => {
                if room::shares_room(from)
                    && level == game::current_level()
                    && mission == game::mission()
                {
                    crate::shots::received((kind, position, direction));
                }
            }
            Event::Game {
                from,
                body:
                    Body::Switch {
                        level,
                        mission,
                        id,
                        on,
                    },
            } => {
                if room::shares_room(from) {
                    crate::coop::received_switch(level, mission, id, on);
                }
            }
            Event::Game {
                from,
                body:
                    Body::KeyUsed {
                        level,
                        mission,
                        key,
                    },
            } => {
                if room::shares_room(from) {
                    crate::coop::received_key(level, mission, key);
                }
            }
            Event::Game {
                from,
                body:
                    Body::Checkpoint {
                        level,
                        mission,
                        record,
                        position,
                        rotation,
                    },
            } => {
                if room::shares_room(from) {
                    crate::coop::received_checkpoint(level, mission, record, position, rotation);
                }
            }
            Event::Game {
                from,
                body:
                    Body::Ball {
                        area,
                        claim,
                        position,
                        velocity,
                        ..
                    },
            } => crate::toys::received(from, (area, claim, position, velocity)),
            Event::Game {
                from,
                body:
                    Body::BossHp {
                        level,
                        mission,
                        hp,
                        full,
                    },
            } => {
                if room::shares_room(from) {
                    crate::bosses::received(level, mission, hp, full);
                }
            }
            Event::Game {
                from,
                body: Body::SkinWorn { skin },
            } => {
                if let Some(r) = online.remote.get_mut(&from) {
                    r.skin = skin;
                }
            }
            Event::Game {
                body: body @ (Body::SkinList { .. } | Body::SkinChunk { .. }),
                ..
            } => {
                for b in online.skins.on_body(&body, now) {
                    let list = matches!(b, Body::SkinListGet { .. });
                    online.session.send(b, list, now);
                }
            }
            // Only a host answers these (session.rs).
            Event::Game {
                body: Body::SkinListGet { .. } | Body::SkinGet { .. },
                ..
            } => {}
        }
    }
    for b in online.skins.tick(now) {
        let list = matches!(b, Body::SkinListGet { .. });
        online.session.send(b, list, now);
    }
    // Our skin, now and then and when it changes (silently: nobody sees this).
    if online.session.my_id().is_some() {
        let mine = crate::skins::worn().unwrap_or_default();
        let due = online
            .skin_sent
            .as_ref()
            .is_none_or(|(s, at)| *s != mine || now.duration_since(*at) >= SKIN_WORN_EVERY);
        if due {
            online.skin_sent = Some((mine.clone(), now));
            online
                .session
                .send(Body::SkinWorn { skin: mine }, true, now);
        }
    }
}

/// `<mod>/skins`, where skins live (and shared ones come down to).
fn skins_dir() -> std::path::PathBuf {
    std::path::Path::new(mod_path()).join("skins")
}

/// Player `id` is in the garden we're in.
fn in_my_garden(online: &Online, id: u8) -> bool {
    game::in_garden()
        && online.remote.get(&id).is_some_and(|r| {
            r.latest.is_some_and(|s| {
                s.level == game::LEVEL_CHAO_WORLD
                    && i32::from(s.chao_area) == game::current_chao_area()
            })
        })
}

/// Game thread: hand remote poses to the puppets and send our own state.
pub fn on_frame() {
    let mut guard = ONLINE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(online) = guard.as_mut() else {
        room::update(None, Vec::new());
        crate::bring::set_friend_gardens(0);
        return;
    };
    let now = Instant::now();

    // Gardens others are in (bit = area): ours there show only the Chao we bring (bring.rs).
    let friend_gardens = online
        .remote
        .values()
        .filter_map(|r| r.latest)
        .filter(|s| s.level == game::LEVEL_CHAO_WORLD && (1..=3).contains(&s.chao_area))
        .fold(0u8, |mask, s| mask | 1 << s.chao_area);
    crate::bring::set_friend_gardens(friend_gardens);

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
            kind: r.latest.map_or(0, |s| s.room.1),
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
        .filter_map(|r| {
            r.timeline
                .sample(clock)
                .map(|pose| (pose, r.name.clone(), r.skin.clone()))
        })
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
    if garden > 0 {
        // Where the others in this garden are drawn, for Chao they're holding.
        let holders = online
            .remote
            .iter()
            .filter(|(_, r)| {
                r.latest
                    .is_some_and(|s| s.level == level && i32::from(s.chao_area) == garden)
            })
            .filter_map(|(&id, r)| {
                r.timeline.sample(clock).map(|p| {
                    let [x, y, z] = p.position;
                    (id, (game::Vector { x, y, z }, p.rotation[1]))
                })
            })
            .collect();
        crate::garden::set_holders(holders);
        // Who's sending their state now (an idling game sends nothing, but is still here).
        let alive = online
            .remote
            .iter()
            .filter(|(_, r)| r.heard.elapsed() < ALIVE)
            .map(|(&id, _)| id)
            .collect();
        crate::garden::set_alive(alive);
    }
    // Outside the gardens: Chao carried about (carried.rs).
    let outside = level == game::LEVEL_CHAO_WORLD && garden <= 0;
    let carriers: Vec<(u8, game::Vector, i32)> = online
        .remote
        .iter()
        .filter(|(_, r)| {
            outside
                && r.latest
                    .is_some_and(|s| s.level == level && i32::from(s.chao_area) == area)
        })
        .filter_map(|(&id, r)| {
            r.timeline.sample(clock).map(|p| {
                let [x, y, z] = p.position;
                (id, game::Vector { x, y, z }, p.rotation[1])
            })
        })
        .collect();
    if let Some(body) = crate::carried::on_frame(outside, &carriers) {
        online.session.send(body, true, now);
    }
    if RESEND_BROUGHT.swap(false, Ordering::Relaxed) {
        online.brought = None;
    }
    if garden != online.garden || RESEND_GARDEN_CHAO.swap(false, Ordering::Relaxed) {
        // Into or out of a garden, the save has its Chao as they are now: the host's held ones
        // start from that.
        online.garden = garden;
        online.resend_chao = true;
        online.brought = None;
    }
    // What we bring to every garden, for the host to keep (and show while we're elsewhere).
    if online.session.my_id().is_some()
        && (online.brought.is_none() || online.frame.is_multiple_of(BROUGHT_EVERY_FRAMES))
    {
        let (awake, list) = brought_now();
        let lock = lock();
        let seen = (
            awake,
            lock,
            list.iter()
                .map(|c| (c.garden, c.row, c.nth, c.data.identity()))
                .collect(),
        );
        if online.brought.as_ref() != Some(&seen) {
            log!(
                "online: bringing {} Chao in all to the shared gardens (others may change them: {}){}",
                list.len(),
                lock.ini(),
                if awake { "" } else { " (save not loaded yet)" }
            );
            online.brought = Some(seen);
            online
                .session
                .send(Body::Brought { awake, lock, list }, true, now);
        }
    }
    crate::held::on_frame(garden);
    if online.resend_chao && garden > 0 && online.session.my_id().is_some() {
        online.resend_chao = false;
        // Sent in full just below: look changes count from here.
        online.sent_looks.clear();
        // Each visitor keeps its row number as its index (friends see "Regrant 2").
        let rows = carried_chao(garden);
        let mine: Vec<(usize, (usize, ChaoData))> = rows
            .into_iter()
            .enumerate()
            .filter_map(|(row, c)| c.map(|c| (row, c)))
            .collect();
        log!("online: bringing {} chao to garden {garden}", mine.len());
        let slots: Vec<(u8, usize)> = mine
            .iter()
            .map(|(index, (slot, _))| (*index as u8, *slot))
            .collect();
        crate::garden::set_mine(slots.clone());
        crate::eggs::set_mine(slots);
        for (index, (_, chao)) in mine {
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

    for (id, index) in crate::garden::take_left()
        .into_iter()
        .chain(crate::eggs::take_left())
    {
        visitors::forget(id, index);
    }
    if garden > 0 {
        for (&id, remote) in &online.remote {
            let Some(state) = remote.latest else { continue };
            if state.level != level || i32::from(state.chao_area) != garden {
                continue;
            }
            let [x, y, z] = state.position;
            for (index, chao) in remote.chao.iter().take(MAX_CARRIED_CHAO).enumerate() {
                let initial = remote
                    .garden
                    .as_ref()
                    .filter(|g| g.available(garden, remote.heard.elapsed() < ALIVE, now))
                    .and_then(|g| g.states[index]);
                // Only while their game sends it (they can take it home any time).
                if let Some(chao) = chao
                    && i32::from(chao.garden()) == garden
                    && (crate::garden::visiting(id, index as u8)
                        || crate::eggs::visiting(id, index as u8)
                        || initial.is_some())
                {
                    visitors::spawn_remote(
                        id,
                        index as u8,
                        (**chao).clone(),
                        game::Vector { x, y, z },
                        initial,
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
            crate::race::set_target(id, s.position, s.anim);
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
    if garden > 0 {
        let sounds = crate::garden::take_sounds();
        for sounds in sounds.chunks(crate::net::protocol::MAX_CHAO_SOUNDS) {
            let body = Body::ChaoSounds {
                sounds: sounds.to_vec(),
            };
            online.session.send(body, false, now);
        }
        let (changes, held) = crate::garden::take_outgoing();
        for (owner, index, changes) in changes {
            let body = Body::ChaoChanges {
                owner,
                index,
                changes,
            };
            online.session.send(body, true, now);
        }
        if online.frame.is_multiple_of(STATE_EVERY_FRAMES) {
            for (owner, index, offset) in held {
                let body = Body::ChaoHeld {
                    owner,
                    index,
                    offset,
                };
                online.session.send(body, false, now);
            }
            for (owner, index, position, rotation_y, motion, mood) in crate::garden::take_touched()
            {
                let body = Body::ChaoTouched {
                    owner,
                    index,
                    position,
                    rotation_y,
                    motion,
                    mood,
                };
                online.session.send(body, false, now);
            }
            for (owner, list) in crate::garden::streams() {
                online
                    .session
                    .send(Body::GardenChao { owner, list }, false, now);
            }
            // A visitor's look changed (it took an animal, evolved): send it again.
            if online.frame.is_multiple_of(LOOK_EVERY_FRAMES) {
                for (index, slot) in crate::garden::mine() {
                    let Some(chao) = game::read_chao_slot(slot) else {
                        continue;
                    };
                    let look = crate::garden::look(&chao);
                    if online.sent_looks.get(&index) != Some(&look)
                        && online.sent_looks.insert(index, look).is_some()
                    {
                        log!("online: our Chao {index} changed its look; sending it again");
                        let body = Body::Chao {
                            index,
                            data: Box::new(chao),
                        };
                        online.session.send(body, true, now);
                    }
                }
            }
        }
    }
    // Shared garden items: ours to everyone, claims to the referee.
    let area = u8::try_from(garden)
        .ok()
        .filter(|a| (1..=3).contains(a))
        .unwrap_or(0);
    let friends: Vec<u8> = online
        .remote
        .iter()
        .filter(|(_, r)| {
            area != 0
                && r.latest.is_some_and(|s| {
                    s.level == game::LEVEL_CHAO_WORLD && i32::from(s.chao_area) == i32::from(area)
                })
        })
        .map(|(&id, _)| id)
        .collect();
    // Who keeps the garden (runs its Chao, trees and toys): among the friends whose updates
    // are coming (a paused game sends none).
    let fresh: Vec<u8> = friends
        .iter()
        .copied()
        .filter(|id| {
            online
                .remote
                .get(id)
                .is_some_and(|r| now.duration_since(r.heard) < KEEPER_FRESH)
        })
        .collect();
    if let Some(body) = crate::keeper::on_frame(online.session.my_id(), area, &fresh) {
        online.session.send(body, false, now);
    }
    // The garden's ground for a host that runs its Chao (land_share.rs).
    for body in crate::land_share::on_frame(area) {
        online.session.send(body, true, now);
    }
    if let Some(((area, claim, position, velocity), flags)) =
        crate::toys::on_frame(online.session.my_id(), area, &friends)
    {
        let body = Body::Ball {
            area,
            claim,
            position,
            velocity,
            flags,
        };
        online.session.send(body, false, now);
    }
    for (area, items) in crate::items::on_frame(online.session.my_id(), area) {
        online
            .session
            .send(Body::GardenItems { area, items }, false, now);
    }
    // Friends' whistles reach our Chao.
    let whistlers: Vec<(u8, game::Vector, u8)> = online
        .remote
        .iter()
        .filter(|(id, _)| friends.contains(id))
        .filter_map(|(&id, r)| {
            let s = r.latest?;
            let [x, y, z] = s.position;
            Some((id, game::Vector { x, y, z }, s.action))
        })
        .collect();
    crate::whistle::on_frame(&whistlers);
    // Visiting eggs: ours to everyone; the friends' we hold, to them.
    if area != 0 {
        let where_friends = whistlers.iter().map(|&(id, p, _)| (id, p)).collect();
        for body in crate::eggs::on_frame(area, &where_friends) {
            let reliable = matches!(body, Body::EggHeld { .. });
            online.session.send(body, reliable, now);
        }
    }
    // Shared trees: the keeper's to everyone (again each second), what we did to copies of them
    // to the keeper.
    for body in crate::trees::on_frame(online.session.my_id(), area) {
        let reliable = matches!(body, Body::TreeAction { .. });
        online.session.send(body, reliable, now);
    }
    // Shared toys: jumps go with the garden's keeper.
    for body in crate::garden_toys::on_frame(area, crate::keeper::keeping()) {
        online.session.send(body, true, now);
    }
    if let Some(course) = crate::kart::take_pick() {
        online.session.send(Body::KartPick { course }, true, now);
    }
    if online.frame.is_multiple_of(STATE_EVERY_FRAMES)
        && let Some((kart, frame)) = crate::kart::take_mine()
    {
        let body = Body::KartState {
            position: kart.position,
            rotation: kart.rotation,
            yaw: kart.yaw,
            wheel: kart.wheel,
            kind: kart.kind,
            accel: kart.accel,
            tilt: kart.tilt,
            frame,
        };
        online.session.send(body, false, now);
    }
    let (set, claims) = crate::hunt::take_outgoing();
    if let Some((level, mission, pieces)) = set {
        let body = Body::HuntSet {
            level,
            mission,
            pieces,
        };
        online.session.send(body, false, now);
    }
    for (level, mission, sig, slot) in claims {
        let body = Body::HuntClaim {
            level,
            mission,
            sig,
            slot,
        };
        online.session.send(body, true, now);
    }
    let (status, route) = crate::stage_race::take_outgoing();
    if let Some((level, mission, progress, finish, reliable)) = status {
        let body = Body::RaceStatus {
            level,
            mission,
            progress,
            finish,
        };
        online.session.send(body, reliable, now);
    }
    if let Some((level, mission, points)) = route {
        online.session.send(
            Body::Route {
                level,
                mission,
                points,
            },
            true,
            now,
        );
    }
    for text in crate::chat::take_outgoing() {
        log!("chat: me: {text}");
        online.session.send(Body::Chat { text }, true, now);
    }
    for id in crate::items::take_claims() {
        online.session.send(Body::ItemClaim { id }, true, now);
    }
    for (kind, position, direction) in crate::shots::take_outgoing() {
        let body = Body::Shot {
            level: game::current_level(),
            mission: game::mission(),
            kind,
            position,
            direction,
        };
        online.session.send(body, false, now);
    }
    for (level, mission, id) in crate::coop::take_switches() {
        let body = Body::Switch {
            level,
            mission,
            id,
            on: true,
        };
        online.session.send(body, true, now);
    }
    for (level, mission, key) in crate::coop::take_keys() {
        online.session.send(
            Body::KeyUsed {
                level,
                mission,
                key,
            },
            true,
            now,
        );
    }
    if let Some((level, mission, hp, full)) = crate::bosses::take_outgoing() {
        let body = Body::BossHp {
            level,
            mission,
            hp,
            full,
        };
        online.session.send(body, true, now);
    }
    if let Some((level, mission, checkpoints)) = crate::coop::take_checkpoints() {
        for (record, position, rotation) in checkpoints {
            let body = Body::Checkpoint {
                level,
                mission,
                record,
                position,
                rotation,
            };
            online.session.send(body, true, now);
        }
    }
    if let Some(gardens) = crate::unlocks::on_frame(room::in_room()) {
        online
            .session
            .send(Body::GardenUnlocks { gardens }, true, now);
    }
    for (level, mission, (index, character, mask)) in crate::coop::take_upgrades() {
        let body = Body::UpgradeGot {
            level,
            mission,
            index,
            character,
            mask,
        };
        online.session.send(body, true, now);
    }
    if let Some((level, mission, enemies)) = crate::enemies::take_outgoing() {
        let body = Body::EnemyStates {
            level,
            mission,
            enemies,
        };
        online.session.send(body, false, now);
    }
    for (body, reliable) in crate::boss_follow::take_outgoing() {
        online.session.send(body, reliable, now);
    }
    if let Some((level, mission, gun, locked, fired)) = crate::lockon::take_outgoing() {
        let body = Body::LockOn {
            level,
            mission,
            gun,
            locked,
            fired,
        };
        online.session.send(body, true, now);
    }
    if let Some((level, mission, rings)) = crate::coop::take_group_rings() {
        let body = Body::StageRings {
            level,
            mission,
            rings,
        };
        online.session.send(body, true, now);
    }
    if let Some((level, mission, objects)) = crate::coop::take_outgoing() {
        let body = Body::StageKills {
            level,
            mission,
            objects,
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
                | crate::karate_live::flags()
                | if game::between_areas() {
                    game::POSE_LOADING
                } else {
                    0
                },
            room: room::wire(),
            upgrades: me.map_or(0, |m| m.upgrades),
            action: me.map_or(0, |m| m.action),
            status: me.map_or(0, |m| {
                (if m.grounded { STATUS_GROUNDED } else { 0 })
                    | if m.holding { STATUS_HOLDING } else { 0 }
                    | if game::local_invulnerable() {
                        crate::net::protocol::STATUS_INVULNERABLE
                    } else {
                        0
                    }
                    | if m.grounded && m.speed < STILL_SPEED {
                        crate::net::protocol::STATUS_STILL
                    } else {
                        0
                    }
            }),
        };
        // In a room race, our racing Chao goes out instead of our (absent) character.
        let mut state = state;
        // In a local two-player game we're hidden from the others (no level = drawn nowhere).
        if game::two_player() {
            state.level = -1;
            state.chao_area = -1;
        }
        if room::in_room()
            && let Some((position, rotation_y, progress, motion)) = crate::race::my_racer()
        {
            state.position = position;
            state.rotation = [0, rotation_y, 0];
            state.frame = if progress.is_finite() { progress } else { 0.0 };
            state.anim = motion;
            state.flags |= game::POSE_RACE_CHAO;
        }
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
/// Our valid Chao, eggs too (they visit as well).
pub fn garden_chao() -> Vec<(usize, ChaoData)> {
    (0..game::GARDEN_SLOTS)
        .filter_map(|slot| game::read_chao_slot(slot).map(|c| (slot, c)))
        .filter(|(_, c)| c.validate().is_ok())
        .collect()
}

pub fn hatched_chao() -> Vec<(usize, ChaoData)> {
    (0..game::GARDEN_SLOTS)
        .filter_map(|slot| game::read_chao_slot(slot).map(|c| (slot, c)))
        .filter(|(_, c)| c.chao_type() != chao::TYPE_EGG && c.validate().is_ok())
        .collect()
}

/// Our chosen Chao ("My Chao": a save slot, or the first hatched one).
fn my_chosen_chao() -> Option<ChaoData> {
    my_chosen_slot().map(|(_, c)| c)
}

/// "My Chao" and its save slot.
fn my_chosen_slot() -> Option<(usize, ChaoData)> {
    let slot = MY_CHAO_SLOT.load(Ordering::Relaxed);
    let hatched = hatched_chao();
    hatched
        .iter()
        .find(|(s, _)| Some(*s) == usize::try_from(slot).ok())
        .or(hatched.first())
        .cloned()
}

/// F9 / config: which save slot is "My Chao" (-1 = the first hatched one). Sent right away.
pub fn set_my_chao(slot: i32) {
    MY_CHAO_SLOT.store(slot, Ordering::Relaxed);
    RESEND_MY_CHAO.store(true, Ordering::Relaxed);
    // It visits first: the garden visitors may change too.
    RESEND_GARDEN_CHAO.store(true, Ordering::Relaxed);
}

/// Chao each player brings to a shared garden: every visitor row (`MAX_CARRIED_CHAO`, F9
/// Visitor 1..4). The rest of theirs there stay home meanwhile (bring.rs).
pub const BRING_CHAO: usize = MAX_CARRIED_CHAO;

/// When others may change our Chao (`ChaoLock::to_u8`).
static LOCK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static RESEND_BROUGHT: AtomicBool = AtomicBool::new(false);

/// F9 / config: when others may change our Chao. The host hears of it at once (`Brought`).
pub fn set_lock(lock: ChaoLock) {
    LOCK.store(lock.to_u8(), Ordering::Relaxed);
    RESEND_BROUGHT.store(true, Ordering::Relaxed);
}

/// When others may change our Chao.
pub fn lock() -> ChaoLock {
    ChaoLock::from_u8(LOCK.load(Ordering::Relaxed)).unwrap_or_default()
}

static RESEND_GARDEN_CHAO: AtomicBool = AtomicBool::new(false);

/// Our Chao changed (an egg hatched, a Chao moved gardens): work out the visitors again.
pub fn refresh_visitors() {
    RESEND_GARDEN_CHAO.store(true, Ordering::Relaxed);
}

/// A visitor row: pick Chao automatically, or send nobody.
pub const VISITOR_AUTO: i32 = -1;
pub const VISITOR_NONE: i32 = -2;
static VISITORS: [std::sync::atomic::AtomicI32; MAX_CARRIED_CHAO] =
    [const { std::sync::atomic::AtomicI32::new(VISITOR_AUTO) }; MAX_CARRIED_CHAO];

/// F9 / config: which Chao is visitor `i` (a save slot, auto or none). Applies right away.
pub fn set_visitor(i: usize, slot: i32) {
    if let Some(v) = VISITORS.get(i) {
        v.store(slot, Ordering::Relaxed);
        RESEND_GARDEN_CHAO.store(true, Ordering::Relaxed);
    }
}

/// Visitor rows on a Chao an earlier row has: those go back to Auto.
pub fn tidy_visitors(mut rows: [i32; MAX_CARRIED_CHAO]) -> [i32; MAX_CARRIED_CHAO] {
    for i in 1..rows.len() {
        if rows[i] >= 0 && rows[..i].contains(&rows[i]) {
            rows[i] = VISITOR_AUTO;
        }
    }
    rows
}

/// Parses a `Visitor1`..`Visitor4` value: a save slot, "Auto" (-1, the default) or "None" (-2).
pub fn parse_visitor(text: &str) -> i32 {
    match text.trim().to_ascii_lowercase().as_str() {
        "none" | "off" => VISITOR_NONE,
        t => t
            .parse()
            .ok()
            .filter(|&n| (VISITOR_NONE..game::GARDEN_SLOTS as i32).contains(&n))
            .unwrap_or(VISITOR_AUTO),
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

/// How often what we bring is looked at again (frames).
const BROUGHT_EVERY_FRAMES: u32 = 120;

/// What we bring to the shared gardens (`Body::Brought`): each garden's visitor rows (eggs
/// stay out: the host holds only hatched Chao), with their copy numbers; and whether our save
/// is loaded, so the host's changes to them reach it.
fn brought_now() -> (bool, Vec<BroughtChao>) {
    let awake = game::chao_in_memory();
    let slots: Vec<Option<ChaoData>> = (0..game::GARDEN_SLOTS)
        .map(|slot| game::read_chao_slot(slot).filter(|c| !c.is_empty()))
        .collect();
    let mut list = Vec::new();
    for garden in 1..=3u8 {
        for (row, pick) in carried_chao(i32::from(garden)).into_iter().enumerate() {
            let Some((slot, chao)) = pick.filter(|(_, c)| c.chao_type() != chao::TYPE_EGG) else {
                continue;
            };
            list.push(BroughtChao {
                garden,
                row: row as u8,
                nth: crate::held::nth_of(&slots, slot),
                data: Box::new(chao),
            });
        }
    }
    (awake, list)
}

/// Our visitors from `garden` (hatched Chao and eggs), by visitor row (save slot and Chao).
fn carried_chao(garden: i32) -> [Option<(usize, ChaoData)>; MAX_CARRIED_CHAO] {
    // Only Chao living in this garden can visit from it (their real ones are here to follow).
    // Auto picks My Chao first, then the others in save order.
    let mine = my_chosen_slot().map(|(slot, _)| slot);
    let mut here: Vec<(usize, ChaoData)> = garden_chao()
        .into_iter()
        .filter(|(_, c)| i32::from(c.garden()) == garden)
        .collect();
    here.sort_by_key(|(slot, _)| (Some(*slot) != mine, *slot));
    let picks = bring_rows(VISITORS.each_ref().map(|v| v.load(Ordering::Relaxed)));
    let slots: Vec<usize> = here.iter().map(|(slot, _)| *slot).collect();
    pick_visitors(&slots, picks)
        .map(|slot| slot.and_then(|slot| here.iter().find(|(s, _)| *s == slot).cloned()))
}

/// The visitor rows that are used: the first `BRING_CHAO` (all of them now); any others bring
/// nobody.
fn bring_rows(mut picks: [i32; MAX_CARRIED_CHAO]) -> [i32; MAX_CARRIED_CHAO] {
    for pick in picks.iter_mut().skip(BRING_CHAO) {
        *pick = VISITOR_NONE;
    }
    picks
}

/// The save slots of the Chao (and eggs) we bring to `garden`.
pub fn brought_slots(garden: i32) -> Vec<usize> {
    carried_chao(garden)
        .into_iter()
        .flatten()
        .map(|(slot, _)| slot)
        .collect()
}

/// What the visitor rows come to in the garden we're in (None outside a garden).
pub fn visitor_preview() -> Option<[Option<(usize, ChaoData)>; MAX_CARRIED_CHAO]> {
    game::in_garden().then(|| carried_chao(game::current_chao_area()))
}

/// The visitor rows over `here` (save slots in this garden, in Auto order): a chosen Chao
/// that lives here keeps its row; Auto rows take the next ones nobody chose. No Chao twice.
fn pick_visitors(
    here: &[usize],
    picks: [i32; MAX_CARRIED_CHAO],
) -> [Option<usize>; MAX_CARRIED_CHAO] {
    let mut out = [None; MAX_CARRIED_CHAO];
    for (row, &pick) in picks.iter().enumerate() {
        out[row] = here
            .iter()
            .copied()
            .find(|&s| s as i32 == pick)
            .filter(|s| !out.contains(&Some(*s)));
    }
    let free: Vec<usize> = here
        .iter()
        .copied()
        .filter(|&s| !out.contains(&Some(s)) && !picks.contains(&(s as i32)))
        .collect();
    let mut free = free.into_iter();
    for (row, &pick) in picks.iter().enumerate() {
        if pick == VISITOR_AUTO {
            out[row] = free.next();
        }
    }
    out
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
    fn a_paused_owners_snapshot_can_spawn_without_waiting_for_another_packet() {
        let now = Instant::now();
        let state = GardenChao {
            index: 0,
            position: [10.0, 2.0, 30.0],
            ..Default::default()
        };
        let snapshot = GardenSnapshot::new(1, &[state], now);
        assert!(snapshot.available(1, false, now + Duration::from_secs(44)));
        assert_eq!(snapshot.states[0].unwrap().position, state.position);
        assert!(!snapshot.available(2, false, now + Duration::from_secs(44)));
        assert!(!snapshot.available(1, false, now + Duration::from_secs(301)));
        assert!(!snapshot.available(1, true, now + Duration::from_secs(9)));
    }

    #[test]
    fn withdrawing_visitors_replaces_cached_presence_and_uses_the_existing_protocol() {
        let now = Instant::now();
        let snapshot = GardenSnapshot::new(1, &[], now);
        assert!(snapshot.available(1, true, now));
        assert!(snapshot.states.iter().all(Option::is_none));
        let message = crate::net::protocol::Message::Game {
            from: 1,
            body: Body::GardenChao {
                owner: 1,
                list: Vec::new(),
            },
        };
        assert_eq!(
            crate::net::protocol::Message::decode(&message.encode()).unwrap(),
            message
        );
    }

    #[test]
    fn visitors_follow_their_rows() {
        let (auto, none) = (VISITOR_AUTO, VISITOR_NONE);
        // Slots living in this garden, My Chao (3) first.
        let here = [3, 0, 1];
        assert_eq!(
            pick_visitors(&here, [auto, auto, none, none]),
            [Some(3), Some(0), None, None]
        );
        assert_eq!(
            pick_visitors(&here, [1, auto, none, none]),
            [Some(1), Some(3), None, None]
        );
        assert_eq!(
            pick_visitors(&here, [auto, 1, none, none]),
            [Some(3), Some(1), None, None]
        );
        assert_eq!(
            pick_visitors(&here, [0, none, none, none]),
            [Some(0), None, None, None]
        );
        assert_eq!(
            pick_visitors(&here, [none, none, none, none]),
            [None, None, None, None]
        );
        // A pick living in another garden doesn't come; it isn't replaced either.
        assert_eq!(
            pick_visitors(&here, [7, auto, none, none]),
            [None, Some(3), None, None]
        );
        // The same Chao twice comes once.
        assert_eq!(
            pick_visitors(&here, [0, 0, none, none]),
            [Some(0), None, None, None]
        );
        // One Chao: row 1 has it, row 2 is empty. None: both empty.
        assert_eq!(
            pick_visitors(&[5], [auto, auto, none, none]),
            [Some(5), None, None, None]
        );
        assert_eq!(
            pick_visitors(&[], [auto, auto, none, none]),
            [None, None, None, None]
        );
        // Four rows, all Auto: the first four here.
        assert_eq!(
            pick_visitors(&[3, 0, 1, 6, 2], [auto; 4]),
            [Some(3), Some(0), Some(1), Some(6)]
        );
        assert_eq!(tidy_visitors([2, 2, 5, 2]), [2, auto, 5, auto]);
        // Four are brought: every row counts; Auto takes the first four nobody chose.
        assert_eq!(BRING_CHAO, 4);
        assert_eq!(
            pick_visitors(&[3, 0, 1, 6, 2], bring_rows([auto; 4])),
            [Some(3), Some(0), Some(1), Some(6)]
        );
        assert_eq!(
            pick_visitors(&[3, 0, 1, 6, 2], bring_rows([6, auto, 1, auto])),
            [Some(6), Some(3), Some(1), Some(0)]
        );
        assert_eq!(
            pick_visitors(&[3, 0, 1, 6, 2], bring_rows([auto, none, 2, auto])),
            [Some(3), None, Some(2), Some(0)]
        );
        assert_eq!(bring_rows([1, none, 4, auto]), [1, none, 4, auto]);
        assert_eq!(parse_visitor("None"), none);
        assert_eq!(parse_visitor(" 4 "), 4);
        assert_eq!(parse_visitor("99"), auto);
    }

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
                lock: ChaoLock::Always,
                visitors: [VISITOR_AUTO; MAX_CARRIED_CHAO],
                room_mode: crate::room::Mode::Race,
                key: None,
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
                lock: ChaoLock::Always,
                visitors: [VISITOR_AUTO; MAX_CARRIED_CHAO],
                room_mode: crate::room::Mode::Race,
                key: None,
            }
        );
        // Our key, as made once (a bad one is as none: a new one is made).
        let c = parse_config("PlayerKey = 000102030405060708090a0b0c0d0e0f").unwrap();
        assert_eq!(c.key, Some(std::array::from_fn(|i| i as u8)));
        assert_eq!(parse_config("PlayerKey = 12zz").unwrap().key, None);
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
    fn old_visit_key_is_ignored_and_the_lock_is_read() {
        let c = parse_config("VisitWith=Off\nVisitor3=5\nOthersChangeChao=ThatGarden\n").unwrap();
        assert_eq!(c.lock, ChaoLock::ThatGarden);
        assert_eq!(c.visitors, [VISITOR_AUTO, VISITOR_AUTO, 5, VISITOR_AUTO]);
        assert_eq!(
            parse_config("OthersChangeChao = never").unwrap().lock,
            ChaoLock::Never
        );
        assert_eq!(parse_config("").unwrap().lock, ChaoLock::Always);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse_config("mode = server").is_err());
        assert!(parse_config("mode = host\nport = 99999").is_err());
        assert!(parse_config("mode = client\nhost = nowhere").is_err());
        assert!(parse_config("name = abcdefghijklmnopqrstuvwxyz").is_err());
    }
}

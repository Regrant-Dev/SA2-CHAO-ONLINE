//! Lobby session over the UDP transport: star topology, host relays.
//!
//! The host is player 0 and assigns ids 1..=7 to clients. Clients only talk to
//! the host; the host forwards `Game` messages to everyone else, overwriting
//! `from` with the sender's real id so nobody can speak for another player.
//!
//! A dedicated host (`Host::dedicated`, the `sa2co-server` program) only relays: it isn't a
//! player, isn't in the player list, and gives clients ids 0..=7.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::protocol::{Body, GameBuild, MAX_PLAYERS, Message, PROTOCOL_VERSION};
use super::udp::{Endpoint, Received};

pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(1);
pub const TIMEOUT: Duration = Duration::from_secs(6);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct Identity {
    pub name: String,
    pub mod_version: String,
    pub game_build: GameBuild,
}

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Client only: the host accepted us.
    Connected {
        my_id: u8,
    },
    /// Client only: connection ended (rejected, host gone, timeout).
    Disconnected {
        reason: String,
    },
    Joined {
        id: u8,
        name: String,
    },
    Left {
        id: u8,
    },
    Game {
        from: u8,
        body: Body,
    },
}

pub enum Session {
    Host(Host),
    Client(Client),
}

impl Session {
    pub fn my_id(&self) -> Option<u8> {
        match self {
            Self::Host(_) => Some(0),
            Self::Client(c) => c.my_id,
        }
    }

    pub fn poll(&mut self, now: Instant) -> Vec<Event> {
        match self {
            Self::Host(h) => h.poll(now),
            Self::Client(c) => c.poll(now),
        }
    }

    /// Sends gameplay data to every other player.
    pub fn send(&mut self, body: Body, reliable: bool, now: Instant) {
        match self {
            Self::Host(h) => h.broadcast(&Message::Game { from: 0, body }, reliable, None, now),
            Self::Client(c) => c.send(body, reliable, now),
        }
    }

    pub fn players(&self) -> Vec<(u8, String)> {
        match self {
            Self::Host(h) => h.players(),
            Self::Client(c) => c.players.iter().map(|(id, n)| (*id, n.clone())).collect(),
        }
    }

    pub fn shutdown(&mut self, now: Instant) {
        match self {
            Self::Host(h) => {
                h.broadcast(&Message::Bye, false, None, now);
            }
            Self::Client(c) => c.ep.send_unreliable(c.host, &Message::Bye.encode()),
        }
    }
}

struct Remote {
    addr: SocketAddr,
    name: String,
    last_heard: Instant,
}

pub struct Host {
    ep: Endpoint,
    me: Identity,
    /// Not a player itself: only relays (the dedicated server).
    dedicated: bool,
    clients: BTreeMap<u8, Remote>,
    by_addr: HashMap<SocketAddr, u8>,
    last_heartbeat: Instant,
}

impl Host {
    pub fn new(ep: Endpoint, me: Identity, now: Instant) -> Self {
        Self {
            ep,
            me,
            dedicated: false,
            clients: BTreeMap::new(),
            by_addr: HashMap::new(),
            last_heartbeat: now,
        }
    }

    /// A host that isn't a player: it only relays, and all 8 ids go to clients.
    pub fn dedicated(ep: Endpoint, me: Identity, now: Instant) -> Self {
        Self {
            dedicated: true,
            ..Self::new(ep, me, now)
        }
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.ep.local_addr()
    }

    /// Everyone in the lobby: (id, name). A dedicated host isn't in it.
    pub fn players(&self) -> Vec<(u8, String)> {
        let mut out = Vec::new();
        if !self.dedicated {
            out.push((0, self.me.name.clone()));
        }
        out.extend(self.clients.iter().map(|(id, r)| (*id, r.name.clone())));
        out
    }

    /// Polls the network: handles joins, relays game messages, drops silent clients.
    pub fn poll_events(&mut self, now: Instant) -> Vec<Event> {
        self.poll(now)
    }

    /// Tells every client we're going away.
    pub fn shutdown(&mut self, now: Instant) {
        self.broadcast(&Message::Bye, false, None, now);
    }

    fn broadcast(&mut self, msg: &Message, reliable: bool, except: Option<u8>, now: Instant) {
        let data = msg.encode();
        let addrs: Vec<SocketAddr> = self
            .clients
            .iter()
            .filter(|(id, _)| Some(**id) != except)
            .map(|(_, r)| r.addr)
            .collect();
        for addr in addrs {
            if reliable {
                self.ep.send_reliable(addr, &data, now);
            } else {
                self.ep.send_unreliable(addr, &data);
            }
        }
    }

    fn reject(&mut self, addr: SocketAddr, reason: &str) {
        // Unreliable so we can drop the link right away; the client times out if it's lost.
        self.ep.send_unreliable(
            addr,
            &Message::Reject {
                reason: reason.into(),
            }
            .encode(),
        );
        self.ep.remove(addr);
    }

    fn drop_client(&mut self, id: u8, now: Instant, events: &mut Vec<Event>) {
        if let Some(r) = self.clients.remove(&id) {
            self.by_addr.remove(&r.addr);
            self.ep.remove(r.addr);
            self.broadcast(&Message::PeerLeft { id }, true, None, now);
            events.push(Event::Left { id });
        }
    }

    fn poll(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        for (addr, received) in self.ep.poll(now) {
            let (data, reliable) = match received {
                Received::Message { data, reliable } => (data, reliable),
                Received::LinkFailed => {
                    if let Some(&id) = self.by_addr.get(&addr) {
                        self.drop_client(id, now, &mut events);
                    }
                    continue;
                }
            };
            let Ok(msg) = Message::decode(&data) else {
                continue;
            };
            match self.by_addr.get(&addr).copied() {
                None => {
                    if let Message::Hello {
                        protocol,
                        mod_version,
                        game_build,
                        name,
                    } = msg
                    {
                        self.handle_hello(
                            addr,
                            protocol,
                            &mod_version,
                            game_build,
                            name,
                            now,
                            &mut events,
                        );
                    } else {
                        self.ep.remove(addr);
                    }
                }
                Some(id) => {
                    if let Some(r) = self.clients.get_mut(&id) {
                        r.last_heard = now;
                    }
                    match msg {
                        Message::Game { body, .. } => {
                            let relayed = Message::Game {
                                from: id,
                                body: body.clone(),
                            };
                            self.broadcast(&relayed, reliable, Some(id), now);
                            events.push(Event::Game { from: id, body });
                        }
                        Message::Bye => self.drop_client(id, now, &mut events),
                        // Duplicate Hello from a client whose Welcome was slow: ignore.
                        _ => {}
                    }
                }
            }
        }

        if now.duration_since(self.last_heartbeat) >= HEARTBEAT_EVERY {
            self.last_heartbeat = now;
            self.broadcast(&Message::Heartbeat, false, None, now);
        }
        let stale: Vec<u8> = self
            .clients
            .iter()
            .filter(|(_, r)| now.duration_since(r.last_heard) > TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.drop_client(id, now, &mut events);
        }
        events
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_hello(
        &mut self,
        addr: SocketAddr,
        protocol: u16,
        mod_version: &str,
        game_build: GameBuild,
        name: String,
        now: Instant,
        events: &mut Vec<Event>,
    ) {
        if protocol != PROTOCOL_VERSION {
            return self.reject(
                addr,
                &format!("protocol {protocol}, host has {PROTOCOL_VERSION}"),
            );
        }
        if mod_version != self.me.mod_version {
            return self.reject(
                addr,
                &format!("mod {mod_version}, host has {}", self.me.mod_version),
            );
        }
        if game_build != self.me.game_build {
            return self.reject(addr, "different game version");
        }
        let first = if self.dedicated { 0 } else { 1 };
        let Some(id) = (first..MAX_PLAYERS as u8).find(|id| !self.clients.contains_key(id)) else {
            return self.reject(addr, "lobby full");
        };

        let mut players = self.players();
        players.push((id, name.clone()));
        self.ep.send_reliable(
            addr,
            &Message::Welcome {
                your_id: id,
                players,
            }
            .encode(),
            now,
        );
        self.broadcast(
            &Message::PeerJoined {
                id,
                name: name.clone(),
            },
            true,
            None,
            now,
        );
        self.clients.insert(
            id,
            Remote {
                addr,
                name: name.clone(),
                last_heard: now,
            },
        );
        self.by_addr.insert(addr, id);
        events.push(Event::Joined { id, name });
    }
}

pub struct Client {
    ep: Endpoint,
    host: SocketAddr,
    my_id: Option<u8>,
    players: BTreeMap<u8, String>,
    started: Instant,
    last_heard: Instant,
    last_heartbeat: Instant,
    closed: bool,
}

impl Client {
    pub fn connect(mut ep: Endpoint, host: SocketAddr, me: &Identity, now: Instant) -> Self {
        let hello = Message::Hello {
            protocol: PROTOCOL_VERSION,
            mod_version: me.mod_version.clone(),
            game_build: me.game_build,
            name: me.name.clone(),
        };
        ep.send_reliable(host, &hello.encode(), now);
        Self {
            ep,
            host,
            my_id: None,
            players: BTreeMap::new(),
            started: now,
            last_heard: now,
            last_heartbeat: now,
            closed: false,
        }
    }

    fn send(&mut self, body: Body, reliable: bool, now: Instant) {
        let Some(from) = self.my_id else { return };
        let data = Message::Game { from, body }.encode();
        if reliable {
            self.ep.send_reliable(self.host, &data, now);
        } else {
            self.ep.send_unreliable(self.host, &data);
        }
    }

    fn close(&mut self, reason: String, events: &mut Vec<Event>) {
        if !self.closed {
            self.closed = true;
            self.my_id = None;
            self.players.clear();
            self.ep.remove(self.host);
            events.push(Event::Disconnected { reason });
        }
    }

    fn poll(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        if self.closed {
            return events;
        }
        for (addr, received) in self.ep.poll(now) {
            if addr != self.host {
                self.ep.remove(addr);
                continue;
            }
            let data = match received {
                Received::Message { data, .. } => data,
                Received::LinkFailed => {
                    self.close("connection to host lost".into(), &mut events);
                    return events;
                }
            };
            let Ok(msg) = Message::decode(&data) else {
                continue;
            };
            self.last_heard = now;
            match msg {
                Message::Welcome { your_id, players } if self.my_id.is_none() => {
                    self.my_id = Some(your_id);
                    events.push(Event::Connected { my_id: your_id });
                    for (id, name) in players {
                        if id != your_id {
                            events.push(Event::Joined {
                                id,
                                name: name.clone(),
                            });
                        }
                        self.players.insert(id, name);
                    }
                }
                Message::Reject { reason } => {
                    self.close(format!("rejected: {reason}"), &mut events);
                    return events;
                }
                Message::PeerJoined { id, name }
                    if Some(id) != self.my_id && self.my_id.is_some() =>
                {
                    if self.players.insert(id, name.clone()).is_none() {
                        events.push(Event::Joined { id, name });
                    }
                }
                Message::PeerLeft { id } => {
                    if self.players.remove(&id).is_some() {
                        events.push(Event::Left { id });
                    }
                }
                Message::Game { from, body }
                    if self.my_id.is_some() && Some(from) != self.my_id =>
                {
                    events.push(Event::Game { from, body });
                }
                Message::Bye => {
                    self.close("host closed the lobby".into(), &mut events);
                    return events;
                }
                _ => {}
            }
        }

        if self.my_id.is_none() && now.duration_since(self.started) > CONNECT_TIMEOUT {
            self.close("no answer from host".into(), &mut events);
        } else if now.duration_since(self.last_heard) > TIMEOUT {
            self.close("host timed out".into(), &mut events);
        } else if self.my_id.is_some() && now.duration_since(self.last_heartbeat) >= HEARTBEAT_EVERY
        {
            self.last_heartbeat = now;
            self.ep
                .send_unreliable(self.host, &Message::Heartbeat.encode());
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::PlayerState;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn local() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
    }

    fn identity(name: &str) -> Identity {
        Identity {
            name: name.into(),
            mod_version: "0.0.1".into(),
            game_build: [9; 8],
        }
    }

    struct Lobby {
        now: Instant,
        host: Session,
        clients: Vec<Session>,
        host_events: Vec<Event>,
        client_events: Vec<Vec<Event>>,
    }

    impl Lobby {
        fn new() -> Self {
            let now = Instant::now();
            let host = Host::new(Endpoint::bind(local()).unwrap(), identity("host"), now);
            Self {
                now,
                host: Session::Host(host),
                clients: vec![],
                host_events: vec![],
                client_events: vec![],
            }
        }

        /// A lobby on a dedicated host (not a player).
        fn dedicated() -> Self {
            let now = Instant::now();
            let host = Host::dedicated(Endpoint::bind(local()).unwrap(), identity("server"), now);
            Self {
                host: Session::Host(host),
                ..Self::new()
            }
        }

        fn host_addr(&self) -> SocketAddr {
            match &self.host {
                Session::Host(h) => h.local_addr(),
                _ => unreachable!(),
            }
        }

        fn join(&mut self, id: &Identity) -> usize {
            let c = Client::connect(
                Endpoint::bind(local()).unwrap(),
                self.host_addr(),
                id,
                self.now,
            );
            self.clients.push(Session::Client(c));
            self.client_events.push(vec![]);
            self.clients.len() - 1
        }

        /// Runs the lobby for `ms` of fake time in 10 ms steps.
        fn run(&mut self, ms: u64) {
            for _ in 0..ms / 10 {
                self.now += Duration::from_millis(10);
                self.host_events.extend(self.host.poll(self.now));
                for (i, c) in self.clients.iter_mut().enumerate() {
                    self.client_events[i].extend(c.poll(self.now));
                }
                std::thread::sleep(Duration::from_micros(300));
            }
        }
    }

    fn state(level: i16) -> Body {
        Body::PlayerState(PlayerState {
            time: 0,
            level,
            mission: 0,
            chao_area: 0,
            position: [1.0, 2.0, 3.0],
            character: 0,
            rotation: [0; 3],
            anim: 0,
            frame: 0.0,
            flags: 0,
            room: (0xFF, 0),
            upgrades: 0,
        })
    }

    #[test]
    fn eight_players_join_and_see_each_other() {
        let mut lobby = Lobby::new();
        for i in 1..8 {
            lobby.join(&identity(&format!("p{i}")));
        }
        lobby.run(500);
        assert_eq!(lobby.host.players().len(), 8);
        for (i, c) in lobby.clients.iter().enumerate() {
            assert!(
                c.my_id().is_some(),
                "client {i} not connected: {:?}",
                lobby.client_events[i]
            );
            assert_eq!(c.players().len(), 8, "client {i} sees {:?}", c.players());
        }
        let mut ids: Vec<u8> = lobby.clients.iter().map(|c| c.my_id().unwrap()).collect();
        ids.sort();
        assert_eq!(ids, (1..8).collect::<Vec<u8>>());
    }

    #[test]
    fn dedicated_host_relays_for_eight_players_and_is_not_one() {
        let mut lobby = Lobby::dedicated();
        for i in 0..9 {
            lobby.join(&identity(&format!("p{i}")));
        }
        lobby.run(500);
        // Eight players get ids 0..=7; the ninth finds the lobby full.
        let mut ids: Vec<u8> = lobby.clients.iter().filter_map(|c| c.my_id()).collect();
        ids.sort();
        assert_eq!(ids, (0..8).collect::<Vec<u8>>());
        assert_eq!(lobby.host.players().len(), 8);
        // Nobody sees the server as a player.
        for c in lobby.clients.iter().filter(|c| c.my_id().is_some()) {
            assert_eq!(c.players().len(), 8);
            assert!(!c.players().iter().any(|(_, n)| n == "server"));
        }
        // Game messages still reach everyone else, from the real sender.
        let sender = lobby
            .clients
            .iter()
            .position(|c| c.my_id().is_some())
            .unwrap();
        let from = lobby.clients[sender].my_id().unwrap();
        let now = lobby.now;
        lobby.clients[sender].send(state(13), true, now);
        lobby.run(300);
        let got = lobby
            .client_events
            .iter()
            .enumerate()
            .filter(|(i, ev)| {
                *i != sender
                    && ev
                        .iter()
                        .any(|e| matches!(e, Event::Game { from: f, .. } if *f == from))
            })
            .count();
        assert_eq!(got, 7);
    }

    #[test]
    fn ninth_player_is_rejected() {
        let mut lobby = Lobby::new();
        for i in 1..9 {
            lobby.join(&identity(&format!("p{i}")));
        }
        lobby.run(500);
        let rejected = lobby
            .client_events
            .iter()
            .filter(|ev| ev.iter().any(|e| matches!(e, Event::Disconnected { reason } if reason.contains("lobby full"))))
            .count();
        assert_eq!(rejected, 1);
        assert_eq!(lobby.host.players().len(), 8);
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut lobby = Lobby::new();
        let mut old = identity("old");
        old.mod_version = "0.0.0".into();
        lobby.join(&old);
        let mut other_game = identity("other");
        other_game.game_build = [1; 8];
        lobby.join(&other_game);
        lobby.run(300);
        for ev in &lobby.client_events {
            assert!(ev.iter().any(|e| matches!(e, Event::Disconnected { reason } if reason.starts_with("rejected"))), "{ev:?}");
        }
        assert_eq!(lobby.host.players().len(), 1);
    }

    #[test]
    fn game_messages_are_relayed_with_true_sender() {
        let mut lobby = Lobby::new();
        let a = lobby.join(&identity("a"));
        let b = lobby.join(&identity("b"));
        lobby.run(300);
        let a_id = lobby.clients[a].my_id().unwrap();

        // Client a sends; host and b must see it from a_id.
        let now = lobby.now;
        lobby.clients[a].send(state(13), true, now);
        lobby.run(200);
        assert!(lobby.host_events.contains(&Event::Game {
            from: a_id,
            body: state(13)
        }));
        assert!(lobby.client_events[b].contains(&Event::Game {
            from: a_id,
            body: state(13)
        }));
        // a must not get its own message back.
        assert!(
            !lobby.client_events[a]
                .iter()
                .any(|e| matches!(e, Event::Game { .. }))
        );

        // Host sends; both clients see it from 0.
        lobby.host.send(state(90), false, now);
        lobby.run(200);
        for i in [a, b] {
            assert!(lobby.client_events[i].contains(&Event::Game {
                from: 0,
                body: state(90)
            }));
        }
    }

    #[test]
    fn leaving_and_timeouts_are_reported() {
        let mut lobby = Lobby::new();
        let a = lobby.join(&identity("a"));
        let b = lobby.join(&identity("b"));
        lobby.run(300);
        let a_id = lobby.clients[a].my_id().unwrap();
        let b_id = lobby.clients[b].my_id().unwrap();

        // a says goodbye.
        let now = lobby.now;
        lobby.clients[a].shutdown(now);
        lobby.run(300);
        assert!(lobby.host_events.contains(&Event::Left { id: a_id }));
        assert!(lobby.client_events[b].contains(&Event::Left { id: a_id }));

        // b goes silent (process frozen): stop polling it and let time pass.
        let silent = lobby.clients.remove(b);
        lobby.client_events.remove(b);
        lobby.clients.clear();
        lobby.client_events.clear();
        lobby.run((TIMEOUT + Duration::from_secs(1)).as_millis() as u64);
        assert!(lobby.host_events.contains(&Event::Left { id: b_id }));
        drop(silent);
    }

    #[test]
    fn client_notices_host_going_away() {
        let mut lobby = Lobby::new();
        let a = lobby.join(&identity("a"));
        lobby.run(300);
        let now = lobby.now;
        lobby.host.shutdown(now);
        lobby.run(100);
        assert!(
            lobby.client_events[a]
                .iter()
                .any(|e| matches!(e, Event::Disconnected { .. }))
        );
    }
}

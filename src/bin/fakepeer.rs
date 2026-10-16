//! Fake player for testing the lobby without a second PC.
//!
//!   fakepeer host [port] [name]            host a lobby (the game joins as a client)
//!   fakepeer client <ip:port> [name]       join a lobby hosted by the game
//!
//! It prints lobby events and sends a fake player at 30 Hz: once it has heard from a real
//! player it follows them (same stage, pose and animation, 40 units to the side, about half a
//! second behind), otherwise it walks in a circle in the Neutral Garden (level 90, area 1).
//!
//! Set FAKEPEER_CHAR to a character ID (0 Sonic, 4 Knuckles, ...) to appear as that
//! character instead of copying yours (tests drawing other characters).
//!
//! Set FAKEPEER_CHAO to a comma-separated list of .sa2chao files (e.g. F9 > Export my Chao) and it
//! brings those Chao (up to 2) to the shared garden whenever someone connects.
//!
//! Set FAKEPEER_ROOM_PICK=level,mission,character (e.g. 13,0,0 for City Escape mission 1 as
//! Sonic) and every 30 s it "picks" that stage, which takes you along if you're in its Stage
//! room (FAKEPEER_ROOM=1) and on Stage Select.
//!
//! Set FAKEPEER_RACE_PICK=category,course,level (e.g. 0,0,0 for the first Beginner race) and
//! every 30 s it "picks" that Chao Race, which takes you along if you're in its room
//! (FAKEPEER_ROOM=1) at the race entrance with your Chao chosen and the course list open.
//!
//! Rooms: it copies your state, so it is in whatever room you create. Set
//! FAKEPEER_ROOM=1 to have it create its own room instead (join it with
//! F9 > Room); the PICK/KARATE options below only reach you while you're in its room.
//!
//! Live karate: whenever you lead a live fight against it, it cheers its Chao once a second
//! and records your game's fight to
//! FAKEPEER_KARATE_FILE (default target/karate-fight.rec). Set FAKEPEER_KARATE=1 and,
//! once you have been in the karate area for 5 s (in its room), it starts a live
//! fight against you as the leader and replays that recording (a real fight), or, without one,
//! a pretend fight (the Chao circle, side 1 loses HP, side 0 wins by knockout after ~20 s).
//!
//! Build/run (not built by default): cargo run --release --target x86_64-unknown-linux-gnu --features fakepeer --bin fakepeer -- host

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sa2_chao_online::chao::file;
use sa2_chao_online::game::KNOWN_GAME_BUILD;
use sa2_chao_online::net::protocol::{
    Body, GardenChao, GardenItem, ItemId, ItemPlace, KarateFighter, KarateState, MAX_CARRIED_CHAO,
    Message, PlayerState,
};
use sa2_chao_online::net::session::{Client, Event, Host, Identity, Session};
use sa2_chao_online::net::udp::Endpoint;

/// Follow about half a second behind (states arrive at 30 Hz).
const FOLLOW_DELAY: usize = 15;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: fakepeer host [port] [name] | fakepeer client <ip:port> [name]";
    let now = Instant::now();
    let identity = |name: Option<&String>| Identity {
        name: name.cloned().unwrap_or_else(|| "fakepeer".into()),
        mod_version: env!("CARGO_PKG_VERSION").into(),
        game_build: KNOWN_GAME_BUILD,
    };

    let mut session = match args.first().map(String::as_str) {
        Some("host") => {
            let port: u16 = args.get(1).and_then(|p| p.parse().ok()).unwrap_or(43_902);
            let ep = Endpoint::bind(SocketAddr::from(([0, 0, 0, 0], port))).expect("bind port");
            println!("hosting on port {port}");
            Session::Host(Host::new(ep, identity(args.get(2)), now))
        }
        Some("client") => {
            let host: SocketAddr = args.get(1).and_then(|a| a.parse().ok()).expect(usage);
            let ep = Endpoint::bind(SocketAddr::from(([0, 0, 0, 0], 0))).expect("bind");
            println!("connecting to {host}");
            Session::Client(Client::connect(ep, host, &identity(args.get(2)), now))
        }
        _ => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };

    let chao: Vec<_> = std::env::var("FAKEPEER_CHAO")
        .unwrap_or_default()
        .split(',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| match std::fs::read(p).map(|d| file::decode(&d)) {
            Ok(Ok(c)) => Some(c),
            other => {
                eprintln!("skipping {p}: {other:?}");
                None
            }
        })
        .take(MAX_CARRIED_CHAO)
        .collect();
    println!("bringing {} chao", chao.len());
    let mut send_chao = false;
    let character: Option<u8> = std::env::var("FAKEPEER_CHAR")
        .ok()
        .and_then(|c| c.parse().ok());

    let room_pick: Option<Body> = std::env::var("FAKEPEER_ROOM_PICK").ok().and_then(|v| {
        let n: Vec<i64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        match n[..] {
            [level, mission, character] => Some(Body::RoomPick {
                level: level as i16,
                mission: mission as u8,
                character: character as u8,
            }),
            _ => {
                eprintln!("FAKEPEER_ROOM_PICK must be level,mission,character");
                None
            }
        }
    });
    let race_pick: Option<Body> = std::env::var("FAKEPEER_RACE_PICK").ok().and_then(|v| {
        let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        match n[..] {
            [category, course, level] => Some(Body::RacePick {
                category,
                course,
                level,
            }),
            _ => {
                eprintln!("FAKEPEER_RACE_PICK must be category,course,level");
                None
            }
        }
    });
    let mut last_pick = Instant::now();
    let mut karate = std::env::var("FAKEPEER_KARATE").is_ok_and(|v| v == "1");
    // A room is (creator id, 1) in PlayerState (room.rs).
    let own_room: Option<u8> = std::env::var("FAKEPEER_ROOM")
        .is_ok_and(|v| v == "1")
        .then_some(1);
    // (player id, since when they're in the karate area), and the fight start.
    let mut karate_seen: Option<(u8, Instant)> = None;
    let mut karate_fight: Option<Instant> = None;
    let mut karate_last = Instant::now();
    let karate_file =
        std::env::var("FAKEPEER_KARATE_FILE").unwrap_or_else(|_| "target/karate-fight.rec".into());
    // A recorded fight (ms since its start, message) to replay, and the one being recorded.
    let replay = load_fight(&karate_file);
    if karate {
        match &replay {
            Some(r) => println!("karate: will replay {} recorded updates", r.len()),
            None => println!("karate: no recording, will stream a pretend fight"),
        }
    }
    let mut replay_at = 0usize;
    let mut recording: Vec<(u32, Body)> = Vec::new();
    let mut recording_start = Instant::now();
    let mut last_cheer = Instant::now();

    let start = Instant::now();
    let mut last_state = start;
    // The most recent poses from a real player, oldest first, for a delayed follow.
    let mut heard: std::collections::VecDeque<PlayerState> = Default::default();
    loop {
        let now = Instant::now();
        for event in session.poll(now) {
            match event {
                Event::Game {
                    from,
                    body: Body::PlayerState(s),
                } => {
                    // Too chatty to print every packet; print roughly once a second per player.
                    if now.duration_since(start).as_millis() % 1000 < 16 {
                        println!(
                            "player {from}: level {} area {} char {} pos {:?} anim {} frame {:.1}",
                            s.level, s.chao_area, s.character, s.position, s.anim, s.frame
                        );
                    }
                    // Pass each state on as it arrives, FOLLOW_DELAY states later, so the
                    // spacing is exactly the real player's (smooth playback depends on it).
                    if karate && s.level == 90 && s.chao_area == 10 {
                        karate_seen.get_or_insert((from, now));
                    } else if karate_fight.is_none() {
                        karate_seen = None;
                    }
                    heard.push_back(s);
                    if heard.len() > FOLLOW_DELAY
                        && let Some(target) = heard.pop_front()
                    {
                        let room = match (own_room, session.my_id()) {
                            (Some(kind), Some(me)) => (me, kind),
                            _ => target.room,
                        };
                        let state = PlayerState {
                            room,
                            character: character.unwrap_or(target.character),
                            // A racing Chao (flag 16) just trails half a second behind; a
                            // character walks 40 units to the side.
                            position: [
                                target.position[0]
                                    + if target.flags & 16 != 0 { 0.0 } else { 40.0 },
                                target.position[1],
                                target.position[2],
                            ],
                            ..target
                        };
                        session.send(Body::PlayerState(state), false, now);
                    }
                }
                Event::Game {
                    from,
                    body: Body::Chao { index, data },
                } => println!("player {from} brought chao {index}: {}", data.summary()),
                Event::Game {
                    from,
                    body: body @ (Body::RoomPick { .. } | Body::RacePick { .. }),
                } => println!("player {from} picked: {body:?}"),
                // Our visiting Chao copy the real player's live garden Chao (40 units to the
                // side like our character; held ones stay in our character's hands).
                Event::Game {
                    body: Body::GardenChao(list),
                    ..
                } if !chao.is_empty() => {
                    let list = list
                        .into_iter()
                        .filter(|c| usize::from(c.index) < chao.len())
                        .map(|c| GardenChao {
                            position: [c.position[0] + 40.0, c.position[1], c.position[2]],
                            ..c
                        })
                        .collect();
                    session.send(Body::GardenChao(list), false, now);
                }
                // Own a copy of each of the real player's garden items: same place (40 units
                // to the side on the ground), our own ids. Claim them from us (we host, so our
                // referee answers) to test hand-offs.
                Event::Game {
                    from,
                    body: Body::GardenItems { area, items },
                } => {
                    let me = session.my_id().unwrap_or(0);
                    let items: Vec<GardenItem> = items
                        .into_iter()
                        .filter(|i| i.id.origin == from)
                        .map(|i| GardenItem {
                            id: ItemId {
                                origin: me,
                                serial: i.id.serial | 0x8000,
                            },
                            position: if i.place == ItemPlace::Ground {
                                [i.position[0] + 40.0, i.position[1], i.position[2]]
                            } else {
                                i.position
                            },
                            ..i
                        })
                        .collect();
                    if !items.is_empty() {
                        session.send(Body::GardenItems { area, items }, false, now);
                    }
                }
                Event::Game {
                    body: body @ Body::ItemOwner { .. },
                    ..
                } => println!("referee: {body:?}"),
                Event::Game {
                    from,
                    body:
                        Body::StageKills {
                            level,
                            mission,
                            objects,
                        },
                } => println!("player {from}: level {level} mission {mission} kills {objects:?}"),
                // Record a live karate fight someone leads against us.
                Event::Game {
                    body: body @ Body::KarateState(_),
                    ..
                } => {
                    if recording.is_empty() {
                        recording_start = now;
                        println!("karate: recording the fight");
                    }
                    // Cheer our Chao about once a second, like a player pressing A.
                    if now.duration_since(last_cheer) >= Duration::from_secs(1) {
                        last_cheer = now;
                        session.send(Body::KarateCheer { count: 1 }, true, now);
                    }
                    let ms = now.duration_since(recording_start).as_millis() as u32;
                    recording.push((ms, body));
                }
                Event::Game {
                    body: body @ Body::KarateEnd { .. },
                    ..
                } => {
                    let ms = now.duration_since(recording_start).as_millis() as u32;
                    recording.push((ms, body));
                    match save_fight(&karate_file, &recording) {
                        Ok(()) => {
                            println!("karate: saved {} updates to {karate_file}", recording.len())
                        }
                        Err(e) => println!("karate: can't save {karate_file}: {e}"),
                    }
                    recording.clear();
                }
                Event::Connected { .. } | Event::Joined { .. } => {
                    println!("{event:?}");
                    send_chao = true;
                }
                Event::Disconnected { reason } => {
                    println!("disconnected: {reason}");
                    return;
                }
                other => println!("{other:?}"),
            }
        }
        if send_chao && session.my_id().is_some() {
            send_chao = false;
            for (index, c) in chao.iter().enumerate() {
                let body = Body::Chao {
                    index: index as u8,
                    data: Box::new(c.clone()),
                };
                session.send(body, true, now);
            }
        }
        if session.my_id().is_some() && now.duration_since(last_pick) >= Duration::from_secs(30) {
            last_pick = now;
            for pick in room_pick.iter().chain(&race_pick) {
                println!("room: picking {pick:?}");
                session.send(pick.clone(), true, now);
            }
        }
        // Pretend live karate: pick the player, then stream a fight.
        if let Some((player, since)) = karate_seen {
            if karate_fight.is_none() && now.duration_since(since) > Duration::from_secs(5) {
                println!("karate: starting a live fight against player {player}");
                session.send(Body::KaratePick { opponent: player }, true, now);
                karate_fight = Some(now);
            }
            if let (Some(start), Some(replay)) = (karate_fight, &replay) {
                // Give the player's game ~8 s to load and hold, then replay the real fight.
                let ms = now.duration_since(start).as_millis() as i64 - 8000;
                while replay_at < replay.len() && i64::from(replay[replay_at].0) <= ms {
                    let body = replay[replay_at].1.clone();
                    let reliable = matches!(body, Body::KarateEnd { .. });
                    session.send(body, reliable, now);
                    replay_at += 1;
                }
                if replay_at == replay.len() {
                    println!("karate: replay finished");
                    karate = false;
                    karate_seen = None;
                    karate_fight = None;
                }
            } else if let Some(start) = karate_fight
                && now.duration_since(karate_last) >= Duration::from_millis(33)
            {
                karate_last = now;
                // Give the player's game ~8 s to load and hold, then 20 s of fight.
                let t = now.duration_since(start).as_secs_f32() - 8.0;
                if (0.0..20.0).contains(&t) {
                    let angle = t * 0.8;
                    let fighter = |side: f32, hp: f32| KarateFighter {
                        position: [
                            side * 4.0 * angle.cos(),
                            8.0,
                            19.0 + side * 4.0 * angle.sin(),
                        ],
                        rotation: [0, if side < 0.0 { 0x4000 } else { 0xC000 }, 0],
                        hp,
                        morale: 1000.0,
                        motion: 0,
                    };
                    let state = KarateState {
                        clock: (90.0 - t) as u16,
                        fighters: [
                            fighter(-1.0, 1500.0),
                            fighter(1.0, 1500.0 * (1.0 - t / 20.0)),
                        ],
                    };
                    session.send(Body::KarateState(state), false, now);
                } else if t >= 20.0 {
                    println!("karate: fight over, side 0 wins");
                    session.send(
                        Body::KarateEnd {
                            code: 2,
                            result: 0,
                            hp: [1500.0, 0.0],
                        },
                        true,
                        now,
                    );
                    // One pretend fight per run.
                    karate = false;
                    karate_seen = None;
                    karate_fight = None;
                }
            }
        }

        // Nobody to follow yet: walk in a circle in the Neutral Garden at 30 Hz.
        if heard.is_empty()
            && session.my_id().is_some()
            && now.duration_since(last_state) >= Duration::from_millis(33)
        {
            last_state = now;
            let t = now.duration_since(start).as_secs_f32();
            let state = PlayerState {
                time: (t * 60.0) as u32,
                level: 90,
                mission: 0,
                chao_area: 1,
                character: 0,
                position: [t.cos() * 50.0, 0.0, t.sin() * 50.0],
                rotation: [0, (t * 10_000.0) as i32, 0],
                // Animation 0 with a looping frame counter.
                anim: 0,
                frame: (t * 60.0) % 100.0,
                flags: 0,
                room: match (own_room, session.my_id()) {
                    (Some(kind), Some(me)) => (me, kind),
                    _ => (0xFF, 0),
                },
                upgrades: 0,
            };
            session.send(Body::PlayerState(state), false, now);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A recorded fight: per update, u32 ms since the start, u16 length, an encoded message.
fn save_fight(path: &str, fight: &[(u32, Body)]) -> std::io::Result<()> {
    let mut out = Vec::new();
    for (ms, body) in fight {
        let data = Message::Game {
            from: 0,
            body: body.clone(),
        }
        .encode();
        out.extend_from_slice(&ms.to_le_bytes());
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.extend_from_slice(&data);
    }
    std::fs::write(path, out)
}

fn load_fight(path: &str) -> Option<Vec<(u32, Body)>> {
    let data = std::fs::read(path).ok()?;
    let mut fight = Vec::new();
    let mut at = 0;
    while at + 6 <= data.len() {
        let ms = u32::from_le_bytes(data[at..at + 4].try_into().ok()?);
        let len = usize::from(u16::from_le_bytes(data[at + 4..at + 6].try_into().ok()?));
        let message = Message::decode(data.get(at + 6..at + 6 + len)?).ok()?;
        if let Message::Game { body, .. } = message {
            fight.push((ms, body));
        }
        at += 6 + len;
    }
    (!fight.is_empty()).then_some(fight)
}

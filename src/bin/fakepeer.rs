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
//! brings those Chao (up to 4) to the shared garden whenever someone connects. It also tells the
//! host it brings them (`Brought`, each to its own garden), so once it quits (Ctrl+C) they stay
//! there asleep, and while it's away from their garden they're awake (it prints the host's
//! `SaveChao` for them). Its player key is FAKEPEER_KEY (32 hex digits), or one made from its
//! name: run it again with the same one and the host knows its Chao. FAKEPEER_LOCK (Always,
//! ChaoWorld, ThatGarden, Never; default Always) is its "Others can change my Chao".
//!
//! Set FAKEPEER_ITEMS=1 to have it own a copy of each of your garden items (tests item hand-offs).
//! Picking one up then puts a real extra item in your save, so it's off by default.
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
//! Live Chao battles: set FAKEPEER_BATTLE=accept and it accepts your challenges (battle menu >
//! your team > its name, "live") with its FAKEPEER_CHAO Chao (Normal, basic moves), choosing
//! with the battle AI; FAKEPEER_BATTLE=singles or doubles also challenges you once you have been
//! in the karate area for 5 s (again a minute after each battle). It needs the same GBA ROM as
//! your game: FAKEPEER_ROM, default ~/Games/gba/emerald.gba. Multi battles: with `accept` it
//! also takes your multi battle invites (up to 3 of its Chao) and plays its place;
//! FAKEPEER_BATTLE=multi invites you as its ally against two computer places (copies of its
//! Chao), `multi-foe` invites you as a foe (its ally and yours the computer), at the same times
//! as a challenge. A place nobody takes it gives to the computer. Several fakepeers (one
//! hosting, the others joining it) play several places.
//!
//! Shared wild visitors: set FAKEPEER_WILD to a comma-separated list of modes and it sends
//! `WildHello` with the visitor print of FAKEPEER_ROM (seeing, picking and with the machine), so
//! it shares the garden's visitors with you (net/wild_host.rs, docs/testing.md):
//! - `pick`: answers the host's asks from its ROM (it is the picker while it was in the garden
//!   first: alone it walks in the Neutral Garden); FAKEPEER_WILD_LEAVE=<secs> sends it to the
//!   lobby for good after that long, so your game takes over picking.
//! - `take`: every 30 s picks up the visitor nearest it, holds it 10 s, then puts it down; with
//!   `carry` it goes to the lobby holding it instead (gone for you), and comes back 10 s later.
//! - `absorb`: hands the visitor it holds to your nearest Chao instead (your partners.txt
//!   changes only when your "Others can change my Chao" allows it).
//! - `whistle`: whistles for 2 s every 20 s.
//! - `battle-won`, `battle-lost`: every 40 s reserves the visitor nearest it, goes to the karate
//!   area (out of the garden) and 10 s later sends that result there and comes back.
//! - `hostile`: every 5 s sends visitor messages with unknown ids, picks without an ask,
//!   absorbs of visitors it doesn't hold and values out of range.
//!
//! Build/run (not built by default): cargo run --release --target x86_64-unknown-linux-gnu --features fakepeer --bin fakepeer -- host

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sa2_chao_online::battle::online::{self as live, Duel, Incoming};
use sa2_chao_online::battle::team::Dex;
use sa2_chao_online::chao::file;
use sa2_chao_online::game::KNOWN_GAME_BUILD;
use sa2_chao_online::gba::rom::Rom;
use sa2_chao_online::gba::world::{WildMap, WildMethod};
use sa2_chao_online::net::protocol::ChaoLock;
use sa2_chao_online::net::protocol::{
    ANSWER_ACCEPT, ANSWER_BUSY, Body, BroughtChao, GardenChao, GardenItem, ItemId, ItemPlace,
    KarateFighter, KarateState, LEAVE_LEFT, LEAVE_OVER, MAX_CARRIED_CHAO, MAX_CHAT, MAX_MULTI_TEAM,
    Message, MultiSeat, PlayerState, RACE_PROGRESS_UNKNOWN, SEAT_ACCEPTED, SERVER, WILD_MACHINE,
    WILD_PICK, WILD_SEE, WildArrive, WildCreature, WildEntry, WildPicked, WildPlace, WildRequest,
    WildStep,
};
use sa2_chao_online::net::roster::key_from_hex;
use sa2_chao_online::net::session::{Client, Event, Host, Identity, Session};
use sa2_chao_online::net::udp::Endpoint;

/// Follow about half a second behind (states arrive at 30 Hz).
const FOLLOW_DELAY: usize = 15;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: fakepeer host [port] [name] | fakepeer client <ip:port> [name]";
    let now = Instant::now();
    let identity = |name: Option<&String>| {
        let name = name.cloned().unwrap_or_else(|| "fakepeer".into());
        let key = std::env::var("FAKEPEER_KEY")
            .ok()
            .and_then(|k| key_from_hex(k.trim()))
            .unwrap_or_else(|| {
                let mut key = [0x5A; 16];
                for (k, b) in key.iter_mut().zip(name.bytes()) {
                    *k = b;
                }
                key
            });
        Identity {
            name,
            mod_version: env!("CARGO_PKG_VERSION").into(),
            game_build: KNOWN_GAME_BUILD,
            key,
        }
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
    let lock = ChaoLock::parse(&std::env::var("FAKEPEER_LOCK").unwrap_or_default());
    println!(
        "bringing {} chao; others may change them: {}",
        chao.len(),
        lock.label()
    );
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
    let copy_items = std::env::var("FAKEPEER_ITEMS").is_ok_and(|v| v == "1");
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

    // Live battles: the mode, our ROM data and team, the battle on, and a player to challenge.
    let battle_mode = std::env::var("FAKEPEER_BATTLE").unwrap_or_default();
    let battle_setup: Option<(Dex, Vec<_>)> = (!battle_mode.is_empty())
        .then(|| {
            let path = std::env::var("FAKEPEER_ROM").unwrap_or_else(|_| {
                format!(
                    "{}/Games/gba/emerald.gba",
                    std::env::var("HOME").unwrap_or_default()
                )
            });
            let rom = sa2_chao_online::gba::rom::Rom::open(std::path::Path::new(&path))
                .map_err(|e| eprintln!("battle: {path}: {e}"))
                .ok()?;
            let dex = Dex::from_rom(&rom)
                .map_err(|e| eprintln!("battle: {e}"))
                .ok()?;
            let team = live::plain_team(&rom, &chao);
            if team.is_empty() {
                eprintln!("battle: no team (FAKEPEER_CHAO)");
                return None;
            }
            println!("battle: {} with {} Chao", battle_mode, team.len());
            Some((dex, team))
        })
        .flatten();
    let challenge_format: Option<u8> = match battle_mode.as_str() {
        "singles" => Some(0),
        "doubles" => Some(1),
        _ => None,
    };
    // A multi battle to start: you as its ally (place 1) or as foe 1 (place 2).
    let multi_place: Option<usize> = match battle_mode.as_str() {
        "multi" => Some(1),
        "multi-foe" => Some(2),
        _ => None,
    };
    let mut duel: Option<Duel> = None;
    // The live battle's level rule (the challenge's: ours is Level 50).
    let mut actual_levels = false;
    let mut battle_seen: Option<(u8, Instant)> = None;
    let mut last_challenge: Option<Instant> = None;

    let mut wild = WildPeer::from_env();
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
                    // In the karate area and not busy (flag 32): one to challenge.
                    if s.level == 90 && s.chao_area == 10 && s.flags & 32 == 0 {
                        battle_seen.get_or_insert((from, now));
                    } else {
                        battle_seen = None;
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
                        let state = match wild.as_mut() {
                            Some(w) => w.state(state, from, now),
                            None => state,
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
                // side like our character; held ones stay in our character's hands). Only their
                // own: we never keep the garden, so their game does (and sends ours back to us).
                Event::Game {
                    from,
                    body: Body::GardenChao { owner, list },
                } if !chao.is_empty() && owner == from => {
                    let list = list
                        .into_iter()
                        .filter(|c| usize::from(c.index) < chao.len())
                        .map(|c| GardenChao {
                            position: [c.position[0] + 40.0, c.position[1], c.position[2]],
                            ..c
                        })
                        .collect();
                    let owner = session.my_id().unwrap_or(0);
                    session.send(Body::GardenChao { owner, list }, false, now);
                }
                // Own a copy of each of the real player's garden items: same place (40 units
                // to the side on the ground), our own ids. Claim them from us (we host, so our
                // referee answers) to test hand-offs.
                Event::Game {
                    from,
                    body: Body::GardenItems { area, items },
                } if copy_items => {
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
                            // Held by their Chao: by our copy of it.
                            place: match i.place {
                                ItemPlace::Chao { owner, index } if owner == from => {
                                    ItemPlace::Chao { owner: me, index }
                                }
                                place => place,
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
                // Race a little behind the real player (positions), and finish just after them.
                Event::Game {
                    body:
                        Body::RaceStatus {
                            level,
                            mission,
                            progress,
                            finish,
                        },
                    ..
                } => {
                    let progress = if progress == RACE_PROGRESS_UNKNOWN {
                        progress
                    } else {
                        progress.saturating_sub(300)
                    };
                    let finish = finish.map(|(race, game)| (race + 150, game + 150));
                    let body = Body::RaceStatus {
                        level,
                        mission,
                        progress,
                        finish,
                    };
                    session.send(body, finish.is_some(), now);
                }
                // Drive a kart 40 units to the side of the real player's.
                Event::Game {
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
                    ..
                } => {
                    let body = Body::KartState {
                        position: [position[0] + 40.0, position[1], position[2]],
                        rotation,
                        yaw,
                        wheel,
                        kind,
                        accel,
                        tilt,
                        frame,
                    };
                    session.send(body, false, now);
                }
                // Answer chat so it can be tested alone.
                Event::Game {
                    from,
                    body: Body::Chat { text },
                } => {
                    println!("chat from player {from}: {text}");
                    let reply = format!("heard: {text}");
                    let text: String = reply.chars().take(MAX_CHAT).collect();
                    session.send(Body::Chat { text }, true, now);
                }
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
                Event::Game {
                    from: SERVER,
                    body:
                        body @ (Body::BattleOffer { .. }
                        | Body::BattleStart { .. }
                        | Body::BattleStep { .. }
                        | Body::BattleEnd { .. }
                        | Body::MultiOffer { .. }
                        | Body::MultiStatus { .. }
                        | Body::MultiStart { .. }
                        | Body::MultiTakeover { .. }),
                } => match live::incoming(body, |id| format!("player {id}")) {
                    Some(Incoming::Offer {
                        from,
                        format,
                        actual_levels: levels,
                        multi,
                        ..
                    }) => {
                        let body = match &battle_setup {
                            Some((dex, team)) if duel.is_none() => {
                                let team = match &multi {
                                    Some(invite) => {
                                        println!(
                                            "battle: accepting player {from}'s multi battle \
                                             invite to place {}",
                                            invite.place
                                        );
                                        team.iter().take(MAX_MULTI_TEAM).cloned().collect()
                                    }
                                    None => {
                                        println!(
                                            "battle: accepting player {from}'s {format:?} challenge"
                                        );
                                        team.clone()
                                    }
                                };
                                actual_levels = levels;
                                Body::BattleAnswer {
                                    to: from,
                                    answer: ANSWER_ACCEPT,
                                    dex: live::dex_print(dex),
                                    team,
                                }
                            }
                            _ => Body::BattleAnswer {
                                to: from,
                                answer: ANSWER_BUSY,
                                dex: 0,
                                team: Vec::new(),
                            },
                        };
                        session.send(body, true, now);
                    }
                    Some(Incoming::Start(start)) => {
                        if let Some((dex, team)) = &battle_setup {
                            println!(
                                "battle: {} starts against player {} as side {}",
                                start.battle, start.opponent, start.side
                            );
                            duel = Duel::new(dex, &start, team, actual_levels);
                        }
                    }
                    Some(Incoming::MultiStart(start)) => {
                        if let Some((dex, _)) = &battle_setup {
                            let players = start.seats.each_ref().map(|s| s.player);
                            println!(
                                "battle: multi battle {} starts at place {} (players {players:?})",
                                start.battle, start.place
                            );
                            duel = Duel::multi(dex, &start);
                        } else {
                            let body = Body::BattleLeave {
                                battle: start.battle,
                                reason: LEAVE_LEFT,
                            };
                            session.send(body, true, now);
                        }
                    }
                    Some(Incoming::Status { place, status }) => {
                        println!("battle: multi battle place {place}: status {status}");
                        if status != SEAT_ACCEPTED
                            && let Some((_, team)) = &battle_setup
                        {
                            // Nobody took it: the computer plays it.
                            let team = team.iter().take(MAX_MULTI_TEAM).cloned().collect();
                            let body = Body::MultiFill {
                                place: place as u8,
                                team,
                            };
                            session.send(body, true, now);
                        }
                    }
                    Some(Incoming::Takeover { battle, place, why }) => {
                        println!(
                            "battle: multi battle {battle}: the computer plays place {place} \
                             (reason {why})"
                        );
                    }
                    Some(Incoming::Step {
                        battle,
                        step,
                        seed,
                        picks,
                    }) => {
                        if let Some(d) = duel.as_mut().filter(|d| d.id == battle) {
                            let events = d.step(step, seed, &picks);
                            println!("battle: step {step}, {} events", events.len());
                            if let Some(outcome) = d.over() {
                                println!("battle: over after {} turns: {outcome:?}", d.turn());
                                let body = Body::BattleLeave {
                                    battle,
                                    reason: LEAVE_OVER,
                                };
                                session.send(body, true, now);
                                duel = None;
                                last_challenge = Some(now);
                            }
                        }
                    }
                    Some(Incoming::End {
                        battle,
                        reason,
                        winner,
                    }) => {
                        println!("battle: {battle} ended (reason {reason}, winner {winner})");
                        if duel.as_ref().is_some_and(|d| d.id == battle) || battle == 0 {
                            duel = None;
                            last_challenge = Some(now);
                        }
                    }
                    _ => {}
                },
                Event::Game {
                    from: SERVER,
                    body:
                        body @ (Body::WildList { .. }
                        | Body::WildAsk { .. }
                        | Body::WildAnswer { .. }
                        | Body::WildTaught { .. }
                        | Body::WildUsed { .. }
                        | Body::ServerChao { .. }),
                } if wild.is_some() => {
                    if let Some(w) = wild.as_mut() {
                        for (body, reliable) in w.heard(&body, now) {
                            session.send(body, reliable, now);
                        }
                    }
                }
                Event::Connected { .. } | Event::Joined { .. } => {
                    println!("{event:?}");
                    send_chao = true;
                    if let Some(w) = wild.as_mut() {
                        w.said_hello = false;
                    }
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
            // Each in its own garden, in the rows it has here (copy numbers by file order).
            let list = chao
                .iter()
                .enumerate()
                .map(|(row, c)| BroughtChao {
                    garden: c.garden(),
                    row: row as u8,
                    nth: chao[..row]
                        .iter()
                        .filter(|o| o.identity() == c.identity())
                        .count() as u8,
                    data: Box::new(c.clone()),
                })
                .collect();
            session.send(
                Body::Brought {
                    awake: true,
                    lock,
                    list,
                },
                true,
                now,
            );
        }
        // Live battle: our choice for each step, and a challenge now and then.
        if let Some(body) = duel.as_mut().and_then(Duel::choice) {
            session.send(body, true, now);
        }
        if let (Some(format), Some((dex, team)), Some((player, since))) =
            (challenge_format, &battle_setup, battle_seen)
            && duel.is_none()
            && now.duration_since(since) > Duration::from_secs(5)
            && last_challenge.is_none_or(|at| now.duration_since(at) > Duration::from_secs(60))
        {
            println!("battle: challenging player {player}");
            actual_levels = false;
            let body = Body::BattleChallenge {
                to: player,
                format,
                save_partners: true,
                actual_levels,
                dex: live::dex_print(dex),
                team: team.clone(),
            };
            session.send(body, true, now);
            last_challenge = Some(now);
        }
        if let (Some(place), Some((dex, team)), Some((player, since))) =
            (multi_place, &battle_setup, battle_seen)
            && duel.is_none()
            && now.duration_since(since) > Duration::from_secs(5)
            && last_challenge.is_none_or(|at| now.duration_since(at) > Duration::from_secs(60))
        {
            println!("battle: inviting player {player} to a multi battle at place {place}");
            let ours: Vec<_> = team.iter().take(MAX_MULTI_TEAM).cloned().collect();
            let seats = [1, 2, 3].map(|p| {
                if p == place {
                    MultiSeat::Player(player)
                } else {
                    MultiSeat::Computer(ours.clone())
                }
            });
            let body = Body::MultiInvite {
                seats,
                save_partners: true,
                actual_levels: false,
                dex: live::dex_print(dex),
                team: ours,
            };
            session.send(body, true, now);
            last_challenge = Some(now);
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
                action: 0,
                status: 0,
            };
            let state = match wild.as_mut() {
                Some(w) => w.state(state, 0xFF, now),
                None => state,
            };
            session.send(Body::PlayerState(state), false, now);
        }
        if let Some(w) = wild.as_mut().filter(|_| session.my_id().is_some()) {
            for (body, reliable) in w.tick(now) {
                session.send(body, reliable, now);
            }
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

/// Shared wild visitors (FAKEPEER_WILD): our ROM, what the host last told us and what we're
/// doing with a visitor.
struct WildPeer {
    modes: Vec<String>,
    rom: Rom,
    maps: Vec<(u8, WildMap)>,
    print: u32,
    rng: u64,
    /// The garden of the last list we heard, and its visitors.
    area: u8,
    list: Vec<WildEntry>,
    /// Each Chao the server runs where we are: (owner, index) -> position.
    chao: std::collections::HashMap<(u8, u8), [f32; 3]>,
    /// The player we follow.
    player: Option<u8>,
    /// Where we last stood.
    at: [f32; 3],
    /// Walking to a visitor to take it, battle it or hand it over: what next, and when.
    task: Option<(Task, Instant)>,
    stand_at: Option<[f32; 3]>,
    holding: Option<u16>,
    /// In the lobby (carried one out) until then.
    away_until: Option<Instant>,
    leave_at: Option<Instant>,
    last_try: Instant,
    last_whistle: Instant,
    last_hostile: Instant,
    last_held: Instant,
    /// `WildHello` went to the host since we (re)connected.
    said_hello: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Task {
    Take(u16),
    Battle(u16),
    /// Holding `id`, put down (or carried out, or handed over) at that time.
    Hold(u16),
    Absorb(u16, (u8, u8)),
    Result(u16, WildStep),
}

impl WildPeer {
    fn from_env() -> Option<Self> {
        let modes: Vec<String> = std::env::var("FAKEPEER_WILD")
            .ok()?
            .split(',')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        let path = std::env::var("FAKEPEER_ROM").unwrap_or_else(|_| {
            format!(
                "{}/Games/gba/emerald.gba",
                std::env::var("HOME").unwrap_or_default()
            )
        });
        let rom = Rom::open(std::path::Path::new(&path))
            .map_err(|e| eprintln!("wild: {path}: {e}"))
            .ok()?;
        let print = sa2_chao_online::net::wild_print::visitor_print(&rom)?;
        let maps = rom
            .wild_maps()?
            .into_iter()
            .filter_map(|m| Some((rom.map_section(m.group, m.num)?, m)))
            .collect();
        let leave_at = std::env::var("FAKEPEER_WILD_LEAVE")
            .ok()
            .and_then(|s| s.parse().ok())
            .map(|secs| Instant::now() + Duration::from_secs(secs));
        println!("wild: {modes:?}, visitor print {print:08x}");
        let now = Instant::now();
        Some(Self {
            modes,
            rom,
            maps,
            print,
            rng: 0x9E37_79B9_7F4A_7C15 ^ u64::from(print),
            area: 0,
            list: Vec::new(),
            chao: Default::default(),
            player: None,
            at: [0.0; 3],
            task: None,
            stand_at: None,
            holding: None,
            away_until: None,
            leave_at,
            last_try: now,
            last_whistle: now,
            last_hostile: now,
            last_held: now,
            said_hello: false,
        })
    }

    fn mode(&self, m: &str) -> bool {
        self.modes.iter().any(|x| x == m)
    }

    fn hello(&self) -> Body {
        Body::WildHello {
            flags: WILD_SEE | WILD_PICK | WILD_MACHINE,
            print: self.print,
        }
    }

    fn below(&mut self, n: u64) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        if n == 0 { 0 } else { self.rng % n }
    }

    /// Our state, where what we do with a visitor puts us.
    fn state(&mut self, mut s: PlayerState, following: u8, now: Instant) -> PlayerState {
        if following != 0xFF {
            self.player = Some(following);
        }
        let away =
            self.away_until.is_some_and(|t| now < t) || self.leave_at.is_some_and(|t| now >= t);
        if away {
            // The Chao World lobby: out of every garden.
            s.chao_area = 5;
        } else if matches!(self.task, Some((Task::Result(..), _))) {
            // Battling a reserved visitor: in the karate arena, as a player's trip goes.
            s.chao_area = 10;
        } else if let Some(at) = self.stand_at {
            s.position = at;
        }
        if self.mode("whistle") && now.duration_since(self.last_whistle) < Duration::from_secs(2) {
            s.action = 42;
        }
        self.at = s.position;
        s
    }

    /// A creature from section `section`'s land or water tables (none: any area's).
    fn pick(&mut self, section: Option<u8>, pond: bool) -> Option<(u8, WildCreature)> {
        let wanted = |m: &WildMethod| match m {
            WildMethod::Land | WildMethod::Rocks => !pond,
            _ => pond,
        };
        let maps: Vec<(u8, WildMap)> = self
            .maps
            .iter()
            .filter(|(s, m)| {
                section.is_none_or(|x| x == *s) && m.tables.iter().any(|t| wanted(&t.method))
            })
            .cloned()
            .collect();
        let (section, map) = maps.get(self.below(maps.len() as u64) as usize)?.clone();
        let tables: Vec<_> = map.tables.iter().filter(|t| wanted(&t.method)).collect();
        let table = tables.get(self.below(tables.len() as u64) as usize)?;
        let total: u64 = table.slots.iter().map(|s| u64::from(s.chance)).sum();
        let mut roll = self.below(total);
        let slot = *table.slots.iter().find(|s| {
            let c = u64::from(s.chance);
            let hit = roll < c;
            roll = roll.saturating_sub(c);
            hit
        })?;
        let level =
            slot.min_level + self.below(u64::from(slot.max_level - slot.min_level) + 1) as u8;
        let species = self.rom.species(slot.species)?;
        let ability = match species.abilities[self.below(2) as usize] {
            0 => species.abilities[0],
            a => a,
        };
        let moves = self.rom.moves_at_level(slot.species, level);
        (moves != [0; 4]).then_some((
            section,
            WildCreature {
                species: slot.species,
                level,
                ability,
                shiny: self.below(1024) == 0,
                moves,
            },
        ))
    }

    /// What the host told us: our answers.
    fn heard(&mut self, body: &Body, now: Instant) -> Vec<(Body, bool)> {
        match body {
            Body::WildList { area, list, .. } => {
                self.area = *area;
                self.list = list.clone();
            }
            Body::ServerChao { owner, list, .. } => {
                for c in list {
                    self.chao.insert((*owner, c.index), c.position);
                }
            }
            Body::WildAsk {
                area,
                ask,
                section,
                land,
                pond,
                ..
            } if self.mode("pick") => {
                let mut chosen = *section;
                let mut list = Vec::new();
                for i in 0..land + pond {
                    let in_pond = i >= *land;
                    if let Some((s, creature)) = self.pick(chosen, in_pond) {
                        chosen = Some(s);
                        let arrive = if in_pond {
                            WildArrive::Pond
                        } else {
                            WildArrive::Land
                        };
                        list.push(WildPicked { creature, arrive });
                    }
                }
                let section = chosen
                    .or_else(|| self.maps.first().map(|(s, _)| *s))
                    .unwrap_or(0);
                println!(
                    "wild: picked {} for garden {area} from section {section}",
                    list.len()
                );
                return vec![(
                    Body::WildPick {
                        area: *area,
                        ask: *ask,
                        section,
                        list,
                    },
                    true,
                )];
            }
            Body::WildAnswer { kind, id, ok, .. } => {
                println!(
                    "wild: {kind:?} {id}: {}",
                    if *ok { "granted" } else { "refused" }
                );
                match (kind, self.task, ok) {
                    (WildRequest::Take, Some((Task::Take(t), _)), true) if t == *id => {
                        self.holding = Some(*id);
                        self.task = Some((Task::Hold(*id), now + Duration::from_secs(10)));
                    }
                    (WildRequest::Battle, Some((Task::Battle(t), _)), true) if t == *id => {
                        let step = if self.mode("battle-won") {
                            WildStep::Won
                        } else {
                            WildStep::Lost
                        };
                        self.stand_at = None;
                        // Longer than a run with one player in it lasts, as a real battle is.
                        self.task = Some((Task::Result(*id, step), now + Duration::from_secs(25)));
                    }
                    (_, _, false) => {
                        self.task = None;
                        self.stand_at = None;
                    }
                    _ => {}
                }
            }
            Body::WildTaught {
                identity, creature, ..
            } => println!(
                "wild: our Chao {identity:016x} learned from species {}",
                creature.species
            ),
            Body::WildUsed { garden, key } => {
                println!("wild: our partner {key:x} in garden {garden} was used")
            }
            _ => {}
        }
        Vec::new()
    }

    /// The roaming visitor nearest us.
    fn nearest(&self) -> Option<WildEntry> {
        let d = |e: &WildEntry| (e.v[0] - self.at[0]).powi(2) + (e.v[2] - self.at[2]).powi(2);
        self.list
            .iter()
            .filter(|e| e.place == WildPlace::Roam)
            .min_by(|a, b| d(a).total_cmp(&d(b)))
            .copied()
    }

    /// What to send now.
    fn tick(&mut self, now: Instant) -> Vec<(Body, bool)> {
        let mut out = Vec::new();
        if !self.said_hello {
            self.said_hello = true;
            out.push((self.hello(), true));
        }
        if self.mode("whistle") && now.duration_since(self.last_whistle) >= Duration::from_secs(20)
        {
            self.last_whistle = now;
            println!("wild: whistling");
        }
        if self.mode("hostile") && now.duration_since(self.last_hostile) >= Duration::from_secs(5) {
            self.last_hostile = now;
            out.extend(self.hostile().into_iter().map(|b| (b, true)));
        }
        if let Some(id) = self.holding
            && now.duration_since(self.last_held) >= Duration::from_millis(100)
        {
            self.last_held = now;
            out.push((
                Body::WildHeld {
                    id,
                    let_go: false,
                    v: [0.0, 5.0, 2.0],
                },
                false,
            ));
        }
        if self.away_until.is_some_and(|t| now >= t) {
            self.away_until = None;
        }
        let idle = self.task.is_none() && self.away_until.is_none();
        let every = if self.mode("take") { 30 } else { 40 };
        if idle && now.duration_since(self.last_try) >= Duration::from_secs(every) {
            self.last_try = now;
            if let Some(v) = self.nearest() {
                let task = if self.mode("take") {
                    Some(Task::Take(v.id))
                } else if self.mode("battle-won") || self.mode("battle-lost") {
                    Some(Task::Battle(v.id))
                } else {
                    None
                };
                if let Some(task) = task {
                    println!("wild: going to visitor {} for {task:?}", v.id);
                    self.stand_at = Some([v.v[0] + 2.0, v.v[1], v.v[2]]);
                    self.task = Some((task, now + Duration::from_millis(1500)));
                }
            }
        }
        let Some((task, _)) = self.task.filter(|(_, at)| now >= *at) else {
            return out;
        };
        match task {
            Task::Take(id) => {
                out.push((Body::WildTake { id }, true));
                self.task = Some((task, now + Duration::from_secs(5)));
            }
            Task::Battle(id) => {
                out.push((
                    Body::WildBattle {
                        id,
                        step: WildStep::Reserve,
                    },
                    true,
                ));
                self.task = Some((task, now + Duration::from_secs(5)));
            }
            Task::Hold(id) if self.mode("absorb") => {
                let player = self.player.unwrap_or(0);
                let at = self.at;
                let d = |p: &[f32; 3]| (p[0] - at[0]).powi(2) + (p[2] - at[2]).powi(2);
                let chao = self
                    .chao
                    .iter()
                    .filter(|((owner, _), _)| *owner == player)
                    .min_by(|a, b| d(a.1).total_cmp(&d(b.1)))
                    .map(|(k, p)| (*k, *p));
                match chao {
                    Some((key, p)) => {
                        println!("wild: handing visitor {id} to Chao {key:?}");
                        self.stand_at = Some([p[0] + 1.0, p[1], p[2]]);
                        self.task = Some((Task::Absorb(id, key), now + Duration::from_secs(1)));
                    }
                    None => self.task = Some((task, now + Duration::from_secs(5))),
                }
            }
            Task::Hold(id) if self.mode("carry") => {
                println!("wild: carrying visitor {id} out to the lobby");
                self.holding = None;
                self.task = None;
                self.stand_at = None;
                self.away_until = Some(now + Duration::from_secs(10));
            }
            Task::Hold(id) => {
                println!("wild: putting visitor {id} down");
                out.push((
                    Body::WildPut {
                        id,
                        at: self.at,
                        angle: 0,
                    },
                    true,
                ));
                self.holding = None;
                self.task = None;
                self.stand_at = None;
            }
            Task::Absorb(id, (owner, index)) => {
                out.push((Body::WildAbsorbed { id, owner, index }, true));
                self.holding = None;
                self.task = None;
                self.stand_at = None;
            }
            Task::Result(id, step) => {
                println!("wild: battle with visitor {id}: {step:?}");
                out.push((Body::WildBattle { id, step }, true));
                self.task = None;
            }
        }
        out
    }

    /// Messages the host must shrug off: unknown or others' visitors, picks without an ask,
    /// values out of range (refused by decoding).
    fn hostile(&mut self) -> Vec<Body> {
        let id = self.below(0x10000) as u16;
        let known = self.list.first().map_or(1, |e| e.id);
        let creature = WildCreature {
            species: self.below(500) as u16,
            level: self.below(200) as u8,
            ability: self.below(100) as u8,
            shiny: false,
            moves: [self.below(400) as u16, 0, 0, 0],
        };
        let far = [1.0e5, -1.0e5, 3.0];
        vec![
            Body::WildTake { id },
            Body::WildTake { id: 0 },
            Body::WildPut {
                id: known,
                at: far,
                angle: 0,
            },
            Body::WildHeld {
                id: known,
                let_go: true,
                v: far,
            },
            Body::WildAbsorbed {
                id: known,
                owner: self.below(16) as u8,
                index: self.below(4) as u8,
            },
            Body::WildBattle {
                id: known,
                step: WildStep::Lost,
            },
            Body::WildPick {
                area: 1 + self.below(3) as u8,
                ask: id,
                section: 1,
                list: vec![WildPicked {
                    creature,
                    arrive: WildArrive::SwimAt { at: far, angle: 0 },
                }],
            },
            Body::WildCarry {
                area: 1 + self.below(3) as u8,
                creature,
                offset: [0.0, 5.0, 2.0],
            },
            Body::WildBring {
                area: 1 + self.below(3) as u8,
                key: id.into(),
                from_save: true,
                creature,
                at: Some((far, 0)),
            },
        ]
    }
}

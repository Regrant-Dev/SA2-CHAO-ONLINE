//! Session messages (sent over the transport in `udp.rs`). See docs/protocol.md.

use super::wire::{Reader, Result, WireError, Writer};
use crate::chao::{CHAO_SIZE, ChaoData};

/// Bump on any incompatible change to messages or framing.
pub const PROTOCOL_VERSION: u16 = 15;
pub const MAX_PLAYERS: usize = 8;
/// Highest level number a room may pick.
const MAX_LEVEL: i16 = 100;

/// Chao each player brings into a shared garden.
pub const MAX_CARRIED_CHAO: usize = 2;
/// `Body::Chao` index of the player's chosen Chao for races and karate ("My Chao").
pub const MY_CHAO_INDEX: u8 = 2;
/// Chao slots per player in `Body::Chao`: the garden Chao, then "My Chao".
pub const CHAO_SLOTS: usize = 3;
/// Player names: short enough for a tidy name tag.
pub const MAX_NAME: usize = 10;
pub const MAX_REASON: usize = 64;
pub const MAX_VERSION: usize = 16;
/// Positions are world units; anything beyond this is garbage.
const MAX_COORD: f32 = 1.0e6;
/// Karate HP/morale are stat + 1000, at most a few thousand.
const MAX_KARATE_HP: f32 = 1.0e5;
/// Animation frames; real motions are a few hundred frames long.
const MAX_FRAME: f32 = 1.0e5;

/// Identifies the exact game build (PE TimeDateStamp + SizeOfImage).
pub type GameBuild = [u8; 8];

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Hello {
        protocol: u16,
        mod_version: String,
        game_build: GameBuild,
        name: String,
    },
    Welcome {
        your_id: u8,
        players: Vec<(u8, String)>,
    },
    Reject {
        reason: String,
    },
    PeerJoined {
        id: u8,
        name: String,
    },
    PeerLeft {
        id: u8,
    },
    Heartbeat,
    Bye,
    /// Gameplay data. `from` is set by the host when relaying; clients' values are overwritten.
    Game {
        from: u8,
        body: Body,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    PlayerState(PlayerState),
    /// A Chao a player brings: `index` < MAX_CARRIED_CHAO = their garden Chao (visitors),
    /// `MY_CHAO_INDEX` = the Chao they chose for races and karate.
    Chao {
        index: u8,
        data: Box<ChaoData>,
    },
    /// Room: the creator picked this stage; members on Stage Select go along.
    RoomPick {
        level: i16,
        mission: u8,
        character: u8,
    },
    /// Room: the creator confirmed this Chao Race; members at the race entrance go along.
    RacePick {
        category: u8,
        course: u8,
        level: u8,
    },
    /// Live karate: the sender (leading) starts a VS fight against `opponent`'s Chao.
    KaratePick {
        opponent: u8,
    },
    /// Live karate: the leader's fight, ~30 per second.
    KarateState(KarateState),
    /// Live karate: how the leader's fight ended (`code` 2 = knockout/ring-out, 3 = time up;
    /// `result` 0/1 = side that won, 2 = draw) and the final HP of both sides.
    KarateEnd {
        code: u8,
        result: u8,
        hp: [f32; 2],
    },
    /// Live karate: the follower pressed A (cheer) this many times since the last message.
    KarateCheer {
        count: u8,
    },
}

/// Live karate: one fighter as the leader's game has it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KarateFighter {
    pub position: [f32; 3],
    pub rotation: [i32; 3],
    pub hp: f32,
    pub morale: f32,
    pub motion: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KarateState {
    /// Seconds left on the fight clock.
    pub clock: u16,
    /// Side 0 (the leader's Chao) and side 1.
    pub fighters: [KarateFighter; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerState {
    /// Sender's frame counter (60 per second), for smooth playback (see timeline.rs).
    pub time: u32,
    pub level: i16,
    /// Mission number within the stage (0..=4).
    pub mission: u8,
    pub chao_area: i8,
    /// Character ID (`CharObj2Base::CharID`): 0 Sonic, 1 Shadow, 2 Tails, 3 Eggman, ...
    pub character: u8,
    pub position: [f32; 3],
    /// Angles (BAMS, 0x10000 = full turn): x, y, z.
    pub rotation: [i32; 3],
    /// Index into the character's animation list.
    pub anim: u16,
    /// Current frame of that animation.
    pub frame: f32,
    /// `game::POSE_*` bits (spin ball, blending).
    pub flags: u8,
    /// The room the player is in: (creator's id, kind), or (0xFF, 0) for none (room.rs).
    pub room: (u8, u8),
    /// `CharObj2Base::Upgrades` (mechs draw their upgrade parts from it).
    pub upgrades: u32,
}

mod tag {
    pub const HELLO: u8 = 1;
    pub const WELCOME: u8 = 2;
    pub const REJECT: u8 = 3;
    pub const PEER_JOINED: u8 = 4;
    pub const PEER_LEFT: u8 = 5;
    pub const HEARTBEAT: u8 = 6;
    pub const BYE: u8 = 7;
    pub const GAME: u8 = 8;

    pub const PLAYER_STATE: u8 = 1;
    pub const CHAO: u8 = 2;
    // 3 is unused (an early fight request).
    pub const ROOM_PICK: u8 = 4;
    pub const RACE_PICK: u8 = 5;
    pub const KARATE_PICK: u8 = 6;
    pub const KARATE_STATE: u8 = 7;
    pub const KARATE_END: u8 = 8;
    pub const KARATE_CHEER: u8 = 9;
}

fn player_id(r: &mut Reader) -> Result<u8> {
    let id = r.u8()?;
    if (id as usize) < MAX_PLAYERS {
        Ok(id)
    } else {
        Err(WireError::BadValue("player id"))
    }
}

fn name(r: &mut Reader) -> Result<String> {
    let s = r.str(MAX_NAME)?;
    if s.is_empty() {
        Err(WireError::BadString)
    } else {
        Ok(s)
    }
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            Self::Hello {
                protocol,
                mod_version,
                game_build,
                name,
            } => {
                w.u8(tag::HELLO)
                    .u16(*protocol)
                    .str(mod_version)
                    .bytes(game_build)
                    .str(name);
            }
            Self::Welcome { your_id, players } => {
                w.u8(tag::WELCOME).u8(*your_id).u8(players.len() as u8);
                for (id, name) in players {
                    w.u8(*id).str(name);
                }
            }
            Self::Reject { reason } => {
                w.u8(tag::REJECT).str(reason);
            }
            Self::PeerJoined { id, name } => {
                w.u8(tag::PEER_JOINED).u8(*id).str(name);
            }
            Self::PeerLeft { id } => {
                w.u8(tag::PEER_LEFT).u8(*id);
            }
            Self::Heartbeat => {
                w.u8(tag::HEARTBEAT);
            }
            Self::Bye => {
                w.u8(tag::BYE);
            }
            Self::Game { from, body } => {
                w.u8(tag::GAME).u8(*from);
                body.encode(&mut w);
            }
        }
        w.buf
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let msg = match r.u8()? {
            tag::HELLO => Self::Hello {
                protocol: r.u16()?,
                mod_version: r.str(MAX_VERSION)?,
                game_build: r.take(8)?.try_into().unwrap(),
                name: name(&mut r)?,
            },
            tag::WELCOME => {
                let your_id = player_id(&mut r)?;
                let count = r.u8()? as usize;
                if count > MAX_PLAYERS {
                    return Err(WireError::BadValue("player count"));
                }
                let players = (0..count)
                    .map(|_| Ok((player_id(&mut r)?, name(&mut r)?)))
                    .collect::<Result<_>>()?;
                Self::Welcome { your_id, players }
            }
            tag::REJECT => Self::Reject {
                reason: r.str(MAX_REASON)?,
            },
            tag::PEER_JOINED => Self::PeerJoined {
                id: player_id(&mut r)?,
                name: name(&mut r)?,
            },
            tag::PEER_LEFT => Self::PeerLeft {
                id: player_id(&mut r)?,
            },
            tag::HEARTBEAT => Self::Heartbeat,
            tag::BYE => Self::Bye,
            tag::GAME => Self::Game {
                from: player_id(&mut r)?,
                body: Body::decode(&mut r)?,
            },
            t => return Err(WireError::BadTag("message", t)),
        };
        r.finish()?;
        Ok(msg)
    }
}

impl Body {
    fn encode(&self, w: &mut Writer) {
        match self {
            Self::PlayerState(s) => {
                w.u8(tag::PLAYER_STATE)
                    .u32(s.time)
                    .i16(s.level)
                    .u8(s.mission)
                    .i8(s.chao_area)
                    .u8(s.character);
                for v in s.position {
                    w.f32(v);
                }
                for a in s.rotation {
                    w.i32(a);
                }
                w.u16(s.anim)
                    .f32(s.frame)
                    .u8(s.flags)
                    .u8(s.room.0)
                    .u8(s.room.1)
                    .u32(s.upgrades);
            }
            Self::Chao { index, data } => {
                w.u8(tag::CHAO).u8(*index).bytes(&data.bytes);
            }
            Self::RoomPick {
                level,
                mission,
                character,
            } => {
                w.u8(tag::ROOM_PICK).i16(*level).u8(*mission).u8(*character);
            }
            Self::RacePick {
                category,
                course,
                level,
            } => {
                w.u8(tag::RACE_PICK).u8(*category).u8(*course).u8(*level);
            }
            Self::KaratePick { opponent } => {
                w.u8(tag::KARATE_PICK).u8(*opponent);
            }
            Self::KarateState(s) => {
                w.u8(tag::KARATE_STATE).u16(s.clock);
                for f in &s.fighters {
                    for v in f.position {
                        w.f32(v);
                    }
                    for a in f.rotation {
                        w.i32(a);
                    }
                    w.f32(f.hp).f32(f.morale).u8(f.motion);
                }
            }
            Self::KarateCheer { count } => {
                w.u8(tag::KARATE_CHEER).u8(*count);
            }
            Self::KarateEnd { code, result, hp } => {
                w.u8(tag::KARATE_END)
                    .u8(*code)
                    .u8(*result)
                    .f32(hp[0])
                    .f32(hp[1]);
            }
        }
    }

    fn decode(r: &mut Reader) -> Result<Self> {
        Ok(match r.u8()? {
            tag::PLAYER_STATE => Self::PlayerState(PlayerState {
                time: r.u32()?,
                level: r.i16()?,
                mission: r.u8()?,
                chao_area: r.i8()?,
                character: r.u8()?,
                position: [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                rotation: [r.i32()?, r.i32()?, r.i32()?],
                anim: r.u16()?,
                frame: r.f32(MAX_FRAME)?,
                flags: r.u8()?,
                room: (r.u8()?, r.u8()?),
                upgrades: r.u32()?,
            }),
            tag::CHAO => {
                let index = r.u8()?;
                if usize::from(index) >= CHAO_SLOTS {
                    return Err(WireError::BadValue("chao index"));
                }
                let chao = ChaoData::from_bytes(r.take(CHAO_SIZE)?).ok_or(WireError::Truncated)?;
                chao.validate()
                    .map_err(|_| WireError::BadValue("chao data"))?;
                Self::Chao {
                    index,
                    data: Box::new(chao),
                }
            }
            tag::ROOM_PICK => {
                let (level, mission, character) = (r.i16()?, r.u8()?, r.u8()?);
                // Real stages are 1..=70ish; missions 0..=4; characters 0..=15.
                if !(1..=MAX_LEVEL).contains(&level) || mission > 4 || character > 15 {
                    return Err(WireError::BadValue("room pick"));
                }
                Self::RoomPick {
                    level,
                    mission,
                    character,
                }
            }
            tag::RACE_PICK => {
                let (category, course, level) = (r.u8()?, r.u8()?, r.u8()?);
                // Categories 0..=4 (Party races are local multiplayer), courses 0..=12,
                // levels 0..=11 (re/chao-race-sync.md).
                if category > 4 || course > 12 || level > 11 {
                    return Err(WireError::BadValue("race pick"));
                }
                Self::RacePick {
                    category,
                    course,
                    level,
                }
            }
            tag::KARATE_PICK => Self::KaratePick {
                opponent: player_id(r)?,
            },
            tag::KARATE_STATE => {
                let clock = r.u16()?;
                let mut fighters = [KarateFighter::default(); 2];
                for f in &mut fighters {
                    f.position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                    f.rotation = [r.i32()?, r.i32()?, r.i32()?];
                    f.hp = r.f32(MAX_KARATE_HP)?;
                    f.morale = r.f32(MAX_KARATE_HP)?;
                    f.motion = r.u8()?;
                }
                Self::KarateState(KarateState { clock, fighters })
            }
            tag::KARATE_CHEER => Self::KarateCheer { count: r.u8()? },
            tag::KARATE_END => {
                let (code, result) = (r.u8()?, r.u8()?);
                if !matches!(code, 2 | 3) || result > 2 {
                    return Err(WireError::BadValue("karate end"));
                }
                Self::KarateEnd {
                    code,
                    result,
                    hp: [r.f32(MAX_KARATE_HP)?, r.f32(MAX_KARATE_HP)?],
                }
            }
            t => return Err(WireError::BadTag("body", t)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_chao() -> ChaoData {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        c.bytes[0x80] = 0x05;
        c.bytes[0x81] = 1;
        c
    }

    fn all_messages() -> Vec<Message> {
        vec![
            Message::Hello {
                protocol: PROTOCOL_VERSION,
                mod_version: "0.0.1".into(),
                game_build: [1, 2, 3, 4, 5, 6, 7, 8],
                name: "Regrant".into(),
            },
            Message::Welcome {
                your_id: 3,
                players: vec![(0, "host".into()), (3, "me".into())],
            },
            Message::Reject {
                reason: "lobby full".into(),
            },
            Message::PeerJoined {
                id: 7,
                name: "seven".into(),
            },
            Message::PeerLeft { id: 2 },
            Message::Heartbeat,
            Message::Bye,
            Message::Game {
                from: 1,
                body: Body::PlayerState(PlayerState {
                    time: 123_456,
                    level: 13,
                    mission: 2,
                    chao_area: -1,
                    character: 1,
                    position: [1.5, -20.0, 300.25],
                    rotation: [0x100, 0x4000, -0x200],
                    anim: 12,
                    frame: 37.5,
                    flags: 1,
                    room: (0, 2),
                    upgrades: 0x40_0000,
                }),
            },
            Message::Game {
                from: 2,
                body: Body::Chao {
                    index: 1,
                    data: Box::new(valid_chao()),
                },
            },
            Message::Game {
                from: 1,
                body: Body::KaratePick { opponent: 2 },
            },
            Message::Game {
                from: 2,
                body: Body::KarateCheer { count: 3 },
            },
            Message::Game {
                from: 1,
                body: Body::KarateState(KarateState {
                    clock: 42,
                    fighters: [
                        KarateFighter {
                            position: [-5.0, 8.0, 19.0],
                            rotation: [0, 0x4000, 0],
                            hp: 1200.0,
                            morale: 900.0,
                            motion: 3,
                        },
                        KarateFighter::default(),
                    ],
                }),
            },
            Message::Game {
                from: 1,
                body: Body::KarateEnd {
                    code: 3,
                    result: 2,
                    hp: [10.0, 10.0],
                },
            },
            Message::Game {
                from: 4,
                body: Body::RacePick {
                    category: 1,
                    course: 6,
                    level: 2,
                },
            },
            Message::Game {
                from: 3,
                body: Body::RoomPick {
                    level: 13,
                    mission: 2,
                    character: 0,
                },
            },
        ]
    }

    #[test]
    fn round_trip_every_message() {
        for msg in all_messages() {
            assert_eq!(Message::decode(&msg.encode()), Ok(msg.clone()), "{msg:?}");
        }
    }

    #[test]
    fn rejects_bad_ids_and_values() {
        assert!(Message::decode(&Message::PeerLeft { id: 8 }.encode()).is_err());
        let mut bad = Message::Game {
            from: 0,
            body: Body::PlayerState(PlayerState {
                time: 0,
                level: 0,
                mission: 0,
                chao_area: 0,
                character: 0,
                position: [0.0; 3],
                rotation: [0; 3],
                anim: 0,
                frame: 0.0,
                flags: 0,
                room: (0xFF, 0),
                upgrades: 0,
            }),
        }
        .encode();
        // Overwrite position.x with NaN.
        bad[12..16].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(
            Message::decode(&bad),
            Err(WireError::BadValue("float out of range"))
        );

        let mut chao = valid_chao();
        chao.bytes[0x81] = 0;
        let bad = Message::Game {
            from: 0,
            body: Body::Chao {
                index: 0,
                data: Box::new(chao),
            },
        }
        .encode();
        assert_eq!(Message::decode(&bad), Err(WireError::BadValue("chao data")));
    }

    #[test]
    fn rejects_trailing_and_truncated() {
        let mut data = Message::Heartbeat.encode();
        data.push(0);
        assert_eq!(Message::decode(&data), Err(WireError::Trailing(1)));
        let data = Message::PeerJoined {
            id: 1,
            name: "abc".into(),
        }
        .encode();
        assert_eq!(
            Message::decode(&data[..data.len() - 1]),
            Err(WireError::Truncated)
        );
        assert_eq!(Message::decode(&[]), Err(WireError::Truncated));
    }

    #[test]
    fn random_bytes_never_panic() {
        // Simple LCG so the test is deterministic without extra crates.
        let mut state = 0x1234_5678u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        for msg in all_messages() {
            let good = msg.encode();
            for _ in 0..500 {
                let mut data = good.clone();
                let flips = 1 + next() as usize % 4;
                for _ in 0..flips {
                    let i = next() as usize % data.len();
                    data[i] = next();
                }
                let cut = next() as usize % (data.len() + 1);
                let _ = Message::decode(&data[..cut]);
                let _ = Message::decode(&data);
            }
        }
        for len in 0..64 {
            let data: Vec<u8> = (0..len).map(|_| next()).collect();
            let _ = Message::decode(&data);
        }
    }
}

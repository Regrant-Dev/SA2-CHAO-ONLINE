//! Session messages (sent over the transport in `udp.rs`). See docs/protocol.md.

use super::wire::{Reader, Result, WireError, Writer};
use crate::chao::{CHAO_SIZE, ChaoData};

/// Bump on any incompatible change to messages or framing.
pub const PROTOCOL_VERSION: u16 = 16;
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
/// Changed fields per `ChaoChanges` (the whitelist in `chao::changes` has fewer).
const MAX_CHAO_CHANGES: usize = 128;
/// Stage object records per stage (`SETObjectData[0x600]`, re/stage-objects.md).
pub const MAX_STAGE_OBJECTS: u16 = 0x600;
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
    /// Shared garden: the sender's visiting Chao (their `Body::Chao` indices) as they are in
    /// the sender's game right now, ~30 per second.
    GardenChao(Vec<GardenChao>),
    /// Shared garden: the sender's interaction changed `owner`'s visiting Chao `index`
    /// (`chao::changes::diff` output) for the owner to apply to the real one.
    ChaoChanges {
        owner: u8,
        index: u8,
        changes: Vec<(u16, f32)>,
    },
    /// Shared garden: the sender is holding `owner`'s Chao `index`, at `offset` from themselves
    /// in their own frame. ~30 per second while held.
    ChaoHeld {
        owner: u8,
        index: u8,
        offset: [f32; 3],
    },
    /// Shared garden: items the sender owns in garden `area` (1..3), ~10 per second, in
    /// chunks of at most `MAX_GARDEN_ITEMS`.
    GardenItems {
        area: u8,
        items: Vec<GardenItem>,
    },
    /// Shared garden, to the host only: the sender picked up (or their Chao took) item `id`
    /// and asks to own it. The host's referee answers with `ItemOwner` (net/referee.rs).
    ItemClaim {
        id: ItemId,
    },
    /// From the host's referee: item `id` belongs to `owner` (a granted claim, or the answer to
    /// a refused one).
    ItemOwner {
        id: ItemId,
        owner: u8,
    },
    /// Co-op room: these stage objects (SET record indices) were destroyed in this stage.
    StageKills {
        level: i16,
        mission: u8,
        objects: Vec<u16>,
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

/// One of a player's Chao in their garden (re/chao-garden-sync.md).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GardenChao {
    /// `Body::Chao` index (< MAX_CARRIED_CHAO).
    pub index: u8,
    pub position: [f32; 3],
    /// Facing (BAMS).
    pub rotation_y: i32,
    /// Animation id (the shared Chao motion table).
    pub motion: u16,
    /// Held by its owner; `offset` is then where it is relative to them, in their own frame.
    pub held: bool,
    pub offset: [f32; 3],
    /// Mood shown over it (heart, "?", "!", swirl, ...): current state (0..=5), timer, length.
    pub bubble: (i16, i32, i32),
    /// Face: eyes and mouth ids.
    pub eyes: i16,
    pub mouth: i16,
}

/// A shared garden item, the same in every game: who first had it and their number for it.
/// It keeps this id when it changes hands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId {
    pub origin: u8,
    pub serial: u16,
}

/// Where a shared garden item is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ItemPlace {
    /// On the ground (or flying): `position` is where.
    #[default]
    Ground,
    /// In its owner's hands: `position` is the offset from them, in their frame.
    Player,
    /// Held or being eaten by the owner's visiting Chao `n` (`Body::Chao` index): offset from
    /// it, in its frame.
    Chao(u8),
    /// In use where others can't see it (one of the owner's other Chao has it): `position` is
    /// where it is. Not claimable, like the others.
    Busy,
}

/// A garden item its owner has (re/chao-garden-items.md 3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GardenItem {
    pub id: ItemId,
    /// ALW category: 2 animal or Chaos Drive, 3 fruit, 7 seed, 9 hat.
    pub category: u8,
    /// Fruit/seed/hat kind, animal id (0..20) or Chaos Drive colour + 0x15.
    pub kind: u8,
    /// Fruit: bites left (shape).
    pub bites: u8,
    pub place: ItemPlace,
    pub position: [f32; 3],
}

impl GardenItem {
    /// The owner is using it (holding it, or their Chao is): nobody else may claim it.
    pub fn busy(&self) -> bool {
        self.place != ItemPlace::Ground
    }
}

/// Items per `GardenItems` message.
pub const MAX_GARDEN_ITEMS: usize = 24;

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
    pub const STAGE_KILLS: u8 = 10;
    pub const GARDEN_CHAO: u8 = 11;
    pub const CHAO_CHANGES: u8 = 12;
    pub const CHAO_HELD: u8 = 13;
    pub const GARDEN_ITEMS: u8 = 14;
    pub const ITEM_CLAIM: u8 = 15;
    pub const ITEM_OWNER: u8 = 16;
}

fn item_id(r: &mut Reader) -> Result<ItemId> {
    Ok(ItemId {
        origin: player_id(r)?,
        serial: r.u16()?,
    })
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
            Self::GardenChao(list) => {
                w.u8(tag::GARDEN_CHAO).u8(list.len() as u8);
                for c in list {
                    w.u8(c.index);
                    for v in c.position {
                        w.f32(v);
                    }
                    w.i32(c.rotation_y).u16(c.motion).u8(u8::from(c.held));
                    for v in c.offset {
                        w.f32(v);
                    }
                    w.i16(c.bubble.0)
                        .i32(c.bubble.1)
                        .i32(c.bubble.2)
                        .i16(c.eyes)
                        .i16(c.mouth);
                }
            }
            Self::ChaoChanges {
                owner,
                index,
                changes,
            } => {
                w.u8(tag::CHAO_CHANGES)
                    .u8(*owner)
                    .u8(*index)
                    .u8(changes.len() as u8);
                for &(at, value) in changes {
                    w.u16(at).f32(value);
                }
            }
            Self::ChaoHeld {
                owner,
                index,
                offset,
            } => {
                w.u8(tag::CHAO_HELD).u8(*owner).u8(*index);
                for v in offset {
                    w.f32(*v);
                }
            }
            Self::GardenItems { area, items } => {
                w.u8(tag::GARDEN_ITEMS).u8(*area).u8(items.len() as u8);
                for i in items {
                    let (place, chao) = match i.place {
                        ItemPlace::Ground => (0, 0),
                        ItemPlace::Player => (1, 0),
                        ItemPlace::Chao(n) => (2, n),
                        ItemPlace::Busy => (3, 0),
                    };
                    w.u8(i.id.origin)
                        .u16(i.id.serial)
                        .u8(i.category)
                        .u8(i.kind)
                        .u8(i.bites)
                        .u8(place)
                        .u8(chao);
                    for v in i.position {
                        w.f32(v);
                    }
                }
            }
            Self::ItemClaim { id } => {
                w.u8(tag::ITEM_CLAIM).u8(id.origin).u16(id.serial);
            }
            Self::ItemOwner { id, owner } => {
                w.u8(tag::ITEM_OWNER)
                    .u8(id.origin)
                    .u16(id.serial)
                    .u8(*owner);
            }
            Self::StageKills {
                level,
                mission,
                objects,
            } => {
                w.u8(tag::STAGE_KILLS)
                    .i16(*level)
                    .u8(*mission)
                    .u16(objects.len() as u16);
                for &n in objects {
                    w.u16(n);
                }
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
            tag::GARDEN_CHAO => {
                let count = usize::from(r.u8()?);
                if count > MAX_CARRIED_CHAO {
                    return Err(WireError::BadValue("garden chao count"));
                }
                let mut list = Vec::with_capacity(count);
                for _ in 0..count {
                    let index = r.u8()?;
                    if usize::from(index) >= MAX_CARRIED_CHAO {
                        return Err(WireError::BadValue("garden chao index"));
                    }
                    let position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                    let (rotation_y, motion, held) = (r.i32()?, r.u16()?, r.u8()?);
                    if held > 1 {
                        return Err(WireError::BadValue("garden chao held"));
                    }
                    let offset = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                    let bubble = (r.i16()?, r.i32()?, r.i32()?);
                    let (eyes, mouth) = (r.i16()?, r.i16()?);
                    list.push(GardenChao {
                        index,
                        position,
                        rotation_y,
                        motion,
                        held: held == 1,
                        offset,
                        bubble,
                        eyes,
                        mouth,
                    });
                }
                Self::GardenChao(list)
            }
            tag::CHAO_CHANGES => {
                let (owner, index, count) = (player_id(r)?, r.u8()?, usize::from(r.u8()?));
                if usize::from(index) >= MAX_CARRIED_CHAO || count > MAX_CHAO_CHANGES {
                    return Err(WireError::BadValue("chao changes"));
                }
                let changes = (0..count)
                    .map(|_| {
                        let at = r.u16()?;
                        if usize::from(at) >= CHAO_SIZE {
                            return Err(WireError::BadValue("chao field"));
                        }
                        Ok((at, r.f32(MAX_COORD)?))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::ChaoChanges {
                    owner,
                    index,
                    changes,
                }
            }
            tag::CHAO_HELD => {
                let (owner, index) = (player_id(r)?, r.u8()?);
                if usize::from(index) >= MAX_CARRIED_CHAO {
                    return Err(WireError::BadValue("chao held"));
                }
                Self::ChaoHeld {
                    owner,
                    index,
                    offset: [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                }
            }
            tag::GARDEN_ITEMS => {
                let (area, count) = (r.u8()?, usize::from(r.u8()?));
                if !(1..=3).contains(&area) || count > MAX_GARDEN_ITEMS {
                    return Err(WireError::BadValue("garden items"));
                }
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = item_id(r)?;
                    let (category, kind, bites) = (r.u8()?, r.u8()?, r.u8()?);
                    let place = match (r.u8()?, r.u8()?) {
                        (0, _) => ItemPlace::Ground,
                        (1, _) => ItemPlace::Player,
                        (2, n) if usize::from(n) < MAX_CARRIED_CHAO => ItemPlace::Chao(n),
                        (3, _) => ItemPlace::Busy,
                        _ => return Err(WireError::BadValue("item place")),
                    };
                    if !matches!(category, 2 | 3 | 7 | 9) || bites > 4 {
                        return Err(WireError::BadValue("garden item"));
                    }
                    items.push(GardenItem {
                        id,
                        category,
                        kind,
                        bites,
                        place,
                        position: [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                    });
                }
                Self::GardenItems { area, items }
            }
            tag::ITEM_CLAIM => Self::ItemClaim { id: item_id(r)? },
            tag::ITEM_OWNER => Self::ItemOwner {
                id: item_id(r)?,
                owner: player_id(r)?,
            },
            tag::STAGE_KILLS => {
                let (level, mission) = (r.i16()?, r.u8()?);
                let count = r.u16()?;
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || count > MAX_STAGE_OBJECTS {
                    return Err(WireError::BadValue("stage kills"));
                }
                let objects = (0..count)
                    .map(|_| {
                        let n = r.u16()?;
                        if n < MAX_STAGE_OBJECTS {
                            Ok(n)
                        } else {
                            Err(WireError::BadValue("stage object"))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::StageKills {
                    level,
                    mission,
                    objects,
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
                from: 2,
                body: Body::GardenChao(vec![
                    GardenChao {
                        index: 0,
                        position: [10.0, 0.5, -30.0],
                        rotation_y: 0x8000,
                        motion: 0x101,
                        held: true,
                        offset: [0.0, 4.0, 1.2],
                        bubble: (4, 30, 180),
                        eyes: 9,
                        mouth: 3,
                    },
                    GardenChao::default(),
                ]),
            },
            Message::Game {
                from: 4,
                body: Body::ChaoChanges {
                    owner: 1,
                    index: 0,
                    changes: vec![(0x82, 1.0), (0xB0, -0.05), (0x11C, 7.0)],
                },
            },
            Message::Game {
                from: 4,
                body: Body::ChaoHeld {
                    owner: 1,
                    index: 1,
                    offset: [0.5, 6.0, 2.0],
                },
            },
            Message::Game {
                from: 2,
                body: Body::GardenItems {
                    area: 1,
                    items: vec![
                        GardenItem {
                            id: ItemId {
                                origin: 2,
                                serial: 7,
                            },
                            category: 3,
                            kind: 2,
                            bites: 4,
                            place: ItemPlace::Player,
                            position: [0.0, 5.0, 3.0],
                        },
                        GardenItem {
                            id: ItemId {
                                origin: 5,
                                serial: 300,
                            },
                            category: 2,
                            kind: 0x16,
                            bites: 0,
                            place: ItemPlace::Chao(1),
                            position: [0.0, 1.5, 1.0],
                        },
                        GardenItem {
                            id: ItemId {
                                origin: 0,
                                serial: 0,
                            },
                            category: 9,
                            kind: 3,
                            bites: 0,
                            place: ItemPlace::Ground,
                            position: [10.0, 0.5, -40.0],
                        },
                    ],
                },
            },
            Message::Game {
                from: 3,
                body: Body::ItemClaim {
                    id: ItemId {
                        origin: 2,
                        serial: 7,
                    },
                },
            },
            Message::Game {
                from: 0,
                body: Body::ItemOwner {
                    id: ItemId {
                        origin: 2,
                        serial: 7,
                    },
                    owner: 3,
                },
            },
            Message::Game {
                from: 3,
                body: Body::StageKills {
                    level: 13,
                    mission: 0,
                    objects: vec![0, 57, 0x5FF],
                },
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

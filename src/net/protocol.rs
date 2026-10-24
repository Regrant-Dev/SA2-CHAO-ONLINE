//! Session messages (sent over the transport in `udp.rs`). See docs/protocol.md.

use super::wire::{Reader, Result, WireError, Writer};
use crate::chao::{CHAO_SIZE, ChaoData};

/// Bump on any incompatible change to messages or framing.
pub const PROTOCOL_VERSION: u16 = 27;
pub const MAX_PLAYERS: usize = 8;
/// Highest level number a room may pick.
const MAX_LEVEL: i16 = 100;

/// Chao each player brings into a shared garden.
pub const MAX_CARRIED_CHAO: usize = 4;
/// Record fields a `ChaoLive` carries, (offset, length): happiness, power/run, fly/swim and
/// alignment, emotions, needs and personality, and the six character bonds.
pub const CHAO_LIVE_FIELDS: [(usize, usize); 4] =
    [(0x82, 2), (0xA8, 12), (0x12C, 0x2B), (0x16C, 36)];
/// `Ball::flags`: its owner holds it; it's in the water (`*0x1DBE56C`).
pub const BALL_HELD: u8 = 1;
pub const BALL_IN_WATER: u8 = 2;

/// Record bytes a `ChaoTouched` carries, (offset, length): emotions, needs and personality
/// (the part of `CHAO_LIVE_FIELDS` that never goes to a save).
pub const CHAO_MOOD: (usize, usize) = (0x12C, 0x2B);
/// Bytes in a `ChaoLive`.
pub const CHAO_LIVE_LEN: usize = 2 + 12 + 0x2B + 36;
/// Record fields the host may write into an owner's save (`SaveChao`), (offset, length): stat
/// progress, grades, levels and points; type; happiness; age, adult age, life and max life;
/// the age clock, abilities, alignment and evolution; eyes (an evolution's); the hat worn;
/// animals seen and parts; the emotion clocks, emotions, needs, personality and illnesses;
/// the bond clock and the six character bonds; the DNA grade alleles (an evolution raises
/// one). Everything a Chao's life in a garden changes that the server lives through.
pub const SAVE_CHAO_FIELDS: [(usize, usize); 11] = [
    (0x20, 0x28),
    (0x80, 1),
    (0x82, 2),
    (0x86, 8),
    (0xA4, 0x20),
    (0xD1, 1),
    (0xD5, 1),
    (0x118, 12),
    (0x126, 0x3A),
    (0x168, 0x28),
    (0x494, 0x10),
];

/// Whether `len` bytes at `at` lie inside one of `SAVE_CHAO_FIELDS`.
pub fn save_field(at: usize, len: usize) -> bool {
    SAVE_CHAO_FIELDS
        .iter()
        .any(|&(o, l)| at >= o && at + len <= o + l && len > 0)
}

/// Chao owner ids `HELD_OWNER..=MAX_CHAO_OWNER`: Chao the host holds for players who aren't
/// in that garden (asleep: offline; awake: elsewhere), as `HeldChao` names them. Player ids
/// are below `MAX_PLAYERS`; both fit an `ItemPlace::Chao` byte (owner in the high nibble).
pub const HELD_OWNER: u8 = 8;
pub const MAX_CHAO_OWNER: u8 = 15;
/// Chao one `Brought` carries: each garden's visitor rows.
pub const MAX_BROUGHT: usize = 3 * MAX_CARRIED_CHAO;
/// Spans in one `SaveChao`.
pub const MAX_SAVE_SPANS: usize = 64;
/// Chao one `ChaoLocked` names: every player's brought rows and every held id.
pub const MAX_CHAO_LOCKED: usize = 2 * MAX_PLAYERS * MAX_CARRIED_CHAO;
/// Chao one `Partners` carries.
pub const MAX_PARTNERS: usize = 32;

/// `GardenLand` parts: at most this many bytes each, this many parts.
pub const LAND_PART: usize = 16 * 1024;
pub const MAX_LAND_PARTS: u8 = 64;
/// `Body::Chao` index of the player's chosen Chao for races and karate ("My Chao").
pub const MY_CHAO_INDEX: u8 = MAX_CARRIED_CHAO as u8;
/// Chao slots per player in `Body::Chao`: the garden Chao, then "My Chao".
pub const CHAO_SLOTS: usize = MAX_CARRIED_CHAO + 1;
/// Player names: short enough for a tidy name tag.
pub const MAX_NAME: usize = 10;
pub const MAX_REASON: usize = 64;
pub const MAX_VERSION: usize = 16;
/// Positions are world units; anything beyond this is garbage.
const MAX_COORD: f32 = 1.0e6;
/// Karate HP/morale are stat + 1000, at most a few thousand.
const MAX_KARATE_HP: f32 = 1.0e5;
/// `Body::RaceStatus::progress` at the goal, and without a route.
pub const RACE_PROGRESS_MAX: u16 = 10_000;
pub const RACE_PROGRESS_UNKNOWN: u16 = 0xFFFF;
/// Points in a `Body::Route`.
pub const MAX_ROUTE: usize = 2000;

/// Chat message length (printable ASCII).
pub const MAX_CHAT: usize = 80;
/// Changed fields per `ChaoChanges` (the whitelist in `chao::changes` has fewer).
const MAX_CHAO_CHANGES: usize = 128;
/// Stage object records per stage (`SETObjectData[0x600]`, re/stage-objects.md).
pub const MAX_STAGE_OBJECTS: u16 = 0x600;
/// A mech locks on to at most this many objects (co2+0x368 < 0x20).
pub const MAX_LOCK_ON: u8 = 32;
/// Largest boss HP accepted (bosses use single digits).
pub const MAX_BOSS_HP: f32 = 1000.0;
/// Animation frames; real motions are a few hundred frames long.
const MAX_FRAME: f32 = 1.0e5;
/// Skins: an id (a folder name), files per list page, a file path, a file's size, a chunk.
pub const MAX_SKIN_ID: usize = 24;
pub const SKIN_LIST_PAGE: usize = 48;
pub const MAX_SKIN_PATH: usize = 96;
pub const MAX_SKIN_FILE: u32 = 16 << 20;
pub const MAX_SKIN_FILES: usize = 1024;
pub const SKIN_CHUNK: usize = 1024;
/// `PlayerState::status` bits.
pub const STATUS_GROUNDED: u8 = 1;
pub const STATUS_HOLDING: u8 = 2;
/// Just hurt: it can't be hurt again for a moment, and blinks meanwhile (EntityData1 +6).
pub const STATUS_INVULNERABLE: u8 = 4;
/// Standing still by the game's own speed (CharObj2 +0x64): its puppet's skin stands, whatever
/// small moves its position makes.
pub const STATUS_STILL: u8 = 8;

/// Identifies the exact game build (PE TimeDateStamp + SizeOfImage).
pub type GameBuild = [u8; 8];
/// A player's own random key (config.ini `PlayerKey`), made once and kept: the host knows
/// their Chao by it from one session to the next.
pub type PlayerKey = [u8; 16];

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Hello {
        protocol: u16,
        mod_version: String,
        game_build: GameBuild,
        name: String,
        key: PlayerKey,
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
    /// Shared garden: player `owner`'s visiting Chao (their `Body::Chao` indices) as they are in
    /// the sender's game right now, ~30 per second. Each player sends their own (so the others
    /// know they're visiting); the garden's keeper also sends everyone else's, which they run.
    GardenChao {
        owner: u8,
        list: Vec<GardenChao>,
    },
    /// Shared garden `area`: the sender keeps it (runs its Chao, trees and toys) in term `term`,
    /// a few times a second (keeper.rs).
    GardenKeeper {
        area: u8,
        term: u32,
    },
    /// Shared garden: sounds and jingles the sender's game just played for Chao it runs, for
    /// the others' copies of them to play too.
    ChaoSounds {
        sounds: Vec<ChaoSound>,
    },
    /// From the host's referee: shared garden `area` as the server keeps it: its 7 tree records
    /// and each toy's last mode with its age in seconds (`TOY_TV`, `TOY_RADIO`, `TOY_BOX`).
    /// Every couple of seconds (re/chao-garden-items.md 7).
    ServerGarden {
        area: u8,
        trees: [[u8; TREE_RECORD]; TREE_SLOTS],
        toys: [(u8, u16); 3],
    },
    /// From the host's referee: items in garden `area` that are the server's (their owner left;
    /// they were in no one's save). Every second; claimed like anyone's.
    ServerItems {
        area: u8,
        items: Vec<GardenItem>,
    },
    /// From the host: it runs shared garden `area`'s Chao (chao_sim) and this is player
    /// `owner`'s, as `GardenChao` has them; ~30 per second while it does. Everyone in the
    /// garden follows these instead of a keeper's game.
    ServerChao {
        area: u8,
        owner: u8,
        list: Vec<GardenChao>,
    },
    /// From the host: player `owner`'s Chao `index` as the server runs it, the record fields that
    /// change as it lives (`CHAO_LIVE_FIELDS`, in order), about once a second; the others' copy
    /// of it shows them (the status balloon).
    ChaoLive {
        owner: u8,
        index: u8,
        bytes: Vec<u8>,
    },
    /// From the host, to an item's owner: a Chao the server runs has their item (it borrows it,
    /// the item stays in their save): now it's where `item` says (in that Chao's hands, or let
    /// go on the ground), with `item.bites` left; `gone`: eaten up or worn, it's no more.
    ItemSteer {
        area: u8,
        item: GardenItem,
        gone: bool,
    },
    /// To the host: the sender's game runs `owner`'s Chao `index` for a while because they
    /// touched it (threw, petted, fed it; not while holding it: `ChaoHeld`), and this is where it
    /// is, with its emotions, needs and personality as the touch left them (`CHAO_MOOD`); ~10
    /// per second meanwhile. The server's Chao goes on from there.
    ChaoTouched {
        owner: u8,
        index: u8,
        position: [f32; 3],
        rotation_y: i32,
        /// The motion it plays there (the others are shown it).
        motion: u16,
        mood: Vec<u8>,
    },
    /// To the host: part `part` of `parts` of garden `garden`'s ground (0 Neutral, 1 Hero,
    /// 2 Dark; `chao_sim::land::Garden::to_bytes`, whose fingerprint is `fingerprint`), or with
    /// `garden` 3 the Chao motion table (`chao_sim::motion::MotionTable::to_bytes`), read from
    /// the sender's game, for the host to run Chao on.
    GardenLand {
        garden: u8,
        fingerprint: u32,
        part: u8,
        parts: u8,
        bytes: Vec<u8>,
    },
    /// The sender carries this Chao around Chao World outside the gardens (lobby, kindergarten,
    /// entrances), `offset` from them in their frame; `None`: not any more. On a change and
    /// every second or so while carrying.
    CarriedChao {
        chao: Option<Box<ChaoData>>,
        offset: [f32; 3],
    },
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
    /// Chat: the sender says `text` (printable ASCII, at most `MAX_CHAT`).
    Chat {
        text: String,
    },
    /// Room race in a stage: the sender's progress (0..=`RACE_PROGRESS_MAX`, or
    /// `RACE_PROGRESS_UNKNOWN` without a route) and, once they reached the goal, their race time
    /// (counted from the room's GO; deaths included) and the game's clear time, in centiseconds.
    RaceStatus {
        level: i16,
        mission: u8,
        progress: u16,
        finish: Option<(u32, u32)>,
    },
    /// Room: someone started a Kart Race on `course` (0..=2); members in the Kart Race course
    /// list go along (each with their own driver).
    KartPick {
        course: u8,
    },
    /// The sender's kart in a Kart Race, ~30 per second: where, facing, the display's extra yaw
    /// and wheel angle (BAMS), the kart type (2..=7), accelerator held (bright exhaust) and the
    /// body tilt from jumps and bumps (BAMS).
    KartState {
        position: [f32; 3],
        rotation: [i32; 3],
        yaw: i32,
        wheel: i32,
        kind: u8,
        accel: bool,
        tilt: [i32; 3],
        /// The sender's game frame it was taken on (wraps): exact timing for its motion.
        frame: u16,
    },
    /// Treasure hunt battle: the room's 3 pieces in a hunt stage (kind, id, position), chosen by
    /// its leader's game; everyone hunts the same ones.
    HuntSet {
        level: i16,
        mission: u8,
        pieces: [(u8, u8, [f32; 3]); 3],
    },
    /// To the referee: we collected piece `slot` (0..3) of the hunt with set `sig`.
    HuntClaim {
        level: i16,
        mission: u8,
        sig: u32,
        slot: u8,
    },
    /// From the referee: piece `slot` of hunt `sig` went to `player` (first to collect it).
    HuntTaken {
        level: i16,
        mission: u8,
        sig: u32,
        slot: u8,
        player: u8,
    },
    /// A stage's route (points ~50 units apart, start to goal), for live race positions.
    Route {
        level: i16,
        mission: u8,
        points: Vec<[f32; 3]>,
    },
    /// Co-op room: these stage objects (SET record indices) were destroyed in this stage.
    StageKills {
        level: i16,
        mission: u8,
        objects: Vec<u16>,
    },
    /// Co-op room: stage key `key` (0 or 1) went into its keyhole in this stage; the others'
    /// keys go in too, so their doors open.
    KeyUsed {
        level: i16,
        mission: u8,
        key: u8,
    },
    /// Co-op room: the sender's mech fired (kind 0 bullet, 1 bazooka) from `position` along
    /// `direction`; the others fire the same shot in their game.
    Shot {
        level: i16,
        mission: u8,
        kind: u8,
        position: [f32; 3],
        direction: [f32; 3],
    },
    /// Co-op room: stage switch `id` (< 32) turned on or off in the sender's game; doors and
    /// gates that watch it follow in everyone's game.
    Switch {
        level: i16,
        mission: u8,
        id: u8,
        on: bool,
    },
    /// Co-op room: the sender passed checkpoint `record` (a SET record index) in this stage and
    /// was at `position`/`rotation`; the others mark it passed and restart there too.
    Checkpoint {
        level: i16,
        mission: u8,
        record: u16,
        position: [f32; 3],
        rotation: [i32; 3],
    },
    /// Shared garden: the garden ball as the sender (whoever kicked or held it last, claim
    /// `claim`) has it; the others' ball follows.
    Ball {
        area: u8,
        claim: u16,
        position: [f32; 3],
        velocity: [f32; 3],
        /// `BALL_HELD`, `BALL_IN_WATER`.
        flags: u8,
    },
    /// Shared garden: the sender keeps garden `area`'s trees (the lowest player id there) and
    /// these are their 7 tree spots, as their save has them (8 bytes each: kind, state, growth,
    /// life, fruit ripeness x3, facing; re/chao-garden-sync.md "Trees"). The others show these.
    GardenTrees {
        area: u8,
        trees: [[u8; TREE_RECORD]; TREE_SLOTS],
        /// Bit per spot: its tree wants water (a planted seed, or a grown one whose life fell
        /// under 6000: GrowTree `Data1+0xAC`; its watering sphere 0xAE is on). The record
        /// keeps only life's low byte.
        thirsty: u8,
    },
    /// Shared garden: the sender planted a seed of `kind` in tree spot `slot` (`TREE_PLANT`) or
    /// shook its tree (`TREE_SHAKE`) in their copy of garden `area`'s trees; its keeper does it
    /// for real.
    TreeAction {
        area: u8,
        slot: u8,
        action: u8,
        kind: u8,
    },
    /// Shared garden `area`: the sender's visiting eggs (their `Body::Chao` index) where they lie,
    /// or held by the sender (`held`; the position is then their hands').
    GardenEggs {
        area: u8,
        eggs: Vec<(u8, [f32; 3], bool)>,
    },
    /// Shared garden: the sender picked up (`held`) or put down player `owner`'s egg `index` (a
    /// copy of it in their game), at `position`; the owner's game holds the real one there.
    EggHeld {
        owner: u8,
        index: u8,
        held: bool,
        position: [f32; 3],
    },
    /// Shared garden `area`: the sender's toy `toy` (`TOY_TV`, `TOY_RADIO`, `TOY_BOX`) jumped to
    /// `mode` (switched on, off or over, by a touch or by itself). From the toys' keeper, the
    /// others' toy does the same; to the keeper, it does it for real and tells everyone.
    ToyMode {
        area: u8,
        toy: u8,
        mode: u8,
    },
    /// Co-op room: the sender's boss in this stage is down to `hp` (its bar started at `full`);
    /// the others' boss drops to it too.
    BossHp {
        level: i16,
        mission: u8,
        hp: f32,
        full: f32,
    },
    /// The skin the sender wears (a skin id), or "" for none. Sent when it changes and now
    /// and then; nothing shows on screen.
    SkinWorn {
        skin: String,
    },
    /// To the server: a page of its shared skin files, please.
    SkinListGet {
        page: u16,
    },
    /// From the server: page `page` of `pages` of its shared skin files (index = page *
    /// SKIN_LIST_PAGE + position here).
    SkinList {
        page: u16,
        pages: u16,
        files: Vec<SkinFile>,
    },
    /// To the server: a chunk of shared file `file` from `offset`.
    SkinGet {
        file: u16,
        offset: u32,
    },
    /// From the server: that chunk (at most SKIN_CHUNK bytes).
    SkinChunk {
        file: u16,
        offset: u32,
        data: Vec<u8>,
    },
    /// Co-op room: rings of ring lines and circles collected in this stage: (the group's SET
    /// record index, which ring 0..8). New ones as they're collected; all of them when a room
    /// player enters the stage.
    StageRings {
        level: i16,
        mission: u8,
        rings: Vec<(u16, u8)>,
    },
    /// Co-op room: the sender's mech lock-on: its gun, the stage objects it has locked on to now
    /// (SET record, where it aims) and those it just fired its lasers at. The others draw the
    /// cursors and lasers (only drawn: what the lasers destroy comes as `StageKills`).
    LockOn {
        level: i16,
        mission: u8,
        gun: [f32; 3],
        locked: Vec<(u16, [f32; 3])>,
        fired: Vec<(u16, [f32; 3])>,
    },
    /// Co-op room: the sender is fighting this stage's boss; with `term` > 0 its game leads the
    /// boss (boss_follow.rs: the others' bosses follow it), 0 just says it's there.
    BossClaim {
        level: i16,
        mission: u8,
        term: u32,
    },
    /// Co-op room: the leader's boss as it is now: its Data1 mode bytes, flags and timer, where
    /// it is and faces, and its motion.
    BossState(BossState),
    /// Co-op room: the sender hit its (following) boss; the leader's boss flinches for it.
    BossHit {
        level: i16,
        mission: u8,
    },
    /// Co-op party: the leading boss's attacks in flight (missiles, energy-gun beams, bombs and
    /// pillars, smoke, flames): the others draw the ones aimed at someone else.
    BossAttacks {
        level: i16,
        mission: u8,
        attacks: Vec<BossAttack>,
    },
    /// Co-op party: stage enemies the sender's game runs (it's the nearest player) that another
    /// player is near too: where each is and faces, and how far the sender is from it. The
    /// others' copies follow (enemies.rs).
    EnemyStates {
        level: i16,
        mission: u8,
        enemies: Vec<EnemyState>,
    },
    /// Co-op party: the sender picked up upgrade `index` (its flag in the save), for
    /// `character`, which gave that character's CharObj2 the bits `mask`. Everyone gets it.
    UpgradeGot {
        level: i16,
        mission: u8,
        index: u8,
        character: u8,
        mask: u32,
    },
    /// Party: the sender's Chao World has these gardens open (`GARDEN_HERO`, `GARDEN_DARK`).
    /// The others open them in their own save too (unlocks.rs).
    GardenUnlocks {
        gardens: u8,
    },
    /// To the host: every Chao the sender brings to the shared gardens (each garden's visitor
    /// rows, eggs left out), straight from their save, whichever garden they're in; on
    /// connecting, on entering or leaving a garden and when the set changes. `awake`: the
    /// save is loaded (Chao World opened once), so the host's `SaveChao` reach it; until then
    /// their Chao sleep like an offline player's.
    /// `lock`: when others may change their Chao (F9 "Others can change my Chao"); it goes
    /// again when it changes.
    Brought {
        awake: bool,
        lock: ChaoLock,
        list: Vec<BroughtChao>,
    },
    /// From the host: Chao `owner` (`HELD_OWNER..`) `index` is one it holds for a player who
    /// isn't in that garden, shown there now; `None`: not any more (its owner came, or it
    /// made room). Copies follow its `ServerChao`.
    HeldChao {
        owner: u8,
        index: u8,
        chao: Option<HeldChao>,
    },
    /// From the host (0xFF), to a Chao's owner only: what that Chao's life changed while they
    /// weren't in its garden (they're elsewhere online). Its record bytes, absolute, inside
    /// `SAVE_CHAO_FIELDS`, for the save slot holding the `nth` copy (0 first, by slot order)
    /// of the Chao whose `ChaoData::identity` is `identity`.
    SaveChao {
        identity: u64,
        nth: u8,
        spans: Vec<(u16, Vec<u8>)>,
    },
    /// From the host (0xFF), to each player: the Chao locked against them now (their owners'
    /// `ChaoLock` says no for where those owners are), as (owner, index), present players'
    /// and held ones alike. Sent when that list changes for them, and on (re)connecting. Their
    /// game keeps its copies of these from taking their items and undoes what a touch changes
    /// on them.
    ChaoLocked {
        chao: Vec<(u8, u8)>,
    },
    /// What the sender's Chao have from their GBA link partners, the four moves they battle with
    /// and their battle levels, for battles against copies of them. Sent on connecting, when
    /// someone joins and when one of them changes, in lists of up to `MAX_PARTNERS`.
    Partners {
        list: Vec<SharedPartner>,
    },
    /// To the host only (net/battles.rs): the sender challenges player `to` to a live battle
    /// (`format` 0 singles, 1 doubles) with this team; `dex` fingerprints the ROM data their
    /// battle runs on (`battle::online::dex_print`). Without `save_partners`, both teams leave
    /// out what partners from a GBA save taught (each game applies it to its own team). Without
    /// `actual_levels`, every Chao fights at level 50 (both games apply it to both teams).
    BattleChallenge {
        to: u8,
        format: u8,
        save_partners: bool,
        actual_levels: bool,
        dex: u32,
        team: Vec<BattleMember>,
    },
    /// From the host: player `from` challenges the receiver, with the challenge's rules.
    BattleOffer {
        from: u8,
        format: u8,
        save_partners: bool,
        actual_levels: bool,
    },
    /// To the host only: the answer to `to`'s challenge (`ANSWER_*`), with the sender's team
    /// when they accept.
    BattleAnswer {
        to: u8,
        answer: u8,
        dex: u32,
        team: Vec<BattleMember>,
    },
    /// From the host, to both players: live battle `battle` starts. The receiver is `side`
    /// (the challenger 0, the accepter 1) against `opponent` and their `team`; `seed` is the
    /// battle's first roll.
    BattleStart {
        battle: u16,
        format: u8,
        side: u8,
        opponent: u8,
        seed: u64,
        team: Vec<BattleMember>,
    },
    /// To the host only: the sender's choices for step `step` (each turn's choices or
    /// replacements, none when they have nothing to choose), and `check`, their battle's
    /// checksum before it.
    BattleChoice {
        battle: u16,
        step: u16,
        check: u32,
        picks: Vec<BattlePick>,
    },
    /// From the host, to both players once both chose: step `step` with both sides' choices
    /// (side, pick) and its rolls' seed.
    BattleStep {
        battle: u16,
        step: u16,
        seed: u64,
        picks: Vec<(u8, BattlePick)>,
    },
    /// To the host only: the sender is done with `battle` (`LEAVE_*`); battle 0 takes back
    /// their challenge, or their answer, before a battle starts.
    BattleLeave {
        battle: u16,
        reason: u8,
    },
    /// From the host: `battle` (0: one that never started) is over for the receiver
    /// (`END_*`), won by side `winner` (2: nobody).
    BattleEnd {
        battle: u16,
        reason: u8,
        winner: u8,
    },
    /// To the host only (net/wild_host.rs): whether the sender sees shared wild visitors and
    /// what they may do (`WILD_SEE`, `WILD_PICK`, `WILD_MACHINE`), and their ROM's visitor
    /// fingerprint (`net::wild_print`; 0 without `WILD_SEE`). On connecting and on change.
    WildHello {
        flags: u8,
        print: u32,
    },
    /// From the host, every 6 frames (unreliable), to the players in garden `area` who see its
    /// visitors: its `serial` (one more each list the host sends, so an older one arriving late
    /// is told apart), the garden's wild area (`section`, a map section id; none yet), the
    /// print its visitors are of, who picks them (`picker`), the owner ids whose Chao can learn
    /// from them now (`learners`, a bit per owner id 0..=15) and every visitor.
    WildList {
        area: u8,
        serial: u16,
        section: Option<u8>,
        print: u32,
        picker: Option<u8>,
        learners: u16,
        list: Vec<WildEntry>,
    },
    /// From the host, to garden `area`'s picker: pick `land` visitors for the grass and `pond`
    /// for the pond from `section` (none: choose the garden's area for this run). `fresh`: the
    /// garden just became shared, and the picker may hand over its own visitors where they are.
    WildAsk {
        area: u8,
        ask: u16,
        section: Option<u8>,
        land: u8,
        pond: u8,
        fresh: bool,
    },
    /// To the host only: the picker's answer to ask `ask`, from `section`.
    WildPick {
        area: u8,
        ask: u16,
        section: u8,
        list: Vec<WildPicked>,
    },
    /// To the host only: a partner of the sender's (from the link machine, or one waiting in
    /// this garden) comes to garden `area`, at `at` (none: where the host puts it). `key` is its
    /// key in the sender's store.
    WildBring {
        area: u8,
        key: u64,
        from_save: bool,
        creature: WildCreature,
        at: Option<([f32; 3], u16)>,
    },
    /// To the host only: the sender comes into garden `area` carrying a wild visitor from
    /// elsewhere, held at `offset`.
    WildCarry {
        area: u8,
        creature: WildCreature,
        offset: [f32; 3],
    },
    /// To the host only: the sender picks up visitor `id`.
    WildTake {
        id: u16,
    },
    /// To the host only, about 10 a second while the sender holds visitor `id`: in their hands
    /// at `v` (in their frame), or let go of and at world position `v` (`let_go`).
    WildHeld {
        id: u16,
        let_go: bool,
        v: [f32; 3],
    },
    /// To the host only: visitor `id` the sender let go of is on the ground at `at`.
    WildPut {
        id: u16,
        at: [f32; 3],
        angle: u16,
    },
    /// To the host only: a Chao (`owner`, `index`) took visitor `id` from the sender.
    WildAbsorbed {
        id: u16,
        owner: u8,
        index: u8,
    },
    /// To the host only: the sender battles visitor `id` (`WildStep`).
    WildBattle {
        id: u16,
        step: WildStep,
    },
    /// From the host, to the player who asked: whether their `kind` request was granted, for
    /// visitor `id` (0 when refused before it had one) and, for a bring, the partner's `key`.
    WildAnswer {
        kind: WildRequest,
        id: u16,
        key: u64,
        ok: bool,
    },
    /// From the host, to every session of a Chao's owner: the Chao whose `ChaoData::identity`
    /// is `identity` absorbed this visitor.
    WildTaught {
        identity: u64,
        origin: WildOrigin,
        creature: WildCreature,
    },
    /// From the host, to a partner's bringer: their partner `key` waiting in garden `garden` was
    /// absorbed (by anyone's Chao): it waits no more.
    WildUsed {
        garden: u8,
        key: u64,
    },
}

/// Most Chao on a live battle team.
pub const MAX_BATTLE_TEAM: usize = 6;
/// `BattleAnswer::answer`.
pub const ANSWER_DECLINE: u8 = 0;
pub const ANSWER_ACCEPT: u8 = 1;
/// Their game can't take a challenge now (in another battle, or away from the battle menu).
pub const ANSWER_BUSY: u8 = 2;
/// `BattlePick::kind`: use move `value`, the out-of-PP move, switch to member `value`, send
/// member `value` in for a fainted fighter.
pub const PICK_MOVE: u8 = 0;
pub const PICK_OUT_OF_PP: u8 = 1;
pub const PICK_SWITCH: u8 = 2;
pub const PICK_REPLACE: u8 = 3;
/// No target, no pass.
pub const PICK_NONE: u8 = 0xFF;
/// `BattleLeave::reason`.
pub const LEAVE_OVER: u8 = 0;
pub const LEAVE_GAVE_UP: u8 = 1;
pub const LEAVE_LEFT: u8 = 2;
/// `BattleEnd::reason`.
pub const END_BUSY: u8 = 0;
pub const END_DECLINED: u8 = 1;
pub const END_ROM_DIFFERS: u8 = 2;
pub const END_FORFEIT: u8 = 3;
pub const END_DISCONNECT: u8 = 4;
pub const END_TIMEOUT: u8 = 5;
pub const END_DESYNC: u8 = 6;
pub const END_WITHDRAWN: u8 = 7;
/// `BattleEnd::winner` for nobody.
pub const NO_WINNER: u8 = 2;

/// `WildHello::flags`: SharedVisitors is on and the ROM is read (the print is its); WildVisitors
/// is on too (the player may pick a garden's visitors); LinkMachine is on too.
pub const WILD_SEE: u8 = 1;
pub const WILD_PICK: u8 = 2;
pub const WILD_MACHINE: u8 = 4;
/// Most visitors in one `WildList`: the game's 10 animal entries less 2 left for its own.
pub const MAX_WILD_LIST: usize = 30;
/// Most visitors in one `WildPick`.
pub const MAX_WILD_PICK: usize = 5;
/// A held visitor's offset in its holder's frame is at most this along each axis.
pub const MAX_WILD_OFFSET: f32 = 10.0;

/// A wild visitor as ids from the ROM (every game names and draws it from its own).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WildCreature {
    /// 1..`SPECIES_COUNT`.
    pub species: u16,
    /// 1..=100.
    pub level: u8,
    /// The ability id (not the slot).
    pub ability: u8,
    pub shiny: bool,
    /// Its four moves (0: an empty slot; at least one, none twice).
    pub moves: [u16; 4],
}

/// Where a visitor of a `WildList` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WildPlace {
    /// On the ground (walking about, or let go of and falling): `WildEntry::v` is its world
    /// position.
    Roam,
    /// In the pond: `v` is its world position (the surface's height).
    Swim,
    /// In player `by`'s hands: `v` is the offset in their frame.
    Held { by: u8 },
    /// Player `by` battles it: `v` is where it stands.
    Battle { by: u8 },
}

/// One visitor of a `WildList`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WildEntry {
    /// Never 0.
    pub id: u16,
    pub creature: WildCreature,
    /// A partner, brought by this player (none: a wild one).
    pub bringer: Option<u8>,
    pub place: WildPlace,
    pub v: [f32; 3],
    /// Facing (BAMS).
    pub angle: u16,
}

/// Where a picked visitor comes (`WildPicked`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WildArrive {
    /// Where the host puts it on the grass, or in the pond.
    Land,
    Pond,
    /// Where it is now in the picker's game (a fresh ask only): on the grass, or swimming.
    LandAt {
        at: [f32; 3],
        angle: u16,
    },
    SwimAt {
        at: [f32; 3],
        angle: u16,
    },
}

/// One visitor of a `WildPick`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WildPicked {
    pub creature: WildCreature,
    pub arrive: WildArrive,
}

/// `WildBattle::step`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WildStep {
    /// Asks to battle it (answered by `WildAnswer`); others can't take it meanwhile.
    Reserve,
    /// The battle is won: it stays.
    Won,
    /// Lost, or given up: it runs away.
    Lost,
    /// Backed out before the fight: it stays.
    Cancelled,
}

/// `WildAnswer::kind`: what was asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WildRequest {
    Take,
    Carry,
    Bring,
    Battle,
}

/// `WildTaught::origin`: where the absorbed visitor came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WildOrigin {
    Wild,
    /// A partner from a GBA save, or another partner (a kept one, the gift).
    SavePartner,
    Partner,
}

/// A Chao on a live battle team: a copy of its record and its battle profile (ROM ids).
#[derive(Clone, Debug, PartialEq)]
pub struct BattleMember {
    pub chao: Box<ChaoData>,
    /// Its partner's species (0: none).
    pub species: u16,
    pub types: [u8; 2],
    /// 0: none.
    pub ability: u8,
    /// 0: an empty slot.
    pub moves: [u16; 4],
    /// Its battle level (1..=100).
    pub level: u8,
}

/// One fighter's choice in a live battle step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattlePick {
    /// Its place on the field (0, or 1 in doubles).
    pub pos: u8,
    /// `PICK_*`.
    pub kind: u8,
    /// The move index (0..4) or team member (0..6).
    pub value: u8,
    /// A move's target, side * 2 + pos, or `PICK_NONE`.
    pub target: u8,
    /// The team member a passing move hands on to, or `PICK_NONE`.
    pub pass: u8,
}

/// What one Chao has from its GBA link partner (`Body::Partners`): ids from the ROM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedPartner {
    /// The Chao's `ChaoData::identity`.
    pub identity: u64,
    /// The partner's species, types and ability (0: not recorded).
    pub species: u16,
    pub types: [u8; 2],
    pub ability: u8,
    /// The four moves the Chao battles with (0: an empty slot).
    pub moves: [u16; 4],
    /// Its battle level (1..=100).
    pub level: u8,
}

/// One Chao a player brings (`Body::Brought`).
#[derive(Clone, Debug, PartialEq)]
pub struct BroughtChao {
    /// Its garden (1..=3) and visitor row there (its `Body::Chao` index when they're in it).
    pub garden: u8,
    pub row: u8,
    /// Earlier save slots holding a Chao of the same `ChaoData::identity`.
    pub nth: u8,
    pub data: Box<ChaoData>,
}

/// A Chao the host holds (`Body::HeldChao`).
#[derive(Clone, Debug, PartialEq)]
pub struct HeldChao {
    /// The garden it's in (1..=3).
    pub area: u8,
    /// Its owner is offline: it sleeps (and only moves when carried).
    pub asleep: bool,
    /// Its owner's `ChaoLock` locks it now: nothing others do to it reaches their save.
    pub locked: bool,
    /// Its owner's player name.
    pub name: String,
    pub data: Box<ChaoData>,
}

/// When others may change a player's Chao (F9 "Others can change my Chao", `Body::Brought`).
/// A locked Chao is still seen, carried, thrown and petted by others, but nothing they do
/// reaches its owner's save (re/chao-garden-sync.md "Locked Chao").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChaoLock {
    #[default]
    Always,
    /// While its owner is anywhere in Chao World (`lock_place`; `AnyGarden` in config.ini
    /// reads as this too).
    ChaoWorld,
    /// While its owner is in the garden it's in.
    ThatGarden,
    Never,
}

impl ChaoLock {
    pub const ALL: [Self; 4] = [Self::Always, Self::ChaoWorld, Self::ThatGarden, Self::Never];

    /// Its wire value (its place in `ALL`).
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }

    /// F9 menu text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::ChaoWorld => "While I'm in Chao World",
            Self::ThatGarden => "While I'm in that garden",
            Self::Never => "Never",
        }
    }

    /// config.ini value (`OthersChangeChao`).
    pub fn ini(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::ChaoWorld => "ChaoWorld",
            Self::ThatGarden => "ThatGarden",
            Self::Never => "Never",
        }
    }

    /// A config.ini value (or the menu's text); anything else is `Always`. The old
    /// `AnyGarden` is `ChaoWorld`.
    pub fn parse(text: &str) -> Self {
        let plain = |t: &str| t.to_ascii_lowercase().replace([' ', '_', '\''], "");
        let t = plain(text.trim());
        if t == "anygarden" {
            return Self::ChaoWorld;
        }
        Self::ALL
            .into_iter()
            .find(|l| plain(l.ini()) == t || plain(l.label()) == t)
            .unwrap_or_default()
    }

    /// Whether a Chao living in garden `chao_area` is locked while its owner is at
    /// `owner_area` (`lock_place`: a garden 1..=3, `PLACE_CHAO_WORLD` elsewhere in Chao
    /// World; 0 out of it, or offline).
    pub fn locks(self, owner_area: u8, chao_area: u8) -> bool {
        match self {
            Self::Always => false,
            Self::ChaoWorld => owner_area == 0,
            Self::ThatGarden => owner_area == 0 || owner_area != chao_area,
            Self::Never => true,
        }
    }
}

/// Where an owner counts as being for their Chao's locks when in Chao World (level 90) but
/// not in a garden: the lobby (ChaoArea 0), the Kindergarten (6, Black Market and all), the
/// race and karate entrance (5), races (4), the stadium (9), karate (10).
pub const PLACE_CHAO_WORLD: u8 = 0x10;

/// Where a player at Chao World area `chao_area` (when `in_chao_world`) counts as being for
/// `ChaoLock::locks`: a garden (1..=3), `PLACE_CHAO_WORLD` anywhere else there, else 0.
pub fn lock_place(in_chao_world: bool, chao_area: i8) -> u8 {
    match chao_area {
        _ if !in_chao_world => 0,
        1..=3 => chao_area as u8,
        _ => PLACE_CHAO_WORLD,
    }
}

/// `Body::GardenUnlocks` bits: the Hero and Dark Gardens (their bits in the Chao save's flags).
pub const GARDEN_HERO: u8 = 0x10;
pub const GARDEN_DARK: u8 = 0x40;

/// At most this many enemies in one `EnemyStates`.
pub const MAX_ENEMY_STATES: usize = 32;

/// One enemy as its owner's game has it (`Body::EnemyStates`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnemyState {
    /// Its SET record.
    pub record: u16,
    pub position: [f32; 3],
    pub rotation: [i32; 3],
    /// How far the owner's player is from it (who's nearest owns it).
    pub distance: f32,
}

/// Bytes of an attack's Data1 sent (`Body::BossAttacks`): everything its display reads, up to
/// its collision pointer (+0x2C).
pub const BOSS_ATTACK_DATA: usize = 0x2C;
/// At most this many attacks in one `BossAttacks`.
pub const MAX_BOSS_ATTACKS: usize = 24;

/// One of a leading boss's attacks in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BossAttack {
    /// The leader's number for it while it flies.
    pub serial: u16,
    /// Which attack (boss_follow.rs `ATTACKS`).
    pub kind: u8,
    /// The player it was fired at.
    pub target: u8,
    /// Its Data1.
    pub data: [u8; BOSS_ATTACK_DATA],
}

/// A leading boss (`Body::BossState`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BossState {
    pub level: i16,
    pub mission: u8,
    /// Data1 +0 mode, +1 sub-mode, +3 counter.
    pub mode: u8,
    pub sub: u8,
    pub counter: u8,
    /// Data1 +4 flags, +6 timer.
    pub flags: u16,
    pub timer: u16,
    pub position: [f32; 3],
    pub rotation: [i32; 3],
    /// Its motion controller's motion now.
    pub motion: u16,
    /// The player it's after (turns to, aims and fires at).
    pub target: u8,
}

/// A short signature of a treasure hunt's 3 pieces (kind, id): claims name the hunt they're for.
pub fn hunt_signature(pieces: &[(u8, u8, [f32; 3]); 3]) -> u32 {
    pieces.iter().fold(0x811C_9DC5u32, |h, (k, i, _)| {
        (h ^ u32::from(*k)).wrapping_mul(0x0100_0193) ^ u32::from(*i)
    })
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
    /// Toys and instruments in its hands: per attach point (`garden::PROP_NODES`), 0 for none
    /// or 1 + an index into `garden::PROPS` (a fixed table: no game addresses on the wire).
    pub props: [u8; 3],
    /// The drum in front of it: its size 0..=255 (0 = none).
    pub drum: u8,
    /// In a cocoon: 0 none, else 1 + its kind (0..=2, `MayuObjectList`).
    pub cocoon: u8,
    /// The picture book in front of it: 0 none, else 1 + the book's mode (0..=3: opening,
    /// open, turning a page, page turning).
    pub book: u8,
    /// From its owner: their game runs it now (it's evolving, or its life is ending), so
    /// everyone follows the owner's, not the keeper's.
    pub owner_runs: bool,
}

/// A sound a Chao made (`Body::ChaoSounds`): player `owner`'s visiting Chao `index`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChaoSound {
    pub owner: u8,
    pub index: u8,
    /// `SOUND_AT` or `SOUND_HELD` (the game's two sound-at-a-place calls, `id` a sound id),
    /// or `SOUND_JINGLE` (`id` an index into `garden::JINGLES`).
    pub call: u8,
    pub id: u16,
    /// Where, from the Chao, in world axes.
    pub offset: [f32; 3],
    /// The call's own three arguments as the game passed them.
    pub args: [i16; 3],
}

pub const SOUND_AT: u8 = 0;
pub const SOUND_HELD: u8 = 1;
pub const SOUND_JINGLE: u8 = 2;
/// Sounds in one `ChaoSounds`.
pub const MAX_CHAO_SOUNDS: usize = 16;

/// Entries in `garden::PROPS` (`GardenChao::props` values go up to this).
pub const MAX_CHAO_PROP: u8 = 22;

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
    /// Held or being eaten by player `owner`'s visiting Chao `index` (`Body::Chao` index):
    /// offset from it, in its frame. The Chao is the item owner's own, or one their game runs
    /// as the garden's keeper (keeper.rs).
    Chao { owner: u8, index: u8 },
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
    /// In no one's save (fruit from the server's trees, or the server's own item): when its
    /// owner leaves the garden, the server keeps it (re/chao-garden-items.md 7).
    pub unsaved: bool,
}

impl GardenItem {
    /// The owner is using it (holding it, or their Chao is): nobody else may claim it.
    pub fn busy(&self) -> bool {
        self.place != ItemPlace::Ground
    }
}

/// A garden's tree spots and the bytes of each one's save record.
pub const TREE_SLOTS: usize = 7;
pub const TREE_RECORD: usize = 8;
/// `TreeAction` actions.
pub const TREE_PLANT: u8 = 0;
pub const TREE_SHAKE: u8 = 1;
/// A Chao watered it (the game's 0xAD sphere reached it): its life to 10000 (`trees.rs`).
pub const TREE_WATER: u8 = 2;

/// `ToyMode` toys, and the highest mode any of them has.
pub const TOY_TV: u8 = 0;
pub const TOY_RADIO: u8 = 1;
pub const TOY_BOX: u8 = 2;
pub const TOY_MAX_MODE: u8 = 12;

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
    /// EntityData1 action (what a skin's animation follows: re/skins.md "Garden actions").
    pub action: u8,
    /// `STATUS_*` bits: on the ground, holding something.
    pub status: u8,
}

/// One file the server shares in its `skins/` folder: `<skin id>/<path inside it>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinFile {
    pub path: String,
    pub size: u32,
    /// FNV-1a 64 of its bytes (`skin_hash`).
    pub hash: u64,
}

/// FNV-1a 64: a shared skin file's checksum (finds changed files; the files are checked
/// again when loaded).
pub fn skin_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3)
    })
}

/// A skin id: a folder name of lowercase letters, digits and `_`.
pub fn skin_id_ok(id: &str) -> bool {
    (1..=MAX_SKIN_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A shared file's path: `<skin id>/<parts>`, plain names only (nothing like `..`).
pub fn skin_path_ok(path: &str) -> bool {
    let mut parts = path.split('/');
    // Names any system can save (Windows refuses `?`, `:` and the like, and changes names
    // ending in a space or dot).
    let ok_part = |p: &str| {
        !p.is_empty()
            && !p.starts_with('.')
            && !p.ends_with([' ', '.'])
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" _-.()!',&+".contains(&b))
    };
    path.len() <= MAX_SKIN_PATH && parts.next().is_some_and(skin_id_ok) && {
        let rest: Vec<&str> = parts.collect();
        !rest.is_empty() && rest.iter().all(|p| ok_part(p))
    }
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
    pub const CHAT: u8 = 17;
    pub const RACE_STATUS: u8 = 18;
    pub const ROUTE: u8 = 19;
    pub const KART_PICK: u8 = 20;
    pub const KART_STATE: u8 = 21;
    pub const HUNT_SET: u8 = 22;
    pub const HUNT_CLAIM: u8 = 23;
    pub const HUNT_TAKEN: u8 = 24;
    pub const KEY_USED: u8 = 25;
    pub const SHOT: u8 = 26;
    pub const SWITCH: u8 = 27;
    pub const CHECKPOINT: u8 = 28;
    pub const BALL: u8 = 29;
    pub const BOSS_HP: u8 = 30;
    pub const SKIN_WORN: u8 = 31;
    pub const SKIN_LIST_GET: u8 = 32;
    pub const SKIN_LIST: u8 = 33;
    pub const SKIN_GET: u8 = 34;
    pub const SKIN_CHUNK: u8 = 35;
    pub const GARDEN_TREES: u8 = 36;
    pub const TREE_ACTION: u8 = 37;
    pub const GARDEN_EGGS: u8 = 38;
    pub const EGG_HELD: u8 = 39;
    pub const TOY_MODE: u8 = 40;
    pub const GARDEN_KEEPER: u8 = 41;
    pub const CHAO_SOUNDS: u8 = 42;
    pub const SERVER_GARDEN: u8 = 43;
    pub const SERVER_ITEMS: u8 = 44;
    pub const CARRIED_CHAO: u8 = 45;
    pub const SERVER_CHAO: u8 = 46;
    pub const GARDEN_LAND: u8 = 47;
    pub const CHAO_LIVE: u8 = 48;
    pub const CHAO_TOUCHED: u8 = 49;
    pub const STAGE_RINGS: u8 = 50;
    pub const LOCK_ON: u8 = 51;
    pub const BOSS_CLAIM: u8 = 52;
    pub const BOSS_STATE: u8 = 53;
    pub const BOSS_HIT: u8 = 54;
    // 43..=49 are the garden's.
    pub const BOSS_ATTACKS: u8 = 55;
    pub const ENEMY_STATES: u8 = 56;
    pub const UPGRADE_GOT: u8 = 57;
    pub const GARDEN_UNLOCKS: u8 = 58;
    pub const ITEM_STEER: u8 = 59;
    pub const BROUGHT: u8 = 60;
    pub const HELD_CHAO: u8 = 61;
    pub const SAVE_CHAO: u8 = 62;
    pub const CHAO_LOCKED: u8 = 63;
    pub const PARTNERS: u8 = 64;
    pub const BATTLE_CHALLENGE: u8 = 65;
    pub const BATTLE_OFFER: u8 = 66;
    pub const BATTLE_ANSWER: u8 = 67;
    pub const BATTLE_START: u8 = 68;
    pub const BATTLE_CHOICE: u8 = 69;
    pub const BATTLE_STEP: u8 = 70;
    pub const BATTLE_LEAVE: u8 = 71;
    pub const BATTLE_END: u8 = 72;
    pub const WILD_HELLO: u8 = 73;
    pub const WILD_LIST: u8 = 74;
    pub const WILD_ASK: u8 = 75;
    pub const WILD_PICK: u8 = 76;
    pub const WILD_BRING: u8 = 77;
    pub const WILD_CARRY: u8 = 78;
    pub const WILD_TAKE: u8 = 79;
    pub const WILD_HELD: u8 = 80;
    pub const WILD_PUT: u8 = 81;
    pub const WILD_ABSORBED: u8 = 82;
    pub const WILD_BATTLE: u8 = 83;
    pub const WILD_ANSWER: u8 = 84;
    pub const WILD_TAUGHT: u8 = 85;
    pub const WILD_USED: u8 = 86;
}

fn write_battle_team(w: &mut Writer, team: &[BattleMember]) {
    w.u8(team.len() as u8);
    for m in team {
        w.bytes(&m.chao.bytes)
            .u16(m.species)
            .u8(m.types[0])
            .u8(m.types[1])
            .u8(m.ability);
        for id in m.moves {
            w.u16(id);
        }
        w.u8(m.level);
    }
}

/// A live battle team of `min..=MAX_BATTLE_TEAM` Chao, their ids in the ROM's ranges.
fn read_battle_team(r: &mut Reader, min: usize) -> Result<Vec<BattleMember>> {
    use crate::gba::rom::{ABILITY_COUNT, MOVE_COUNT, SPECIES_COUNT, TYPE_COUNT};
    let count = usize::from(r.u8()?);
    if !(min..=MAX_BATTLE_TEAM).contains(&count) {
        return Err(WireError::BadValue("battle team"));
    }
    (0..count)
        .map(|_| {
            let chao = chao_data(r)?;
            let species = r.u16()?;
            let types = [r.u8()?, r.u8()?];
            let ability = r.u8()?;
            let moves = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
            let level = r.u8()?;
            if species >= SPECIES_COUNT
                || types.iter().any(|&t| t >= TYPE_COUNT)
                || ability >= ABILITY_COUNT
                || moves.iter().any(|&m| m >= MOVE_COUNT)
                || !(1..=100).contains(&level)
            {
                return Err(WireError::BadValue("battle team"));
            }
            Ok(BattleMember {
                chao,
                species,
                types,
                ability,
                moves,
                level,
            })
        })
        .collect()
}

/// A 0 or 1 byte.
fn wire_flag(r: &mut Reader, what: &'static str) -> Result<bool> {
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(WireError::BadValue(what)),
    }
}

fn battle_format(r: &mut Reader) -> Result<u8> {
    match r.u8()? {
        f @ 0..=1 => Ok(f),
        _ => Err(WireError::BadValue("battle format")),
    }
}

fn write_battle_pick(w: &mut Writer, p: &BattlePick) {
    w.u8(p.pos).u8(p.kind).u8(p.value).u8(p.target).u8(p.pass);
}

fn read_battle_pick(r: &mut Reader) -> Result<BattlePick> {
    let p = BattlePick {
        pos: r.u8()?,
        kind: r.u8()?,
        value: r.u8()?,
        target: r.u8()?,
        pass: r.u8()?,
    };
    let member = usize::from(p.value) < MAX_BATTLE_TEAM;
    let ok = p.pos < 2
        && match p.kind {
            PICK_MOVE => p.value < 4,
            PICK_OUT_OF_PP => p.value == 0,
            PICK_SWITCH | PICK_REPLACE => member,
            _ => false,
        }
        && (p.target == PICK_NONE || p.target < 4)
        && (p.pass == PICK_NONE || usize::from(p.pass) < MAX_BATTLE_TEAM);
    if ok {
        Ok(p)
    } else {
        Err(WireError::BadValue("battle pick"))
    }
}

/// `GardenChao` lists as `GardenChao` and `ServerChao` carry them.
fn write_garden_chao(w: &mut Writer, list: &[GardenChao]) {
    w.u8(list.len() as u8);
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
        for p in c.props {
            w.u8(p);
        }
        w.u8(c.drum)
            .u8(c.cocoon)
            .u8(c.book)
            .u8(u8::from(c.owner_runs));
    }
}

fn read_garden_chao(r: &mut Reader) -> Result<Vec<GardenChao>> {
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
        let props = [r.u8()?, r.u8()?, r.u8()?];
        if props.iter().any(|&p| p > MAX_CHAO_PROP) {
            return Err(WireError::BadValue("garden chao prop"));
        }
        let (drum, cocoon, book, owner_runs) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
        if cocoon > 3 {
            return Err(WireError::BadValue("garden chao cocoon"));
        }
        if book > 4 || owner_runs > 1 {
            return Err(WireError::BadValue("garden chao book"));
        }
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
            props,
            drum,
            cocoon,
            book,
            owner_runs: owner_runs == 1,
        });
    }
    Ok(list)
}

/// Garden items as `GardenItems` and `ServerItems` carry them.
fn write_items(w: &mut Writer, items: &[GardenItem]) {
    w.u8(items.len() as u8);
    for i in items {
        let (place, chao) = match i.place {
            ItemPlace::Ground => (0, 0),
            ItemPlace::Player => (1, 0),
            // Both in one byte: index (< 4) low, owner (< 16) high.
            ItemPlace::Chao { owner, index } => (2, owner << 4 | index),
            ItemPlace::Busy => (3, 0),
        };
        w.u8(i.id.origin)
            .u16(i.id.serial)
            .u8(i.category)
            .u8(i.kind)
            .u8(i.bites)
            .u8(place | if i.unsaved { UNSAVED } else { 0 })
            .u8(chao);
        for v in i.position {
            w.f32(v);
        }
    }
}

/// The place byte's flag for `GardenItem::unsaved`.
const UNSAVED: u8 = 0x80;

fn read_items(r: &mut Reader, area: u8) -> Result<Vec<GardenItem>> {
    let count = usize::from(r.u8()?);
    if !(1..=3).contains(&area) || count > MAX_GARDEN_ITEMS {
        return Err(WireError::BadValue("garden items"));
    }
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let id = item_id(r)?;
        let (category, kind, bites) = (r.u8()?, r.u8()?, r.u8()?);
        let (place, chao) = (r.u8()?, r.u8()?);
        let unsaved = place & UNSAVED != 0;
        let place = match (place & !UNSAVED, chao) {
            (0, _) => ItemPlace::Ground,
            (1, _) => ItemPlace::Player,
            (2, n) if usize::from(n & 0xF) < MAX_CARRIED_CHAO && chao_owner_ok(n >> 4) => {
                ItemPlace::Chao {
                    owner: n >> 4,
                    index: n & 0xF,
                }
            }
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
            unsaved,
        });
    }
    Ok(items)
}

fn item_id(r: &mut Reader) -> Result<ItemId> {
    Ok(ItemId {
        origin: player_id(r)?,
        serial: r.u16()?,
    })
}

/// A `Game` message's sender: a player id, or `SERVER` (the host's own Chao and items).
fn sender(r: &mut Reader) -> Result<u8> {
    let id = r.u8()?;
    if (id as usize) < MAX_PLAYERS || id == SERVER {
        Ok(id)
    } else {
        Err(WireError::BadValue("player id"))
    }
}

/// The sender id the host's own messages carry (no player has it: `garden::SERVER`).
pub const SERVER: u8 = 0xFF;

fn player_id(r: &mut Reader) -> Result<u8> {
    let id = r.u8()?;
    if (id as usize) < MAX_PLAYERS {
        Ok(id)
    } else {
        Err(WireError::BadValue("player id"))
    }
}

/// A Chao's owner: a player id, or one the host holds Chao under (`HELD_OWNER..`).
fn chao_owner(r: &mut Reader) -> Result<u8> {
    let id = r.u8()?;
    if chao_owner_ok(id) {
        Ok(id)
    } else {
        Err(WireError::BadValue("chao owner"))
    }
}

fn chao_owner_ok(id: u8) -> bool {
    usize::from(id) < MAX_PLAYERS || (HELD_OWNER..=MAX_CHAO_OWNER).contains(&id)
}

/// A Chao record off the wire: whole and in range (`ChaoData::validate`).
fn chao_data(r: &mut Reader) -> Result<Box<ChaoData>> {
    let chao = ChaoData::from_bytes(r.take(CHAO_SIZE)?).ok_or(WireError::Truncated)?;
    chao.validate()
        .map_err(|_| WireError::BadValue("chao data"))?;
    Ok(Box::new(chao))
}

fn name(r: &mut Reader) -> Result<String> {
    let s = r.str(MAX_NAME)?;
    if s.is_empty() {
        Err(WireError::BadString)
    } else {
        Ok(s)
    }
}

/// No map section yet (`WildList`, `WildAsk`).
const NO_SECTION: u8 = 0xFF;
/// No player (`WildList` picker, an entry's bringer or holder).
const NO_PLAYER: u8 = 0xFF;

fn write_creature(w: &mut Writer, c: &WildCreature) {
    w.u16(c.species)
        .u8(c.level)
        .u8(c.ability)
        .u8(u8::from(c.shiny));
    for m in c.moves {
        w.u16(m);
    }
}

/// A visitor off the wire: a species, level, ability and moves the ROM has, at least one move
/// and none twice.
fn read_creature(r: &mut Reader) -> Result<WildCreature> {
    use crate::gba::rom::{ABILITY_COUNT, MOVE_COUNT, SPECIES_COUNT};
    let c = WildCreature {
        species: r.u16()?,
        level: r.u8()?,
        ability: r.u8()?,
        shiny: wire_flag(r, "wild shiny")?,
        moves: [r.u16()?, r.u16()?, r.u16()?, r.u16()?],
    };
    let twice = (0..4).any(|i| c.moves[i] != 0 && c.moves[..i].contains(&c.moves[i]));
    if !(1..SPECIES_COUNT).contains(&c.species)
        || !(1..=100).contains(&c.level)
        || c.ability >= ABILITY_COUNT
        || c.moves.iter().any(|&m| m >= MOVE_COUNT)
        || c.moves == [0; 4]
        || twice
    {
        return Err(WireError::BadValue("wild creature"));
    }
    Ok(c)
}

fn wild_area(r: &mut Reader) -> Result<u8> {
    let area = r.u8()?;
    if (1..=3).contains(&area) {
        Ok(area)
    } else {
        Err(WireError::BadValue("wild area"))
    }
}

fn wild_id(r: &mut Reader) -> Result<u16> {
    match r.u16()? {
        0 => Err(WireError::BadValue("wild id")),
        id => Ok(id),
    }
}

fn write_v3(w: &mut Writer, v: [f32; 3]) {
    for x in v {
        w.f32(x);
    }
}

fn read_v3(r: &mut Reader, limit: f32) -> Result<[f32; 3]> {
    Ok([r.f32(limit)?, r.f32(limit)?, r.f32(limit)?])
}

fn write_section(w: &mut Writer, section: Option<u8>) {
    w.u8(section.unwrap_or(NO_SECTION));
}

fn read_section(r: &mut Reader) -> Result<Option<u8>> {
    Ok(Some(r.u8()?).filter(|&s| s != NO_SECTION))
}

fn write_wild_entry(w: &mut Writer, e: &WildEntry) {
    w.u16(e.id);
    write_creature(w, &e.creature);
    let (place, by) = match e.place {
        WildPlace::Roam => (0, NO_PLAYER),
        WildPlace::Swim => (1, NO_PLAYER),
        WildPlace::Held { by } => (2, by),
        WildPlace::Battle { by } => (3, by),
    };
    w.u8(u8::from(e.bringer.is_some()))
        .u8(e.bringer.unwrap_or(NO_PLAYER))
        .u8(place)
        .u8(by);
    write_v3(w, e.v);
    w.u16(e.angle);
}

fn read_wild_entry(r: &mut Reader) -> Result<WildEntry> {
    let id = wild_id(r)?;
    let creature = read_creature(r)?;
    let bad = || WireError::BadValue("wild entry");
    let player = |by: u8| usize::from(by) < MAX_PLAYERS;
    let bringer = match (r.u8()?, r.u8()?) {
        (0, NO_PLAYER) => None,
        (1, id) if player(id) => Some(id),
        _ => return Err(bad()),
    };
    let place = match (r.u8()?, r.u8()?) {
        (0, NO_PLAYER) => WildPlace::Roam,
        (1, NO_PLAYER) => WildPlace::Swim,
        (2, by) if player(by) => WildPlace::Held { by },
        (3, by) if player(by) => WildPlace::Battle { by },
        _ => return Err(bad()),
    };
    let limit = match place {
        WildPlace::Held { .. } => MAX_WILD_OFFSET,
        _ => MAX_COORD,
    };
    Ok(WildEntry {
        id,
        creature,
        bringer,
        place,
        v: read_v3(r, limit)?,
        angle: r.u16()?,
    })
}

fn write_wild_picked(w: &mut Writer, p: &WildPicked) {
    write_creature(w, &p.creature);
    let (place, at, angle) = match p.arrive {
        WildArrive::Land => (0, [0.0; 3], 0),
        WildArrive::Pond => (1, [0.0; 3], 0),
        WildArrive::LandAt { at, angle } => (2, at, angle),
        WildArrive::SwimAt { at, angle } => (3, at, angle),
    };
    w.u8(place);
    write_v3(w, at);
    w.u16(angle);
}

fn read_wild_picked(r: &mut Reader) -> Result<WildPicked> {
    let creature = read_creature(r)?;
    let place = r.u8()?;
    let (at, angle) = (read_v3(r, MAX_COORD)?, r.u16()?);
    let arrive = match place {
        0 => WildArrive::Land,
        1 => WildArrive::Pond,
        2 => WildArrive::LandAt { at, angle },
        3 => WildArrive::SwimAt { at, angle },
        _ => return Err(WireError::BadValue("wild place")),
    };
    Ok(WildPicked { creature, arrive })
}

fn read_u64(r: &mut Reader) -> Result<u64> {
    Ok(u64::from_le_bytes(r.take(8)?.try_into().unwrap()))
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
                key,
            } => {
                w.u8(tag::HELLO)
                    .u16(*protocol)
                    .str(mod_version)
                    .bytes(game_build)
                    .str(name)
                    .bytes(key);
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
                // Older versions send none: they're turned away by their protocol number.
                key: match r.rest() {
                    [] => [0; 16],
                    key => key
                        .try_into()
                        .map_err(|_| WireError::BadValue("player key"))?,
                },
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
                from: sender(&mut r)?,
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
                    .u32(s.upgrades)
                    .u8(s.action)
                    .u8(s.status);
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
            Self::GardenKeeper { area, term } => {
                w.u8(tag::GARDEN_KEEPER).u8(*area).u32(*term);
            }
            Self::ChaoSounds { sounds } => {
                w.u8(tag::CHAO_SOUNDS).u8(sounds.len() as u8);
                for s in sounds {
                    w.u8(s.owner).u8(s.index).u8(s.call).u16(s.id);
                    for v in s.offset {
                        w.f32(v);
                    }
                    for a in s.args {
                        w.i16(a);
                    }
                }
            }
            Self::GardenChao { owner, list } => {
                w.u8(tag::GARDEN_CHAO).u8(*owner);
                write_garden_chao(w, list);
            }
            Self::ServerChao { area, owner, list } => {
                w.u8(tag::SERVER_CHAO).u8(*area).u8(*owner);
                write_garden_chao(w, list);
            }
            Self::ItemSteer { area, item, gone } => {
                w.u8(tag::ITEM_STEER).u8(*area).u8(u8::from(*gone));
                write_items(w, std::slice::from_ref(item));
            }
            Self::ChaoTouched {
                owner,
                index,
                position,
                rotation_y,
                motion,
                mood,
            } => {
                w.u8(tag::CHAO_TOUCHED).u8(*owner).u8(*index);
                for v in position {
                    w.f32(*v);
                }
                w.i32(*rotation_y).u16(*motion).bytes(mood);
            }
            Self::ChaoLive {
                owner,
                index,
                bytes,
            } => {
                w.u8(tag::CHAO_LIVE).u8(*owner).u8(*index).bytes(bytes);
            }
            Self::GardenLand {
                garden,
                fingerprint,
                part,
                parts,
                bytes,
            } => {
                w.u8(tag::GARDEN_LAND)
                    .u8(*garden)
                    .u32(*fingerprint)
                    .u8(*part)
                    .u8(*parts)
                    .u16(bytes.len() as u16)
                    .bytes(bytes);
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
                w.u8(tag::GARDEN_ITEMS).u8(*area);
                write_items(w, items);
            }
            Self::ServerItems { area, items } => {
                w.u8(tag::SERVER_ITEMS).u8(*area);
                write_items(w, items);
            }
            Self::CarriedChao { chao, offset } => {
                w.u8(tag::CARRIED_CHAO);
                for v in offset {
                    w.f32(*v);
                }
                match chao {
                    Some(chao) => w.u8(1).bytes(&chao.bytes),
                    None => w.u8(0),
                };
            }
            Self::ServerGarden { area, trees, toys } => {
                w.u8(tag::SERVER_GARDEN).u8(*area);
                for b in trees.iter().flatten() {
                    w.u8(*b);
                }
                for (mode, age) in toys {
                    w.u8(*mode).u16(*age);
                }
            }
            Self::Chat { text } => {
                w.u8(tag::CHAT).str(text);
            }
            Self::RaceStatus {
                level,
                mission,
                progress,
                finish,
            } => {
                w.u8(tag::RACE_STATUS)
                    .i16(*level)
                    .u8(*mission)
                    .u16(*progress);
                match finish {
                    Some((race, game)) => w.u8(1).u32(*race).u32(*game),
                    None => w.u8(0),
                };
            }
            Self::HuntSet {
                level,
                mission,
                pieces,
            } => {
                w.u8(tag::HUNT_SET).i16(*level).u8(*mission);
                for (kind, id, pos) in pieces {
                    w.u8(*kind).u8(*id);
                    for v in pos {
                        w.f32(*v);
                    }
                }
            }
            Self::HuntClaim {
                level,
                mission,
                sig,
                slot,
            } => {
                w.u8(tag::HUNT_CLAIM)
                    .i16(*level)
                    .u8(*mission)
                    .u32(*sig)
                    .u8(*slot);
            }
            Self::HuntTaken {
                level,
                mission,
                sig,
                slot,
                player,
            } => {
                w.u8(tag::HUNT_TAKEN)
                    .i16(*level)
                    .u8(*mission)
                    .u32(*sig)
                    .u8(*slot)
                    .u8(*player);
            }
            Self::KartPick { course } => {
                w.u8(tag::KART_PICK).u8(*course);
            }
            Self::KartState {
                position,
                rotation,
                yaw,
                wheel,
                kind,
                accel,
                tilt,
                frame,
            } => {
                w.u8(tag::KART_STATE);
                for v in position {
                    w.f32(*v);
                }
                for a in rotation {
                    w.i32(*a);
                }
                w.i32(*yaw).i32(*wheel).u8(*kind).u8(u8::from(*accel));
                for a in tilt {
                    w.i32(*a);
                }
                w.u16(*frame);
            }
            Self::Route {
                level,
                mission,
                points,
            } => {
                w.u8(tag::ROUTE)
                    .i16(*level)
                    .u8(*mission)
                    .u16(points.len() as u16);
                for p in points {
                    for v in p {
                        w.f32(*v);
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
            Self::KeyUsed {
                level,
                mission,
                key,
            } => {
                w.u8(tag::KEY_USED).i16(*level).u8(*mission).u8(*key);
            }
            Self::Shot {
                level,
                mission,
                kind,
                position,
                direction,
            } => {
                w.u8(tag::SHOT).i16(*level).u8(*mission).u8(*kind);
                for v in position.iter().chain(direction) {
                    w.f32(*v);
                }
            }
            Self::Switch {
                level,
                mission,
                id,
                on,
            } => {
                w.u8(tag::SWITCH)
                    .i16(*level)
                    .u8(*mission)
                    .u8(*id)
                    .u8(u8::from(*on));
            }
            Self::Checkpoint {
                level,
                mission,
                record,
                position,
                rotation,
            } => {
                w.u8(tag::CHECKPOINT).i16(*level).u8(*mission).u16(*record);
                for v in position {
                    w.f32(*v);
                }
                for a in rotation {
                    w.i32(*a);
                }
            }
            Self::Ball {
                area,
                claim,
                position,
                velocity,
                flags,
            } => {
                w.u8(tag::BALL).u8(*area).u16(*claim);
                for v in position.iter().chain(velocity) {
                    w.f32(*v);
                }
                w.u8(*flags);
            }
            Self::GardenTrees {
                area,
                trees,
                thirsty,
            } => {
                w.u8(tag::GARDEN_TREES).u8(*area);
                for b in trees.iter().flatten() {
                    w.u8(*b);
                }
                w.u8(*thirsty);
            }
            Self::TreeAction {
                area,
                slot,
                action,
                kind,
            } => {
                w.u8(tag::TREE_ACTION)
                    .u8(*area)
                    .u8(*slot)
                    .u8(*action)
                    .u8(*kind);
            }
            Self::GardenEggs { area, eggs } => {
                w.u8(tag::GARDEN_EGGS).u8(*area).u8(eggs.len() as u8);
                for (index, position, held) in eggs {
                    w.u8(*index);
                    for v in position {
                        w.f32(*v);
                    }
                    w.u8(u8::from(*held));
                }
            }
            Self::ToyMode { area, toy, mode } => {
                w.u8(tag::TOY_MODE).u8(*area).u8(*toy).u8(*mode);
            }
            Self::EggHeld {
                owner,
                index,
                held,
                position,
            } => {
                w.u8(tag::EGG_HELD)
                    .u8(*owner)
                    .u8(*index)
                    .u8(u8::from(*held));
                for v in position {
                    w.f32(*v);
                }
            }
            Self::BossHp {
                level,
                mission,
                hp,
                full,
            } => {
                w.u8(tag::BOSS_HP)
                    .i16(*level)
                    .u8(*mission)
                    .f32(*hp)
                    .f32(*full);
            }
            Self::StageRings {
                level,
                mission,
                rings,
            } => {
                w.u8(tag::STAGE_RINGS)
                    .i16(*level)
                    .u8(*mission)
                    .u16(rings.len() as u16);
                for (record, ring) in rings {
                    w.u16(*record).u8(*ring);
                }
            }
            Self::LockOn {
                level,
                mission,
                gun,
                locked,
                fired,
            } => {
                w.u8(tag::LOCK_ON).i16(*level).u8(*mission);
                for v in gun {
                    w.f32(*v);
                }
                for list in [locked, fired] {
                    w.u8(list.len() as u8);
                    for (record, at) in list {
                        w.u16(*record);
                        for v in at {
                            w.f32(*v);
                        }
                    }
                }
            }
            Self::BossClaim {
                level,
                mission,
                term,
            } => {
                w.u8(tag::BOSS_CLAIM).i16(*level).u8(*mission).u32(*term);
            }
            Self::BossState(b) => {
                w.u8(tag::BOSS_STATE)
                    .i16(b.level)
                    .u8(b.mission)
                    .u8(b.mode)
                    .u8(b.sub)
                    .u8(b.counter)
                    .u16(b.flags)
                    .u16(b.timer);
                for v in b.position {
                    w.f32(v);
                }
                for v in b.rotation {
                    w.i32(v);
                }
                w.u16(b.motion).u8(b.target);
            }
            Self::BossHit { level, mission } => {
                w.u8(tag::BOSS_HIT).i16(*level).u8(*mission);
            }
            Self::BossAttacks {
                level,
                mission,
                attacks,
            } => {
                w.u8(tag::BOSS_ATTACKS)
                    .i16(*level)
                    .u8(*mission)
                    .u8(attacks.len() as u8);
                for a in attacks {
                    w.u16(a.serial).u8(a.kind).u8(a.target).bytes(&a.data);
                }
            }
            Self::GardenUnlocks { gardens } => {
                w.u8(tag::GARDEN_UNLOCKS).u8(*gardens);
            }
            Self::Brought { awake, lock, list } => {
                w.u8(tag::BROUGHT)
                    .u8(u8::from(*awake))
                    .u8(lock.to_u8())
                    .u8(list.len() as u8);
                for c in list {
                    w.u8(c.garden).u8(c.row).u8(c.nth).bytes(&c.data.bytes);
                }
            }
            Self::HeldChao { owner, index, chao } => {
                w.u8(tag::HELD_CHAO)
                    .u8(*owner)
                    .u8(*index)
                    .u8(u8::from(chao.is_some()));
                if let Some(c) = chao {
                    w.u8(c.area)
                        .u8(u8::from(c.asleep))
                        .u8(u8::from(c.locked))
                        .str(&c.name)
                        .bytes(&c.data.bytes);
                }
            }
            Self::SaveChao {
                identity,
                nth,
                spans,
            } => {
                w.u8(tag::SAVE_CHAO)
                    .bytes(&identity.to_le_bytes())
                    .u8(*nth)
                    .u8(spans.len() as u8);
                for (at, bytes) in spans {
                    w.u16(*at).u8(bytes.len() as u8).bytes(bytes);
                }
            }
            Self::ChaoLocked { chao } => {
                w.u8(tag::CHAO_LOCKED).u8(chao.len() as u8);
                for (owner, index) in chao {
                    w.u8(*owner).u8(*index);
                }
            }
            Self::Partners { list } => {
                w.u8(tag::PARTNERS).u8(list.len() as u8);
                for p in list {
                    w.bytes(&p.identity.to_le_bytes())
                        .u16(p.species)
                        .u8(p.types[0])
                        .u8(p.types[1])
                        .u8(p.ability);
                    for m in p.moves {
                        w.u16(m);
                    }
                    w.u8(p.level);
                }
            }
            Self::BattleChallenge {
                to,
                format,
                save_partners,
                actual_levels,
                dex,
                team,
            } => {
                w.u8(tag::BATTLE_CHALLENGE)
                    .u8(*to)
                    .u8(*format)
                    .u8(u8::from(*save_partners))
                    .u8(u8::from(*actual_levels))
                    .u32(*dex);
                write_battle_team(w, team);
            }
            Self::BattleOffer {
                from,
                format,
                save_partners,
                actual_levels,
            } => {
                w.u8(tag::BATTLE_OFFER)
                    .u8(*from)
                    .u8(*format)
                    .u8(u8::from(*save_partners))
                    .u8(u8::from(*actual_levels));
            }
            Self::BattleAnswer {
                to,
                answer,
                dex,
                team,
            } => {
                w.u8(tag::BATTLE_ANSWER).u8(*to).u8(*answer).u32(*dex);
                write_battle_team(w, team);
            }
            Self::BattleStart {
                battle,
                format,
                side,
                opponent,
                seed,
                team,
            } => {
                w.u8(tag::BATTLE_START)
                    .u16(*battle)
                    .u8(*format)
                    .u8(*side)
                    .u8(*opponent)
                    .bytes(&seed.to_le_bytes());
                write_battle_team(w, team);
            }
            Self::BattleChoice {
                battle,
                step,
                check,
                picks,
            } => {
                w.u8(tag::BATTLE_CHOICE)
                    .u16(*battle)
                    .u16(*step)
                    .u32(*check)
                    .u8(picks.len() as u8);
                for p in picks {
                    write_battle_pick(w, p);
                }
            }
            Self::BattleStep {
                battle,
                step,
                seed,
                picks,
            } => {
                w.u8(tag::BATTLE_STEP)
                    .u16(*battle)
                    .u16(*step)
                    .bytes(&seed.to_le_bytes())
                    .u8(picks.len() as u8);
                for (side, p) in picks {
                    w.u8(*side);
                    write_battle_pick(w, p);
                }
            }
            Self::BattleLeave { battle, reason } => {
                w.u8(tag::BATTLE_LEAVE).u16(*battle).u8(*reason);
            }
            Self::BattleEnd {
                battle,
                reason,
                winner,
            } => {
                w.u8(tag::BATTLE_END).u16(*battle).u8(*reason).u8(*winner);
            }
            Self::WildHello { flags, print } => {
                w.u8(tag::WILD_HELLO).u8(*flags).u32(*print);
            }
            Self::WildList {
                area,
                serial,
                section,
                print,
                picker,
                learners,
                list,
            } => {
                w.u8(tag::WILD_LIST).u8(*area).u16(*serial);
                write_section(w, *section);
                w.u32(*print)
                    .u8(picker.unwrap_or(NO_PLAYER))
                    .u16(*learners)
                    .u8(list.len() as u8);
                for e in list {
                    write_wild_entry(w, e);
                }
            }
            Self::WildAsk {
                area,
                ask,
                section,
                land,
                pond,
                fresh,
            } => {
                w.u8(tag::WILD_ASK).u8(*area).u16(*ask);
                write_section(w, *section);
                w.u8(*land).u8(*pond).u8(u8::from(*fresh));
            }
            Self::WildPick {
                area,
                ask,
                section,
                list,
            } => {
                w.u8(tag::WILD_PICK)
                    .u8(*area)
                    .u16(*ask)
                    .u8(*section)
                    .u8(list.len() as u8);
                for p in list {
                    write_wild_picked(w, p);
                }
            }
            Self::WildBring {
                area,
                key,
                from_save,
                creature,
                at,
            } => {
                w.u8(tag::WILD_BRING)
                    .u8(*area)
                    .bytes(&key.to_le_bytes())
                    .u8(u8::from(*from_save));
                write_creature(w, creature);
                let (place, v, angle) = at.map_or((0, [0.0; 3], 0), |(v, a)| (2, v, a));
                w.u8(place);
                write_v3(w, v);
                w.u16(angle);
            }
            Self::WildCarry {
                area,
                creature,
                offset,
            } => {
                w.u8(tag::WILD_CARRY).u8(*area);
                write_creature(w, creature);
                write_v3(w, *offset);
            }
            Self::WildTake { id } => {
                w.u8(tag::WILD_TAKE).u16(*id);
            }
            Self::WildHeld { id, let_go, v } => {
                w.u8(tag::WILD_HELD).u16(*id).u8(u8::from(*let_go));
                write_v3(w, *v);
            }
            Self::WildPut { id, at, angle } => {
                w.u8(tag::WILD_PUT).u16(*id);
                write_v3(w, *at);
                w.u16(*angle);
            }
            Self::WildAbsorbed { id, owner, index } => {
                w.u8(tag::WILD_ABSORBED).u16(*id).u8(*owner).u8(*index);
            }
            Self::WildBattle { id, step } => {
                let step = match step {
                    WildStep::Reserve => 0,
                    WildStep::Won => 1,
                    WildStep::Lost => 2,
                    WildStep::Cancelled => 3,
                };
                w.u8(tag::WILD_BATTLE).u16(*id).u8(step);
            }
            Self::WildAnswer { kind, id, key, ok } => {
                let kind = match kind {
                    WildRequest::Take => 0,
                    WildRequest::Carry => 1,
                    WildRequest::Bring => 2,
                    WildRequest::Battle => 3,
                };
                w.u8(tag::WILD_ANSWER)
                    .u8(kind)
                    .u16(*id)
                    .bytes(&key.to_le_bytes())
                    .u8(u8::from(*ok));
            }
            Self::WildTaught {
                identity,
                origin,
                creature,
            } => {
                let origin = match origin {
                    WildOrigin::Wild => 0,
                    WildOrigin::SavePartner => 1,
                    WildOrigin::Partner => 2,
                };
                w.u8(tag::WILD_TAUGHT)
                    .bytes(&identity.to_le_bytes())
                    .u8(origin);
                write_creature(w, creature);
            }
            Self::WildUsed { garden, key } => {
                w.u8(tag::WILD_USED).u8(*garden).bytes(&key.to_le_bytes());
            }
            Self::UpgradeGot {
                level,
                mission,
                index,
                character,
                mask,
            } => {
                w.u8(tag::UPGRADE_GOT)
                    .i16(*level)
                    .u8(*mission)
                    .u8(*index)
                    .u8(*character)
                    .u32(*mask);
            }
            Self::EnemyStates {
                level,
                mission,
                enemies,
            } => {
                w.u8(tag::ENEMY_STATES)
                    .i16(*level)
                    .u8(*mission)
                    .u8(enemies.len() as u8);
                for e in enemies {
                    w.u16(e.record);
                    for v in e.position {
                        w.f32(v);
                    }
                    for v in e.rotation {
                        w.i32(v);
                    }
                    w.f32(e.distance);
                }
            }
            Self::SkinWorn { skin } => {
                w.u8(tag::SKIN_WORN).str(skin);
            }
            Self::SkinListGet { page } => {
                w.u8(tag::SKIN_LIST_GET).u16(*page);
            }
            Self::SkinList { page, pages, files } => {
                w.u8(tag::SKIN_LIST)
                    .u16(*page)
                    .u16(*pages)
                    .u8(files.len() as u8);
                for f in files {
                    w.str(&f.path).u32(f.size).bytes(&f.hash.to_le_bytes());
                }
            }
            Self::SkinGet { file, offset } => {
                w.u8(tag::SKIN_GET).u16(*file).u32(*offset);
            }
            Self::SkinChunk { file, offset, data } => {
                w.u8(tag::SKIN_CHUNK)
                    .u16(*file)
                    .u32(*offset)
                    .u16(data.len() as u16)
                    .bytes(data);
            }
        }
    }

    fn decode(r: &mut Reader) -> Result<Self> {
        let t = r.u8()?;
        Ok(match t {
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
                action: r.u8()?,
                status: r.u8()?,
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
            tag::GARDEN_KEEPER => {
                let (area, term) = (r.u8()?, r.u32()?);
                if !(1..=3).contains(&area) {
                    return Err(WireError::BadValue("garden keeper"));
                }
                Self::GardenKeeper { area, term }
            }
            tag::CHAO_SOUNDS => {
                let count = usize::from(r.u8()?);
                if count > MAX_CHAO_SOUNDS {
                    return Err(WireError::BadValue("chao sounds"));
                }
                let mut sounds = Vec::with_capacity(count);
                for _ in 0..count {
                    let (owner, index, call, id) = (chao_owner(r)?, r.u8()?, r.u8()?, r.u16()?);
                    if usize::from(index) >= MAX_CARRIED_CHAO || call > SOUND_JINGLE {
                        return Err(WireError::BadValue("chao sound"));
                    }
                    sounds.push(ChaoSound {
                        owner,
                        index,
                        call,
                        id,
                        offset: [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                        args: [r.i16()?, r.i16()?, r.i16()?],
                    });
                }
                Self::ChaoSounds { sounds }
            }
            tag::GARDEN_CHAO => {
                let owner = player_id(r)?;
                let list = read_garden_chao(r)?;
                Self::GardenChao { owner, list }
            }
            tag::SERVER_CHAO => {
                let area = r.u8()?;
                if !(1..=3).contains(&area) {
                    return Err(WireError::BadValue("server chao area"));
                }
                let owner = chao_owner(r)?;
                let list = read_garden_chao(r)?;
                Self::ServerChao { area, owner, list }
            }
            tag::ITEM_STEER => {
                let (area, gone) = (r.u8()?, r.u8()?);
                let mut items = read_items(r, area)?;
                if items.len() != 1 || gone > 1 {
                    return Err(WireError::BadValue("item steer"));
                }
                Self::ItemSteer {
                    area,
                    item: items.remove(0),
                    gone: gone == 1,
                }
            }
            tag::CHAO_TOUCHED => {
                let (owner, index) = (chao_owner(r)?, r.u8()?);
                if usize::from(index) >= MAX_CARRIED_CHAO {
                    return Err(WireError::BadValue("chao touched"));
                }
                Self::ChaoTouched {
                    owner,
                    index,
                    position: [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                    rotation_y: r.i32()?,
                    motion: r.u16()?,
                    mood: r.take(CHAO_MOOD.1)?.to_vec(),
                }
            }
            tag::CHAO_LIVE => {
                let (owner, index) = (chao_owner(r)?, r.u8()?);
                if usize::from(index) >= MAX_CARRIED_CHAO {
                    return Err(WireError::BadValue("chao live"));
                }
                let bytes = r.take(CHAO_LIVE_LEN)?.to_vec();
                Self::ChaoLive {
                    owner,
                    index,
                    bytes,
                }
            }
            tag::GARDEN_LAND => {
                let (garden, fingerprint, part, parts) = (r.u8()?, r.u32()?, r.u8()?, r.u8()?);
                let len = usize::from(r.u16()?);
                // 0..=2 the gardens' ground, 3 the motion table.
                if garden > 3
                    || parts == 0
                    || part >= parts
                    || parts > MAX_LAND_PARTS
                    || len > LAND_PART
                {
                    return Err(WireError::BadValue("garden land"));
                }
                let bytes = r.take(len)?.to_vec();
                Self::GardenLand {
                    garden,
                    fingerprint,
                    part,
                    parts,
                    bytes,
                }
            }
            tag::CHAO_CHANGES => {
                let (owner, index, count) = (chao_owner(r)?, r.u8()?, usize::from(r.u8()?));
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
                let (owner, index) = (chao_owner(r)?, r.u8()?);
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
                let area = r.u8()?;
                Self::GardenItems {
                    area,
                    items: read_items(r, area)?,
                }
            }
            tag::SERVER_ITEMS => {
                let area = r.u8()?;
                Self::ServerItems {
                    area,
                    items: read_items(r, area)?,
                }
            }
            tag::CARRIED_CHAO => {
                let offset = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                let chao = match r.u8()? {
                    0 => None,
                    1 => {
                        let chao =
                            ChaoData::from_bytes(r.take(CHAO_SIZE)?).ok_or(WireError::Truncated)?;
                        chao.validate()
                            .map_err(|_| WireError::BadValue("carried chao"))?;
                        Some(Box::new(chao))
                    }
                    _ => return Err(WireError::BadValue("carried chao")),
                };
                Self::CarriedChao { chao, offset }
            }
            tag::SERVER_GARDEN => {
                let area = r.u8()?;
                if !(1..=3).contains(&area) {
                    return Err(WireError::BadValue("server garden"));
                }
                let mut trees = [[0u8; TREE_RECORD]; TREE_SLOTS];
                for b in trees.iter_mut().flatten() {
                    *b = r.u8()?;
                }
                let mut toys = [(0u8, 0u16); 3];
                for t in &mut toys {
                    *t = (r.u8()?, r.u16()?);
                    if t.0 > TOY_MAX_MODE {
                        return Err(WireError::BadValue("server garden toy"));
                    }
                }
                Self::ServerGarden { area, trees, toys }
            }
            tag::ITEM_CLAIM => Self::ItemClaim { id: item_id(r)? },
            tag::RACE_STATUS => {
                let (level, mission, progress) = (r.i16()?, r.u8()?, r.u16()?);
                let finish = match r.u8()? {
                    0 => None,
                    1 => Some((r.u32()?, r.u32()?)),
                    _ => return Err(WireError::BadValue("race finish")),
                };
                if !(1..=MAX_LEVEL).contains(&level)
                    || mission > 4
                    || (progress > RACE_PROGRESS_MAX && progress != RACE_PROGRESS_UNKNOWN)
                {
                    return Err(WireError::BadValue("race status"));
                }
                Self::RaceStatus {
                    level,
                    mission,
                    progress,
                    finish,
                }
            }
            tag::HUNT_SET => {
                let (level, mission) = (r.i16()?, r.u8()?);
                let mut pieces = [(0, 0, [0.0; 3]); 3];
                for p in &mut pieces {
                    let (kind, id) = (r.u8()?, r.u8()?);
                    *p = (
                        kind,
                        id,
                        [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                    );
                }
                if !(1..=MAX_LEVEL).contains(&level) || mission > 4 {
                    return Err(WireError::BadValue("hunt set"));
                }
                Self::HuntSet {
                    level,
                    mission,
                    pieces,
                }
            }
            tag::HUNT_CLAIM | tag::HUNT_TAKEN => {
                let (level, mission, sig, slot) = (r.i16()?, r.u8()?, r.u32()?, r.u8()?);
                if !(1..=MAX_LEVEL).contains(&level) || mission > 4 || slot > 2 {
                    return Err(WireError::BadValue("hunt piece"));
                }
                if t == tag::HUNT_CLAIM {
                    Self::HuntClaim {
                        level,
                        mission,
                        sig,
                        slot,
                    }
                } else {
                    Self::HuntTaken {
                        level,
                        mission,
                        sig,
                        slot,
                        player: player_id(r)?,
                    }
                }
            }
            tag::KART_PICK => {
                let course = r.u8()?;
                if course > 2 {
                    return Err(WireError::BadValue("kart course"));
                }
                Self::KartPick { course }
            }
            tag::KART_STATE => {
                let position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                let rotation = [r.i32()?, r.i32()?, r.i32()?];
                let (yaw, wheel, kind, accel) = (r.i32()?, r.i32()?, r.u8()?, r.u8()?);
                let tilt = [r.i32()?, r.i32()?, r.i32()?];
                let frame = r.u16()?;
                if !(2..=7).contains(&kind) || accel > 1 {
                    return Err(WireError::BadValue("kart state"));
                }
                Self::KartState {
                    position,
                    rotation,
                    yaw,
                    wheel,
                    kind,
                    accel: accel == 1,
                    tilt,
                    frame,
                }
            }
            tag::ROUTE => {
                let (level, mission, count) = (r.i16()?, r.u8()?, usize::from(r.u16()?));
                if !(1..=MAX_LEVEL).contains(&level) || mission > 4 || count > MAX_ROUTE {
                    return Err(WireError::BadValue("route"));
                }
                let points = (0..count)
                    .map(|_| Ok([r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?]))
                    .collect::<Result<Vec<_>>>()?;
                Self::Route {
                    level,
                    mission,
                    points,
                }
            }
            tag::CHAT => {
                let text = r.str(MAX_CHAT)?;
                if text.is_empty() || !text.bytes().all(|b| (0x20..0x7F).contains(&b)) {
                    return Err(WireError::BadValue("chat"));
                }
                Self::Chat { text }
            }
            tag::ITEM_OWNER => Self::ItemOwner {
                id: item_id(r)?,
                // A player, or the server (a Chao it runs took it).
                owner: sender(r)?,
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
            tag::KEY_USED => {
                let (level, mission, key) = (r.i16()?, r.u8()?, r.u8()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || key > 1 {
                    return Err(WireError::BadValue("key used"));
                }
                Self::KeyUsed {
                    level,
                    mission,
                    key,
                }
            }
            tag::SHOT => {
                let (level, mission, kind) = (r.i16()?, r.u8()?, r.u8()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || kind > 1 {
                    return Err(WireError::BadValue("shot"));
                }
                let position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                let direction = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                Self::Shot {
                    level,
                    mission,
                    kind,
                    position,
                    direction,
                }
            }
            tag::SWITCH => {
                let (level, mission, id, on) = (r.i16()?, r.u8()?, r.u8()?, r.u8()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || id >= 32 || on > 1 {
                    return Err(WireError::BadValue("switch"));
                }
                Self::Switch {
                    level,
                    mission,
                    id,
                    on: on == 1,
                }
            }
            tag::CHECKPOINT => {
                let (level, mission, record) = (r.i16()?, r.u8()?, r.u16()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || record >= MAX_STAGE_OBJECTS {
                    return Err(WireError::BadValue("checkpoint"));
                }
                let position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                let rotation = [r.i32()?, r.i32()?, r.i32()?];
                Self::Checkpoint {
                    level,
                    mission,
                    record,
                    position,
                    rotation,
                }
            }
            tag::BALL => {
                let (area, claim) = (r.u8()?, r.u16()?);
                if !(1..=3).contains(&area) {
                    return Err(WireError::BadValue("ball"));
                }
                let position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                let velocity = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                Self::Ball {
                    area,
                    claim,
                    position,
                    velocity,
                    flags: r.u8()? & (BALL_HELD | BALL_IN_WATER),
                }
            }
            tag::GARDEN_TREES => {
                let area = r.u8()?;
                if !(1..=3).contains(&area) {
                    return Err(WireError::BadValue("garden trees"));
                }
                let mut trees = [[0u8; TREE_RECORD]; TREE_SLOTS];
                for b in trees.iter_mut().flatten() {
                    *b = r.u8()?;
                }
                let thirsty = r.u8()? & ((1 << TREE_SLOTS) - 1) as u8;
                Self::GardenTrees {
                    area,
                    trees,
                    thirsty,
                }
            }
            tag::TREE_ACTION => {
                let (area, slot, action, kind) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
                if !(1..=3).contains(&area)
                    || usize::from(slot) >= TREE_SLOTS
                    || action > TREE_WATER
                {
                    return Err(WireError::BadValue("tree action"));
                }
                Self::TreeAction {
                    area,
                    slot,
                    action,
                    kind,
                }
            }
            tag::GARDEN_EGGS => {
                let (area, count) = (r.u8()?, usize::from(r.u8()?));
                if !(1..=3).contains(&area) || count > MAX_CARRIED_CHAO {
                    return Err(WireError::BadValue("garden eggs"));
                }
                let mut eggs = Vec::with_capacity(count);
                for _ in 0..count {
                    let index = r.u8()?;
                    let position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                    let held = r.u8()?;
                    if usize::from(index) >= MAX_CARRIED_CHAO || held > 1 {
                        return Err(WireError::BadValue("garden egg"));
                    }
                    eggs.push((index, position, held == 1));
                }
                Self::GardenEggs { area, eggs }
            }
            tag::TOY_MODE => {
                let (area, toy, mode) = (r.u8()?, r.u8()?, r.u8()?);
                if !(1..=3).contains(&area) || toy > TOY_BOX || mode > TOY_MAX_MODE {
                    return Err(WireError::BadValue("toy mode"));
                }
                Self::ToyMode { area, toy, mode }
            }
            tag::EGG_HELD => {
                let (owner, index, held) = (player_id(r)?, r.u8()?, r.u8()?);
                let position = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                if usize::from(index) >= MAX_CARRIED_CHAO || held > 1 {
                    return Err(WireError::BadValue("egg held"));
                }
                Self::EggHeld {
                    owner,
                    index,
                    held: held == 1,
                    position,
                }
            }
            tag::BOSS_HP => {
                let (level, mission) = (r.i16()?, r.u8()?);
                let (hp, full) = (r.f32(MAX_BOSS_HP)?, r.f32(MAX_BOSS_HP)?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || hp < 0.0 || full <= 0.0 {
                    return Err(WireError::BadValue("boss hp"));
                }
                Self::BossHp {
                    level,
                    mission,
                    hp,
                    full,
                }
            }
            tag::STAGE_RINGS => {
                let (level, mission) = (r.i16()?, r.u8()?);
                let count = r.u16()?;
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || count > MAX_STAGE_OBJECTS {
                    return Err(WireError::BadValue("stage rings"));
                }
                let rings = (0..count)
                    .map(|_| {
                        let (record, ring) = (r.u16()?, r.u8()?);
                        if record < MAX_STAGE_OBJECTS && ring < 8 {
                            Ok((record, ring))
                        } else {
                            Err(WireError::BadValue("stage ring"))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::StageRings {
                    level,
                    mission,
                    rings,
                }
            }
            tag::LOCK_ON => {
                let (level, mission) = (r.i16()?, r.u8()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 {
                    return Err(WireError::BadValue("lock-on"));
                }
                let gun = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                let mut list = || {
                    let count = r.u8()?;
                    if count > MAX_LOCK_ON {
                        return Err(WireError::BadValue("lock-on count"));
                    }
                    (0..count)
                        .map(|_| {
                            let record = r.u16()?;
                            let at = [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?];
                            if record < MAX_STAGE_OBJECTS {
                                Ok((record, at))
                            } else {
                                Err(WireError::BadValue("lock-on record"))
                            }
                        })
                        .collect::<Result<Vec<_>>>()
                };
                let locked = list()?;
                let fired = list()?;
                Self::LockOn {
                    level,
                    mission,
                    gun,
                    locked,
                    fired,
                }
            }
            tag::BOSS_CLAIM | tag::BOSS_HIT => {
                let (level, mission) = (r.i16()?, r.u8()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 {
                    return Err(WireError::BadValue("boss"));
                }
                if t == tag::BOSS_CLAIM {
                    Self::BossClaim {
                        level,
                        mission,
                        term: r.u32()?,
                    }
                } else {
                    Self::BossHit { level, mission }
                }
            }
            tag::BOSS_ATTACKS => {
                let (level, mission, count) = (r.i16()?, r.u8()?, usize::from(r.u8()?));
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || count > MAX_BOSS_ATTACKS {
                    return Err(WireError::BadValue("boss attacks"));
                }
                let attacks = (0..count)
                    .map(|_| {
                        Ok(BossAttack {
                            serial: r.u16()?,
                            kind: r.u8()?,
                            target: player_id(r)?,
                            data: r.take(BOSS_ATTACK_DATA)?.try_into().unwrap(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::BossAttacks {
                    level,
                    mission,
                    attacks,
                }
            }
            tag::GARDEN_UNLOCKS => {
                let gardens = r.u8()?;
                if gardens & !(GARDEN_HERO | GARDEN_DARK) != 0 {
                    return Err(WireError::BadValue("garden unlocks"));
                }
                Self::GardenUnlocks { gardens }
            }
            tag::UPGRADE_GOT => {
                let (level, mission) = (r.i16()?, r.u8()?);
                let (index, character, mask) = (r.u8()?, r.u8()?, r.u32()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || index >= 0x1E {
                    return Err(WireError::BadValue("upgrade"));
                }
                Self::UpgradeGot {
                    level,
                    mission,
                    index,
                    character,
                    mask,
                }
            }
            tag::ENEMY_STATES => {
                let (level, mission, count) = (r.i16()?, r.u8()?, usize::from(r.u8()?));
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 || count > MAX_ENEMY_STATES {
                    return Err(WireError::BadValue("enemy states"));
                }
                let enemies = (0..count)
                    .map(|_| {
                        let record = r.u16()?;
                        if record >= MAX_STAGE_OBJECTS {
                            return Err(WireError::BadValue("enemy record"));
                        }
                        Ok(EnemyState {
                            record,
                            position: [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                            rotation: [r.i32()?, r.i32()?, r.i32()?],
                            distance: r.f32(MAX_COORD)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::EnemyStates {
                    level,
                    mission,
                    enemies,
                }
            }
            tag::BOSS_STATE => {
                let (level, mission) = (r.i16()?, r.u8()?);
                if !(0..=MAX_LEVEL).contains(&level) || mission > 4 {
                    return Err(WireError::BadValue("boss state"));
                }
                Self::BossState(BossState {
                    level,
                    mission,
                    mode: r.u8()?,
                    sub: r.u8()?,
                    counter: r.u8()?,
                    flags: r.u16()?,
                    timer: r.u16()?,
                    position: [r.f32(MAX_COORD)?, r.f32(MAX_COORD)?, r.f32(MAX_COORD)?],
                    rotation: [r.i32()?, r.i32()?, r.i32()?],
                    motion: r.u16()?,
                    target: player_id(r)?,
                })
            }
            tag::SKIN_WORN => {
                let skin = r.str(MAX_SKIN_ID)?;
                if !skin.is_empty() && !skin_id_ok(&skin) {
                    return Err(WireError::BadValue("skin id"));
                }
                Self::SkinWorn { skin }
            }
            tag::SKIN_LIST_GET => Self::SkinListGet { page: r.u16()? },
            tag::SKIN_LIST => {
                let (page, pages, count) = (r.u16()?, r.u16()?, usize::from(r.u8()?));
                let max_pages = MAX_SKIN_FILES.div_ceil(SKIN_LIST_PAGE) as u16;
                if count > SKIN_LIST_PAGE || pages > max_pages || (pages > 0 && page >= pages) {
                    return Err(WireError::BadValue("skin list"));
                }
                let files = (0..count)
                    .map(|_| {
                        let path = r.str(MAX_SKIN_PATH)?;
                        let size = r.u32()?;
                        let hash = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
                        if !skin_path_ok(&path) || size > MAX_SKIN_FILE {
                            return Err(WireError::BadValue("skin file"));
                        }
                        Ok(SkinFile { path, size, hash })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::SkinList { page, pages, files }
            }
            tag::SKIN_GET => {
                let (file, offset) = (r.u16()?, r.u32()?);
                if usize::from(file) >= MAX_SKIN_FILES || offset >= MAX_SKIN_FILE {
                    return Err(WireError::BadValue("skin get"));
                }
                Self::SkinGet { file, offset }
            }
            tag::SKIN_CHUNK => {
                let (file, offset, len) = (r.u16()?, r.u32()?, usize::from(r.u16()?));
                if usize::from(file) >= MAX_SKIN_FILES
                    || len == 0
                    || len > SKIN_CHUNK
                    || offset.saturating_add(len as u32) > MAX_SKIN_FILE
                {
                    return Err(WireError::BadValue("skin chunk"));
                }
                Self::SkinChunk {
                    file,
                    offset,
                    data: r.take(len)?.to_vec(),
                }
            }
            tag::BROUGHT => {
                let (awake, lock, count) = (r.u8()?, r.u8()?, usize::from(r.u8()?));
                let lock = ChaoLock::from_u8(lock).ok_or(WireError::BadValue("brought lock"))?;
                if awake > 1 || count > MAX_BROUGHT {
                    return Err(WireError::BadValue("brought"));
                }
                let list = (0..count)
                    .map(|_| {
                        let (garden, row, nth) = (r.u8()?, r.u8()?, r.u8()?);
                        if !(1..=3).contains(&garden) || usize::from(row) >= MAX_CARRIED_CHAO {
                            return Err(WireError::BadValue("brought chao"));
                        }
                        Ok(BroughtChao {
                            garden,
                            row,
                            nth,
                            data: chao_data(r)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::Brought {
                    awake: awake == 1,
                    lock,
                    list,
                }
            }
            tag::HELD_CHAO => {
                let (owner, index, has) = (r.u8()?, r.u8()?, r.u8()?);
                if !(HELD_OWNER..=MAX_CHAO_OWNER).contains(&owner)
                    || usize::from(index) >= MAX_CARRIED_CHAO
                    || has > 1
                {
                    return Err(WireError::BadValue("held chao"));
                }
                let chao = if has == 1 {
                    let (area, asleep, locked) = (r.u8()?, r.u8()?, r.u8()?);
                    if !(1..=3).contains(&area) || asleep > 1 || locked > 1 {
                        return Err(WireError::BadValue("held chao"));
                    }
                    Some(HeldChao {
                        area,
                        asleep: asleep == 1,
                        locked: locked == 1,
                        name: name(r)?,
                        data: chao_data(r)?,
                    })
                } else {
                    None
                };
                Self::HeldChao { owner, index, chao }
            }
            tag::SAVE_CHAO => {
                let identity = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
                let (nth, count) = (r.u8()?, usize::from(r.u8()?));
                if count > MAX_SAVE_SPANS {
                    return Err(WireError::BadValue("save chao"));
                }
                let spans = (0..count)
                    .map(|_| {
                        let (at, len) = (r.u16()?, usize::from(r.u8()?));
                        if !save_field(usize::from(at), len) {
                            return Err(WireError::BadValue("save chao field"));
                        }
                        Ok((at, r.take(len)?.to_vec()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::SaveChao {
                    identity,
                    nth,
                    spans,
                }
            }
            tag::CHAO_LOCKED => {
                let count = usize::from(r.u8()?);
                if count > MAX_CHAO_LOCKED {
                    return Err(WireError::BadValue("chao locked"));
                }
                let chao = (0..count)
                    .map(|_| {
                        let (owner, index) = (r.u8()?, r.u8()?);
                        if owner > MAX_CHAO_OWNER || usize::from(index) >= MAX_CARRIED_CHAO {
                            return Err(WireError::BadValue("chao locked"));
                        }
                        Ok((owner, index))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::ChaoLocked { chao }
            }
            tag::PARTNERS => {
                use crate::gba::rom::{ABILITY_COUNT, MOVE_COUNT, SPECIES_COUNT, TYPE_COUNT};
                let count = usize::from(r.u8()?);
                if count > MAX_PARTNERS {
                    return Err(WireError::BadValue("partners"));
                }
                let list = (0..count)
                    .map(|_| {
                        let identity = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
                        let species = r.u16()?;
                        let types = [r.u8()?, r.u8()?];
                        let ability = r.u8()?;
                        let moves = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
                        let level = r.u8()?;
                        if species >= SPECIES_COUNT
                            || types.iter().any(|&t| t >= TYPE_COUNT)
                            || ability >= ABILITY_COUNT
                            || moves.iter().any(|&m| m >= MOVE_COUNT)
                            || !(1..=100).contains(&level)
                        {
                            return Err(WireError::BadValue("partners"));
                        }
                        Ok(SharedPartner {
                            identity,
                            species,
                            types,
                            ability,
                            moves,
                            level,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::Partners { list }
            }
            tag::BATTLE_CHALLENGE => Self::BattleChallenge {
                to: player_id(r)?,
                format: battle_format(r)?,
                save_partners: wire_flag(r, "battle rule")?,
                actual_levels: wire_flag(r, "battle rule")?,
                dex: r.u32()?,
                team: read_battle_team(r, 1)?,
            },
            tag::BATTLE_OFFER => Self::BattleOffer {
                from: player_id(r)?,
                format: battle_format(r)?,
                save_partners: wire_flag(r, "battle rule")?,
                actual_levels: wire_flag(r, "battle rule")?,
            },
            tag::BATTLE_ANSWER => {
                let to = player_id(r)?;
                let answer = r.u8()?;
                let dex = r.u32()?;
                let team = read_battle_team(r, 0)?;
                let ok = match answer {
                    ANSWER_ACCEPT => !team.is_empty(),
                    ANSWER_DECLINE | ANSWER_BUSY => team.is_empty(),
                    _ => false,
                };
                if !ok {
                    return Err(WireError::BadValue("battle answer"));
                }
                Self::BattleAnswer {
                    to,
                    answer,
                    dex,
                    team,
                }
            }
            tag::BATTLE_START => {
                let battle = r.u16()?;
                let format = battle_format(r)?;
                let side = r.u8()?;
                if side > 1 {
                    return Err(WireError::BadValue("battle side"));
                }
                Self::BattleStart {
                    battle,
                    format,
                    side,
                    opponent: player_id(r)?,
                    seed: u64::from_le_bytes(r.take(8)?.try_into().unwrap()),
                    team: read_battle_team(r, 1)?,
                }
            }
            tag::BATTLE_CHOICE => {
                let battle = r.u16()?;
                let step = r.u16()?;
                let check = r.u32()?;
                let count = usize::from(r.u8()?);
                if count > 2 {
                    return Err(WireError::BadValue("battle picks"));
                }
                let picks = (0..count)
                    .map(|_| read_battle_pick(r))
                    .collect::<Result<_>>()?;
                Self::BattleChoice {
                    battle,
                    step,
                    check,
                    picks,
                }
            }
            tag::BATTLE_STEP => {
                let battle = r.u16()?;
                let step = r.u16()?;
                let seed = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
                let count = usize::from(r.u8()?);
                if count > 4 {
                    return Err(WireError::BadValue("battle picks"));
                }
                let picks = (0..count)
                    .map(|_| {
                        let side = r.u8()?;
                        if side > 1 {
                            return Err(WireError::BadValue("battle side"));
                        }
                        Ok((side, read_battle_pick(r)?))
                    })
                    .collect::<Result<_>>()?;
                Self::BattleStep {
                    battle,
                    step,
                    seed,
                    picks,
                }
            }
            tag::BATTLE_LEAVE => {
                let battle = r.u16()?;
                let reason = r.u8()?;
                if reason > LEAVE_LEFT {
                    return Err(WireError::BadValue("battle leave"));
                }
                Self::BattleLeave { battle, reason }
            }
            tag::BATTLE_END => {
                let battle = r.u16()?;
                let reason = r.u8()?;
                let winner = r.u8()?;
                if reason > END_WITHDRAWN || winner > NO_WINNER {
                    return Err(WireError::BadValue("battle end"));
                }
                Self::BattleEnd {
                    battle,
                    reason,
                    winner,
                }
            }
            tag::WILD_HELLO => {
                let (flags, print) = (r.u8()?, r.u32()?);
                let see = flags & WILD_SEE != 0;
                if flags & !(WILD_SEE | WILD_PICK | WILD_MACHINE) != 0 || see != (print != 0) {
                    return Err(WireError::BadValue("wild hello"));
                }
                Self::WildHello { flags, print }
            }
            tag::WILD_LIST => {
                let area = wild_area(r)?;
                let serial = r.u16()?;
                let section = read_section(r)?;
                let print = r.u32()?;
                let picker = match r.u8()? {
                    NO_PLAYER => None,
                    id if usize::from(id) < MAX_PLAYERS => Some(id),
                    _ => return Err(WireError::BadValue("wild picker")),
                };
                let learners = r.u16()?;
                let count = usize::from(r.u8()?);
                if count > MAX_WILD_LIST {
                    return Err(WireError::BadValue("wild list"));
                }
                let list: Vec<WildEntry> = (0..count)
                    .map(|_| read_wild_entry(r))
                    .collect::<Result<_>>()?;
                if (0..count).any(|i| list[..i].iter().any(|e| e.id == list[i].id)) {
                    return Err(WireError::BadValue("wild list"));
                }
                Self::WildList {
                    area,
                    serial,
                    section,
                    print,
                    picker,
                    learners,
                    list,
                }
            }
            tag::WILD_ASK => {
                let area = wild_area(r)?;
                let ask = r.u16()?;
                let section = read_section(r)?;
                let (land, pond) = (r.u8()?, r.u8()?);
                let fresh = wire_flag(r, "wild ask")?;
                if !(1..=MAX_WILD_PICK).contains(&(usize::from(land) + usize::from(pond))) {
                    return Err(WireError::BadValue("wild ask"));
                }
                Self::WildAsk {
                    area,
                    ask,
                    section,
                    land,
                    pond,
                    fresh,
                }
            }
            tag::WILD_PICK => {
                let area = wild_area(r)?;
                let ask = r.u16()?;
                let section = r.u8()?;
                let count = usize::from(r.u8()?);
                if section == NO_SECTION || count > MAX_WILD_PICK {
                    return Err(WireError::BadValue("wild pick"));
                }
                let list = (0..count)
                    .map(|_| read_wild_picked(r))
                    .collect::<Result<_>>()?;
                Self::WildPick {
                    area,
                    ask,
                    section,
                    list,
                }
            }
            tag::WILD_BRING => {
                let area = wild_area(r)?;
                let key = read_u64(r)?;
                let from_save = wire_flag(r, "wild bring")?;
                let creature = read_creature(r)?;
                let place = r.u8()?;
                let (v, angle) = (read_v3(r, MAX_COORD)?, r.u16()?);
                let at = match place {
                    0 => None,
                    2 => Some((v, angle)),
                    _ => return Err(WireError::BadValue("wild bring")),
                };
                Self::WildBring {
                    area,
                    key,
                    from_save,
                    creature,
                    at,
                }
            }
            tag::WILD_CARRY => Self::WildCarry {
                area: wild_area(r)?,
                creature: read_creature(r)?,
                offset: read_v3(r, MAX_WILD_OFFSET)?,
            },
            tag::WILD_TAKE => Self::WildTake { id: wild_id(r)? },
            tag::WILD_HELD => {
                let id = wild_id(r)?;
                let let_go = wire_flag(r, "wild held")?;
                let v = read_v3(r, if let_go { MAX_COORD } else { MAX_WILD_OFFSET })?;
                Self::WildHeld { id, let_go, v }
            }
            tag::WILD_PUT => Self::WildPut {
                id: wild_id(r)?,
                at: read_v3(r, MAX_COORD)?,
                angle: r.u16()?,
            },
            tag::WILD_ABSORBED => {
                let id = wild_id(r)?;
                let owner = chao_owner(r)?;
                let index = r.u8()?;
                if usize::from(index) >= MAX_CARRIED_CHAO {
                    return Err(WireError::BadValue("wild absorbed"));
                }
                Self::WildAbsorbed { id, owner, index }
            }
            tag::WILD_BATTLE => {
                let id = wild_id(r)?;
                let step = match r.u8()? {
                    0 => WildStep::Reserve,
                    1 => WildStep::Won,
                    2 => WildStep::Lost,
                    3 => WildStep::Cancelled,
                    _ => return Err(WireError::BadValue("wild battle")),
                };
                Self::WildBattle { id, step }
            }
            tag::WILD_ANSWER => {
                let kind = match r.u8()? {
                    0 => WildRequest::Take,
                    1 => WildRequest::Carry,
                    2 => WildRequest::Bring,
                    3 => WildRequest::Battle,
                    _ => return Err(WireError::BadValue("wild answer")),
                };
                Self::WildAnswer {
                    kind,
                    id: r.u16()?,
                    key: read_u64(r)?,
                    ok: wire_flag(r, "wild answer")?,
                }
            }
            tag::WILD_TAUGHT => {
                let identity = read_u64(r)?;
                let origin = match r.u8()? {
                    0 => WildOrigin::Wild,
                    1 => WildOrigin::SavePartner,
                    2 => WildOrigin::Partner,
                    _ => return Err(WireError::BadValue("wild taught")),
                };
                Self::WildTaught {
                    identity,
                    origin,
                    creature: read_creature(r)?,
                }
            }
            tag::WILD_USED => Self::WildUsed {
                garden: wild_area(r)?,
                key: read_u64(r)?,
            },
            _ => return Err(WireError::BadTag("body", t)),
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
                key: [0xA5; 16],
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
                    action: 0,
                    status: 0,
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
                body: Body::GardenChao {
                    owner: 3,
                    list: vec![
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
                            props: [5, 0, 20],
                            drum: 200,
                            cocoon: 2,
                            book: 3,
                            owner_runs: true,
                        },
                        GardenChao::default(),
                    ],
                },
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
                            unsaved: false,
                        },
                        GardenItem {
                            id: ItemId {
                                origin: 5,
                                serial: 300,
                            },
                            category: 2,
                            kind: 0x16,
                            bites: 0,
                            place: ItemPlace::Chao { owner: 3, index: 1 },
                            position: [0.0, 1.5, 1.0],
                            unsaved: false,
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
                            unsaved: false,
                        },
                    ],
                },
            },
            Message::Game {
                from: 3,
                body: Body::RaceStatus {
                    level: 13,
                    mission: 0,
                    progress: 4567,
                    finish: Some((8345, 8000)),
                },
            },
            Message::Game {
                from: 3,
                body: Body::RaceStatus {
                    level: 13,
                    mission: 1,
                    progress: RACE_PROGRESS_UNKNOWN,
                    finish: None,
                },
            },
            Message::Game {
                from: 1,
                body: Body::KartPick { course: 2 },
            },
            Message::Game {
                from: 1,
                body: Body::HuntSet {
                    level: 16,
                    mission: 0,
                    pieces: [
                        (1, 3, [10.0, 20.0, 30.0]),
                        (2, 9, [-5.0, 0.0, 400.0]),
                        (7, 1, [0.0, 0.0, 0.0]),
                    ],
                },
            },
            Message::Game {
                from: 2,
                body: Body::HuntClaim {
                    level: 16,
                    mission: 0,
                    sig: 0xDEAD_BEEF,
                    slot: 1,
                },
            },
            Message::Game {
                from: 0,
                body: Body::HuntTaken {
                    level: 16,
                    mission: 0,
                    sig: 0xDEAD_BEEF,
                    slot: 1,
                    player: 2,
                },
            },
            Message::Game {
                from: 1,
                body: Body::KartState {
                    position: [100.0, 5.0, -300.0],
                    rotation: [0, 0x4000, 0],
                    yaw: 0x100,
                    wheel: -0x2000,
                    kind: 4,
                    accel: true,
                    tilt: [0x100, -0x200, 0],
                    frame: 0xFFF0,
                },
            },
            Message::Game {
                from: 3,
                body: Body::Route {
                    level: 13,
                    mission: 0,
                    points: vec![[0.0, 1.0, 2.0], [50.0, 0.0, -10.0]],
                },
            },
            Message::Game {
                from: 5,
                body: Body::Chat {
                    text: "gg, nice run!".into(),
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
                body: Body::KeyUsed {
                    level: 26,
                    mission: 0,
                    key: 1,
                },
            },
            Message::Game {
                from: 2,
                body: Body::Shot {
                    level: 11,
                    mission: 0,
                    kind: 1,
                    position: [1.0, 2.0, -3.0],
                    direction: [0.0, 0.5, 8.0],
                },
            },
            Message::Game {
                from: 0,
                body: Body::Switch {
                    level: 26,
                    mission: 0,
                    id: 31,
                    on: true,
                },
            },
            Message::Game {
                from: 1,
                body: Body::Checkpoint {
                    level: 13,
                    mission: 1,
                    record: 0x5FF,
                    position: [10.0, -20.5, 3000.0],
                    rotation: [0, -0x4000, 0x10],
                },
            },
            Message::Game {
                from: 2,
                body: Body::Ball {
                    area: 2,
                    claim: 7,
                    position: [1.5, 0.0, -40.0],
                    velocity: [0.25, -0.5, 1.0],
                    flags: BALL_HELD,
                },
            },
            Message::Game {
                from: 1,
                body: Body::GardenTrees {
                    area: 3,
                    trees: [[5, 3, 40, 90, 10, 200, 0, 0x70]; TREE_SLOTS],
                    thirsty: 0b100_0010,
                },
            },
            Message::Game {
                from: 3,
                body: Body::GardenEggs {
                    area: 2,
                    eggs: vec![(0, [1.0, 2.0, 3.0], false), (3, [-4.5, 0.0, 9.0], true)],
                },
            },
            Message::Game {
                from: 1,
                body: Body::GardenKeeper { area: 3, term: 7 },
            },
            Message::Game {
                from: 0,
                body: Body::ServerGarden {
                    area: 2,
                    trees: [[1, 2, 3, 4, 5, 6, 7, 8]; TREE_SLOTS],
                    toys: [(3, 12), (0, u16::MAX), (1, 0)],
                },
            },
            Message::Game {
                from: 0,
                body: Body::ServerItems {
                    area: 3,
                    items: vec![GardenItem {
                        id: ItemId {
                            origin: 4,
                            serial: 0xF001,
                        },
                        category: 3,
                        kind: 1,
                        bites: 2,
                        position: [5.0, 0.5, -7.0],
                        unsaved: true,
                        ..GardenItem::default()
                    }],
                },
            },
            Message::Game {
                from: 2,
                body: Body::CarriedChao {
                    chao: None,
                    offset: [0.0, 4.0, 1.2],
                },
            },
            Message::Game {
                from: 1,
                body: Body::ChaoSounds {
                    sounds: vec![
                        ChaoSound {
                            owner: 2,
                            index: 3,
                            call: SOUND_HELD,
                            id: 0x101C,
                            offset: [0.0, 1.5, -0.5],
                            args: [1, -5, 60],
                        },
                        ChaoSound {
                            call: SOUND_JINGLE,
                            id: 4,
                            ..ChaoSound::default()
                        },
                    ],
                },
            },
            Message::Game {
                from: 2,
                body: Body::ToyMode {
                    area: 2,
                    toy: TOY_RADIO,
                    mode: 5,
                },
            },
            Message::Game {
                from: 1,
                body: Body::EggHeld {
                    owner: 3,
                    index: 3,
                    held: true,
                    position: [10.0, 0.5, -2.0],
                },
            },
            Message::Game {
                from: 2,
                body: Body::TreeAction {
                    area: 1,
                    slot: 6,
                    action: TREE_SHAKE,
                    kind: 0,
                },
            },
            Message::Game {
                from: 0,
                body: Body::BossHp {
                    level: 61,
                    mission: 0,
                    hp: 2.5,
                    full: 8.0,
                },
            },
            Message::Game {
                from: 3,
                body: Body::StageRings {
                    level: 13,
                    mission: 0,
                    rings: vec![(4, 0), (4, 7), (0x5FF, 3)],
                },
            },
            Message::Game {
                from: 2,
                body: Body::LockOn {
                    level: 14,
                    mission: 1,
                    gun: [1.0, 2.5, -3.0],
                    locked: vec![(9, [10.0, 0.0, 5.0])],
                    fired: vec![(3, [-1.0, 8.0, 2.0]), (0x5FF, [0.0, 0.0, 0.0])],
                },
            },
            Message::Game {
                from: 1,
                body: Body::BossClaim {
                    level: 61,
                    mission: 0,
                    term: 3,
                },
            },
            Message::Game {
                from: 1,
                body: Body::BossState(BossState {
                    level: 60,
                    mission: 0,
                    mode: 5,
                    sub: 2,
                    counter: 1,
                    flags: 0x11,
                    timer: 300,
                    position: [10.0, -2.0, 300.5],
                    rotation: [0, 0x4000, -1],
                    motion: 3,
                    target: 2,
                }),
            },
            Message::Game {
                from: 4,
                body: Body::BossHit {
                    level: 61,
                    mission: 0,
                },
            },
            Message::Game {
                from: 1,
                body: Body::BossAttacks {
                    level: 60,
                    mission: 0,
                    attacks: vec![BossAttack {
                        serial: 7,
                        kind: 2,
                        target: 3,
                        data: [5; BOSS_ATTACK_DATA],
                    }],
                },
            },
            Message::Game {
                from: 2,
                body: Body::EnemyStates {
                    level: 14,
                    mission: 0,
                    enemies: vec![EnemyState {
                        record: 40,
                        position: [1.0, 2.0, 3.0],
                        rotation: [0, 0x2000, 0],
                        distance: 55.5,
                    }],
                },
            },
            Message::Game {
                from: 3,
                body: Body::UpgradeGot {
                    level: 3,
                    mission: 0,
                    index: 4,
                    character: 0,
                    mask: 0x40,
                },
            },
            Message::Game {
                from: 2,
                body: Body::GardenUnlocks {
                    gardens: GARDEN_HERO | GARDEN_DARK,
                },
            },
            Message::Game {
                from: 2,
                body: Body::SkinWorn {
                    skin: "space_hero".into(),
                },
            },
            Message::Game {
                from: 2,
                body: Body::SkinWorn { skin: "".into() },
            },
            Message::Game {
                from: 0,
                body: Body::SkinListGet { page: 1 },
            },
            Message::Game {
                from: 0,
                body: Body::SkinList {
                    page: 0,
                    pages: 2,
                    files: vec![SkinFile {
                        path: "my_skin/sounds/jump/01 - Whoa.wav".into(),
                        size: 12345,
                        hash: 0xDEAD_BEEF_0123_4567,
                    }],
                },
            },
            Message::Game {
                from: 0,
                body: Body::SkinGet {
                    file: 3,
                    offset: 2048,
                },
            },
            Message::Game {
                from: 0,
                body: Body::SkinChunk {
                    file: 3,
                    offset: 2048,
                    data: vec![7; 1000],
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
            Message::Game {
                from: 3,
                body: Body::Brought {
                    awake: true,
                    lock: ChaoLock::ThatGarden,
                    list: vec![
                        BroughtChao {
                            garden: 1,
                            row: 0,
                            nth: 0,
                            data: Box::new(valid_chao()),
                        },
                        BroughtChao {
                            garden: 3,
                            row: 1,
                            nth: 1,
                            data: Box::new(valid_chao()),
                        },
                    ],
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::HeldChao {
                    owner: HELD_OWNER + 2,
                    index: 3,
                    chao: Some(HeldChao {
                        area: 2,
                        asleep: true,
                        locked: true,
                        name: "Player1".into(),
                        data: Box::new(valid_chao()),
                    }),
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::HeldChao {
                    owner: MAX_CHAO_OWNER,
                    index: 0,
                    chao: None,
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::SaveChao {
                    identity: 0x0123_4567_89AB_CDEF,
                    nth: 1,
                    spans: vec![(0x80, vec![0x0B]), (0x12C, vec![1, 2, 3])],
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::ChaoLocked {
                    chao: vec![(0, 3), (HELD_OWNER + 1, 0)],
                },
            },
            Message::Game {
                from: 2,
                body: Body::Partners {
                    list: vec![SharedPartner {
                        identity: 0x0123_4567_89AB_CDEF,
                        species: 200,
                        types: [10, 2],
                        ability: 66,
                        moves: [33, 45, 0, 0],
                        level: 23,
                    }],
                },
            },
            Message::Game {
                from: 1,
                body: Body::BattleChallenge {
                    to: 2,
                    format: 1,
                    save_partners: false,
                    actual_levels: true,
                    dex: 0xDEAD_BEEF,
                    team: vec![battle_member(); 2],
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::BattleOffer {
                    from: 1,
                    format: 0,
                    save_partners: true,
                    actual_levels: false,
                },
            },
            Message::Game {
                from: 2,
                body: Body::BattleAnswer {
                    to: 1,
                    answer: ANSWER_ACCEPT,
                    dex: 7,
                    team: vec![battle_member()],
                },
            },
            Message::Game {
                from: 2,
                body: Body::BattleAnswer {
                    to: 1,
                    answer: ANSWER_BUSY,
                    dex: 0,
                    team: Vec::new(),
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::BattleStart {
                    battle: 3,
                    format: 1,
                    side: 1,
                    opponent: 1,
                    seed: 0x0123_4567_89AB_CDEF,
                    team: vec![battle_member(); MAX_BATTLE_TEAM],
                },
            },
            Message::Game {
                from: 1,
                body: Body::BattleChoice {
                    battle: 3,
                    step: 9,
                    check: 0x1234_5678,
                    picks: vec![
                        BattlePick {
                            pos: 0,
                            kind: PICK_MOVE,
                            value: 3,
                            target: 3,
                            pass: 5,
                        },
                        BattlePick {
                            pos: 1,
                            kind: PICK_SWITCH,
                            value: 4,
                            target: PICK_NONE,
                            pass: PICK_NONE,
                        },
                    ],
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::BattleStep {
                    battle: 3,
                    step: 9,
                    seed: u64::MAX,
                    picks: vec![(
                        1,
                        BattlePick {
                            pos: 0,
                            kind: PICK_REPLACE,
                            value: 2,
                            target: PICK_NONE,
                            pass: PICK_NONE,
                        },
                    )],
                },
            },
            Message::Game {
                from: 2,
                body: Body::BattleLeave {
                    battle: 3,
                    reason: LEAVE_GAVE_UP,
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::BattleEnd {
                    battle: 0,
                    reason: END_WITHDRAWN,
                    winner: NO_WINNER,
                },
            },
            Message::Game {
                from: 2,
                body: Body::WildHello {
                    flags: WILD_SEE | WILD_PICK | WILD_MACHINE,
                    print: 0xDEAD_BEEF,
                },
            },
            Message::Game {
                from: 2,
                body: Body::WildHello { flags: 0, print: 0 },
            },
            Message::Game {
                from: SERVER,
                body: Body::WildList {
                    area: 2,
                    serial: 0xBEEF,
                    section: Some(16),
                    print: 9,
                    picker: Some(3),
                    learners: 0x8201,
                    list: vec![
                        WildEntry {
                            id: 1,
                            creature: wild_creature(),
                            bringer: None,
                            place: WildPlace::Roam,
                            v: [10.0, 0.5, -30.0],
                            angle: 0x1234,
                        },
                        WildEntry {
                            id: 2,
                            creature: WildCreature {
                                shiny: true,
                                ..wild_creature()
                            },
                            bringer: Some(4),
                            place: WildPlace::Held { by: 5 },
                            v: [0.0, 4.0, 2.0],
                            angle: 0,
                        },
                        WildEntry {
                            id: 0xFFFF,
                            creature: wild_creature(),
                            bringer: None,
                            place: WildPlace::Battle { by: 7 },
                            v: [-1.0, 2.0, 3.0],
                            angle: 9,
                        },
                        WildEntry {
                            id: 3,
                            creature: wild_creature(),
                            bringer: None,
                            place: WildPlace::Swim,
                            v: [-1.0, -2.0, 3.0],
                            angle: 9,
                        },
                    ],
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::WildList {
                    area: 1,
                    serial: 0,
                    section: None,
                    print: 0,
                    picker: None,
                    learners: 0,
                    list: Vec::new(),
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::WildAsk {
                    area: 3,
                    ask: 500,
                    section: None,
                    land: 2,
                    pond: 1,
                    fresh: true,
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildPick {
                    area: 3,
                    ask: 500,
                    section: 0,
                    list: vec![
                        WildPicked {
                            creature: wild_creature(),
                            arrive: WildArrive::Land,
                        },
                        WildPicked {
                            creature: wild_creature(),
                            arrive: WildArrive::Pond,
                        },
                        WildPicked {
                            creature: wild_creature(),
                            arrive: WildArrive::LandAt {
                                at: [1.0, 2.0, 3.0],
                                angle: 4,
                            },
                        },
                        WildPicked {
                            creature: wild_creature(),
                            arrive: WildArrive::SwimAt {
                                at: [1.0, -2.0, 3.0],
                                angle: 5,
                            },
                        },
                    ],
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildBring {
                    area: 1,
                    key: 0x0123_4567_89AB_CDEF,
                    from_save: true,
                    creature: wild_creature(),
                    at: Some(([50.0, 10.0, 22.0], 0x96F8)),
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildBring {
                    area: 2,
                    key: 1,
                    from_save: false,
                    creature: wild_creature(),
                    at: None,
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildCarry {
                    area: 1,
                    creature: wild_creature(),
                    offset: [0.0, 5.0, -2.0],
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildTake { id: 77 },
            },
            Message::Game {
                from: 1,
                body: Body::WildHeld {
                    id: 77,
                    let_go: false,
                    v: [0.0, 5.0, 2.0],
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildHeld {
                    id: 77,
                    let_go: true,
                    v: [100.0, 5.0, -200.0],
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildPut {
                    id: 77,
                    at: [100.0, 0.0, -200.0],
                    angle: 0x8000,
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildAbsorbed {
                    id: 77,
                    owner: 9,
                    index: 3,
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildBattle {
                    id: 77,
                    step: WildStep::Reserve,
                },
            },
            Message::Game {
                from: 1,
                body: Body::WildBattle {
                    id: 77,
                    step: WildStep::Cancelled,
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::WildAnswer {
                    kind: WildRequest::Bring,
                    id: 0,
                    key: 99,
                    ok: false,
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::WildAnswer {
                    kind: WildRequest::Battle,
                    id: 12,
                    key: 0,
                    ok: true,
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::WildTaught {
                    identity: 0xFEED_FACE_0000_0001,
                    origin: WildOrigin::SavePartner,
                    creature: wild_creature(),
                },
            },
            Message::Game {
                from: SERVER,
                body: Body::WildUsed {
                    garden: 3,
                    key: 0x77,
                },
            },
        ]
    }

    fn battle_member() -> BattleMember {
        BattleMember {
            chao: Box::new(valid_chao()),
            species: 25,
            types: [13, 13],
            ability: 9,
            moves: [84, 98, 0, 0],
            level: 37,
        }
    }

    #[test]
    fn live_battle_messages_stay_in_range() {
        let decodes =
            |body: Body| Message::decode(&Message::Game { from: 1, body }.encode()).is_ok();
        let challenge = |team: Vec<BattleMember>, format: u8| Body::BattleChallenge {
            to: 2,
            format,
            save_partners: true,
            actual_levels: false,
            dex: 0,
            team,
        };
        // The rules are flags.
        let bytes = Message::Game {
            from: 1,
            body: Body::BattleOffer {
                from: 2,
                format: 0,
                save_partners: false,
                actual_levels: true,
            },
        }
        .encode();
        for back in [1, 2] {
            let mut bytes = bytes.clone();
            let at = bytes.len() - back;
            bytes[at] = 2;
            assert!(Message::decode(&bytes).is_err());
        }
        assert!(decodes(challenge(vec![battle_member(); 6], 0)));
        assert!(!decodes(challenge(vec![battle_member(); 7], 0)));
        assert!(!decodes(challenge(Vec::new(), 0)));
        assert!(!decodes(challenge(vec![battle_member()], 2)));
        let mut bad = battle_member();
        bad.moves[3] = crate::gba::rom::MOVE_COUNT;
        assert!(!decodes(challenge(vec![bad], 0)));
        let mut bad = battle_member();
        bad.types[1] = crate::gba::rom::TYPE_COUNT;
        assert!(!decodes(challenge(vec![bad], 0)));
        let mut bad = battle_member();
        bad.ability = crate::gba::rom::ABILITY_COUNT;
        assert!(!decodes(challenge(vec![bad], 0)));
        for level in [0, 101] {
            let mut bad = battle_member();
            bad.level = level;
            assert!(!decodes(challenge(vec![bad], 0)));
        }
        // An accept comes with a team, the other answers without.
        let answer = |answer: u8, team: Vec<BattleMember>| Body::BattleAnswer {
            to: 2,
            answer,
            dex: 0,
            team,
        };
        assert!(decodes(answer(ANSWER_DECLINE, Vec::new())));
        assert!(!decodes(answer(ANSWER_ACCEPT, Vec::new())));
        assert!(!decodes(answer(ANSWER_DECLINE, vec![battle_member()])));
        assert!(!decodes(answer(3, Vec::new())));
        // Picks: at most 2 a player, 4 a step, each in range.
        let pick = |pos: u8, kind: u8, value: u8, target: u8, pass: u8| BattlePick {
            pos,
            kind,
            value,
            target,
            pass,
        };
        let choice = |picks: Vec<BattlePick>| Body::BattleChoice {
            battle: 1,
            step: 1,
            check: 0,
            picks,
        };
        let ok = pick(1, PICK_MOVE, 3, 2, PICK_NONE);
        assert!(decodes(choice(vec![ok; 2])));
        assert!(!decodes(choice(vec![ok; 3])));
        for bad in [
            pick(2, PICK_MOVE, 0, PICK_NONE, PICK_NONE),
            pick(0, PICK_MOVE, 4, PICK_NONE, PICK_NONE),
            pick(0, PICK_OUT_OF_PP, 1, PICK_NONE, PICK_NONE),
            pick(0, PICK_SWITCH, 6, PICK_NONE, PICK_NONE),
            pick(0, 4, 0, PICK_NONE, PICK_NONE),
            pick(0, PICK_MOVE, 0, 4, PICK_NONE),
            pick(0, PICK_MOVE, 0, PICK_NONE, 6),
        ] {
            assert!(!decodes(choice(vec![bad])), "{bad:?}");
        }
        let step = |picks: Vec<(u8, BattlePick)>| Body::BattleStep {
            battle: 1,
            step: 1,
            seed: 0,
            picks,
        };
        assert!(decodes(step(vec![(0, ok), (0, ok), (1, ok), (1, ok)])));
        assert!(!decodes(step(vec![(0, ok); 5])));
        assert!(!decodes(step(vec![(2, ok)])));
        assert!(!decodes(Body::BattleLeave {
            battle: 1,
            reason: LEAVE_LEFT + 1
        }));
        assert!(!decodes(Body::BattleEnd {
            battle: 1,
            reason: END_WITHDRAWN + 1,
            winner: 0
        }));
        assert!(!decodes(Body::BattleEnd {
            battle: 1,
            reason: END_FORFEIT,
            winner: NO_WINNER + 1
        }));
        // A whole team of six fits in one reliable message.
        let start = Message::Game {
            from: SERVER,
            body: Body::BattleStart {
                battle: 1,
                format: 1,
                side: 0,
                opponent: 2,
                seed: 1,
                team: vec![battle_member(); MAX_BATTLE_TEAM],
            },
        };
        assert!(start.encode().len() < 32 * 1100);
    }

    #[test]
    fn partners_stay_in_the_roms_ranges() {
        let some = |ability: u8, level: u8, count: usize| {
            Message::Game {
                from: 1,
                body: Body::Partners {
                    list: vec![
                        SharedPartner {
                            identity: 1,
                            species: 1,
                            types: [0, 0],
                            ability,
                            moves: [1, 0, 0, 0],
                            level,
                        };
                        count
                    ],
                },
            }
            .encode()
        };
        let one = |ability: u8, count: usize| some(ability, 5, count);
        assert!(Message::decode(&one(77, MAX_PARTNERS)).is_ok());
        assert!(Message::decode(&one(78, 1)).is_err());
        assert!(Message::decode(&one(1, MAX_PARTNERS + 1)).is_err());
        assert!(Message::decode(&some(1, 100, 1)).is_ok());
        assert!(Message::decode(&some(1, 0, 1)).is_err());
        assert!(Message::decode(&some(1, 101, 1)).is_err());
    }

    fn wild_creature() -> WildCreature {
        WildCreature {
            species: 25,
            level: 30,
            ability: 9,
            shiny: false,
            moves: [84, 0, 98, 86],
        }
    }

    #[test]
    fn wild_visitors_stay_in_range() {
        let decodes =
            |body: Body| Message::decode(&Message::Game { from: 1, body }.encode()).is_ok();
        let carry = |creature: WildCreature| Body::WildCarry {
            area: 1,
            creature,
            offset: [0.0; 3],
        };
        let c = wild_creature();
        assert!(decodes(carry(c)));
        for bad in [
            WildCreature { species: 0, ..c },
            WildCreature {
                species: crate::gba::rom::SPECIES_COUNT,
                ..c
            },
            WildCreature { level: 0, ..c },
            WildCreature { level: 101, ..c },
            WildCreature {
                ability: crate::gba::rom::ABILITY_COUNT,
                ..c
            },
            WildCreature { moves: [0; 4], ..c },
            WildCreature {
                moves: [84, 84, 0, 0],
                ..c
            },
            WildCreature {
                moves: [crate::gba::rom::MOVE_COUNT, 0, 0, 0],
                ..c
            },
        ] {
            assert!(!decodes(carry(bad)), "{bad:?}");
        }
        // A shiny byte past 1.
        let mut bytes = Message::Game {
            from: 1,
            body: carry(c),
        }
        .encode();
        // tag, from, body tag, area, species u16, level, ability: the shiny byte.
        bytes[8] = 2;
        assert!(Message::decode(&bytes).is_err());
        // Areas, ids, owners, indexes, steps.
        assert!(!decodes(Body::WildCarry {
            area: 4,
            creature: c,
            offset: [0.0; 3]
        }));
        assert!(!decodes(Body::WildUsed { garden: 0, key: 1 }));
        assert!(!decodes(Body::WildTake { id: 0 }));
        assert!(!decodes(Body::WildAbsorbed {
            id: 1,
            owner: 16,
            index: 0
        }));
        assert!(!decodes(Body::WildAbsorbed {
            id: 1,
            owner: 0,
            index: 4
        }));
        // An offset in hands is short; a let-go position can be anywhere in the garden.
        assert!(!decodes(Body::WildHeld {
            id: 1,
            let_go: false,
            v: [0.0, 11.0, 0.0]
        }));
        assert!(decodes(Body::WildHeld {
            id: 1,
            let_go: true,
            v: [0.0, 11.0, 300.0]
        }));
        assert!(!decodes(Body::WildCarry {
            area: 1,
            creature: c,
            offset: [f32::NAN, 0.0, 0.0]
        }));
        assert!(!decodes(Body::WildPut {
            id: 1,
            at: [2.0e6, 0.0, 0.0],
            angle: 0
        }));
        // Hellos: known flags only, a print exactly when seeing.
        assert!(!decodes(Body::WildHello { flags: 8, print: 0 }));
        assert!(!decodes(Body::WildHello {
            flags: WILD_SEE,
            print: 0
        }));
        assert!(!decodes(Body::WildHello {
            flags: WILD_PICK,
            print: 5
        }));
        // Asks of 1..=5; picks of at most 5 with a section; lists of at most 8, ids once.
        let ask = |land: u8, pond: u8| Body::WildAsk {
            area: 1,
            ask: 1,
            section: Some(3),
            land,
            pond,
            fresh: false,
        };
        assert!(decodes(ask(4, 1)));
        assert!(!decodes(ask(0, 0)) && !decodes(ask(5, 1)));
        let pick = |section: u8, n: usize| Body::WildPick {
            area: 1,
            ask: 1,
            section,
            list: vec![
                WildPicked {
                    creature: c,
                    arrive: WildArrive::Land,
                };
                n
            ],
        };
        assert!(decodes(pick(0, MAX_WILD_PICK)));
        assert!(!decodes(pick(0, MAX_WILD_PICK + 1)) && !decodes(pick(0xFF, 1)));
        let entry = |id: u16, place: WildPlace, v: [f32; 3]| WildEntry {
            id,
            creature: c,
            bringer: None,
            place,
            v,
            angle: 0,
        };
        let list = |list: Vec<WildEntry>| Body::WildList {
            area: 1,
            serial: 1,
            section: None,
            print: 1,
            picker: None,
            learners: 0,
            list,
        };
        let many = |n: u16| {
            (1..=n)
                .map(|i| entry(i, WildPlace::Roam, [0.0; 3]))
                .collect()
        };
        assert!(decodes(list(many(MAX_WILD_LIST as u16))));
        assert!(!decodes(list(many(MAX_WILD_LIST as u16 + 1))));
        let one = entry(1, WildPlace::Roam, [0.0; 3]);
        assert!(!decodes(list(vec![one, one])));
        assert!(!decodes(list(vec![entry(
            1,
            WildPlace::Held { by: 8 },
            [0.0; 3]
        )])));
        assert!(!decodes(list(vec![entry(
            1,
            WildPlace::Held { by: 1 },
            [0.0, 20.0, 0.0]
        )])));
        assert!(!decodes(list(vec![WildEntry {
            bringer: Some(MAX_PLAYERS as u8),
            ..one
        }])));
        // Enum bytes past the known ones.
        let mut battle = Message::Game {
            from: 1,
            body: Body::WildBattle {
                id: 1,
                step: WildStep::Cancelled,
            },
        }
        .encode();
        *battle.last_mut().unwrap() = 4;
        assert!(Message::decode(&battle).is_err());
        let mut taught = Message::Game {
            from: SERVER,
            body: Body::WildTaught {
                identity: 1,
                origin: WildOrigin::Partner,
                creature: c,
            },
        }
        .encode();
        taught[11] = 3;
        assert!(Message::decode(&taught).is_err());
        // A full list is small (33 bytes an entry), well inside one unreliable packet.
        let full = Message::Game {
            from: SERVER,
            body: Body::WildList {
                area: 1,
                serial: 2,
                section: Some(1),
                print: 1,
                picker: Some(0),
                learners: 0xFFFF,
                list: many(MAX_WILD_LIST as u16),
            },
        };
        assert_eq!(full.encode().len(), 3 + 12 + MAX_WILD_LIST * 33);
    }

    #[test]
    fn round_trip_every_message() {
        for msg in all_messages() {
            assert_eq!(Message::decode(&msg.encode()), Ok(msg.clone()), "{msg:?}");
        }
    }

    #[test]
    fn chao_locks_follow_the_owners_garden() {
        use ChaoLock::*;
        // Owner in garden 2, offline/outside (0); the Chao lives in garden 2 or 3.
        assert!(!Always.locks(0, 2));
        assert!(!ChaoWorld.locks(3, 2) && ChaoWorld.locks(0, 2));
        // Anywhere else in Chao World counts too, for "Chao World" but not "that garden".
        for area in [0, 4, 5, 6, 9, 10] {
            let place = lock_place(true, area);
            assert!(
                !ChaoWorld.locks(place, 2) && ThatGarden.locks(place, 2),
                "{area}"
            );
        }
        assert_eq!(lock_place(false, 2), 0);
        assert_eq!(lock_place(true, 2), 2);
        assert_eq!(ChaoLock::parse("AnyGarden"), ChaoWorld);
        assert_eq!(ChaoLock::parse("While I'm in Chao World"), ChaoWorld);
        assert!(!ThatGarden.locks(2, 2) && ThatGarden.locks(3, 2) && ThatGarden.locks(0, 2));
        assert!(Never.locks(2, 2));
        for l in ChaoLock::ALL {
            assert_eq!(ChaoLock::from_u8(l.to_u8()), Some(l));
            assert_eq!(ChaoLock::parse(l.ini()), l);
            assert_eq!(ChaoLock::parse(l.label()), l);
        }
        assert_eq!(ChaoLock::from_u8(4), None);
        assert_eq!(ChaoLock::parse("bogus"), Always);
        // A lock past the known ones doesn't read.
        let mut bytes = Message::Game {
            from: 1,
            body: Body::Brought {
                awake: false,
                lock: Never,
                list: Vec::new(),
            },
        }
        .encode();
        let at = bytes.len() - 2;
        assert_eq!(bytes[at], Never.to_u8());
        bytes[at] = 9;
        assert!(Message::decode(&bytes).is_err());
    }

    #[test]
    fn the_hosts_garden_messages_round_trip() {
        for garden in 0..=3 {
            let msg = Message::Game {
                from: 1,
                body: Body::GardenLand {
                    garden,
                    fingerprint: 0x5BF5_52E8,
                    part: 1,
                    parts: 2,
                    bytes: vec![7; 300],
                },
            };
            assert_eq!(Message::decode(&msg.encode()), Ok(msg.clone()), "{msg:?}");
        }
        let msg = Message::Game {
            from: 0xFF,
            body: Body::ServerChao {
                area: 1,
                owner: 2,
                list: vec![GardenChao {
                    index: 1,
                    position: [1.0, 2.0, 3.0],
                    motion: 175,
                    bubble: (2, 30, 180),
                    ..GardenChao::default()
                }],
            },
        };
        assert_eq!(Message::decode(&msg.encode()), Ok(msg));
        // What the host sends as itself: Chao sounds, record changes, item owners, toys.
        for body in [
            Body::ChaoSounds {
                sounds: vec![ChaoSound {
                    owner: 1,
                    index: 0,
                    id: 0x601C,
                    args: [0, 0, 0x6E],
                    ..ChaoSound::default()
                }],
            },
            Body::ChaoChanges {
                owner: 1,
                index: 0,
                changes: vec![(0x20, 5.0)],
            },
            Body::ItemOwner {
                id: ItemId {
                    origin: 1,
                    serial: 4,
                },
                owner: SERVER,
            },
            Body::ToyMode {
                area: 1,
                toy: TOY_TV,
                mode: 1,
            },
            Body::ChaoLive {
                owner: 1,
                index: 0,
                bytes: (0..CHAO_LIVE_LEN as u8).collect(),
            },
            Body::ItemSteer {
                area: 2,
                item: GardenItem {
                    id: ItemId {
                        origin: 1,
                        serial: 9,
                    },
                    category: 3,
                    kind: 4,
                    bites: 2,
                    place: ItemPlace::Chao { owner: 1, index: 0 },
                    position: [0.0, 0.6, 2.0],
                    unsaved: false,
                },
                gone: false,
            },
            Body::ChaoTouched {
                owner: 1,
                index: 2,
                position: [3.0, -1.5, 40.0],
                rotation_y: 0x4000,
                motion: 0x20B,
                mood: vec![9; CHAO_MOOD.1],
            },
        ] {
            let msg = Message::Game { from: SERVER, body };
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
                action: 0,
                status: 0,
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
    fn chao_owners_may_be_held_by_the_host_but_players_may_not() {
        let held = |owner| {
            Message::Game {
                from: SERVER,
                body: Body::ServerChao {
                    area: 1,
                    owner,
                    list: vec![GardenChao::default()],
                },
            }
            .encode()
        };
        for owner in [0, 7, HELD_OWNER, MAX_CHAO_OWNER] {
            assert!(Message::decode(&held(owner)).is_ok(), "{owner}");
        }
        assert!(Message::decode(&held(MAX_CHAO_OWNER + 1)).is_err());
        // Held Chao go in an item's place byte too, and their changes, holds and touches.
        for body in [
            Body::ChaoChanges {
                owner: HELD_OWNER,
                index: 1,
                changes: vec![(0x82, 1.0)],
            },
            Body::ChaoHeld {
                owner: MAX_CHAO_OWNER,
                index: 3,
                offset: [0.0, 2.0, 1.0],
            },
            Body::ServerItems {
                area: 1,
                items: vec![GardenItem {
                    category: 3,
                    place: ItemPlace::Chao {
                        owner: MAX_CHAO_OWNER,
                        index: 3,
                    },
                    ..GardenItem::default()
                }],
            },
        ] {
            let msg = Message::Game { from: 1, body };
            assert_eq!(Message::decode(&msg.encode()), Ok(msg.clone()), "{msg:?}");
        }
        // A player's own fields stay player ids.
        let bad = Message::Game {
            from: 1,
            body: Body::GardenChao {
                owner: HELD_OWNER,
                list: Vec::new(),
            },
        };
        assert!(Message::decode(&bad.encode()).is_err());
        // Only held ids name a held Chao.
        let bad = Message::Game {
            from: SERVER,
            body: Body::HeldChao {
                owner: 3,
                index: 0,
                chao: None,
            },
        };
        assert!(Message::decode(&bad.encode()).is_err());
        // A locked list names real Chao only.
        for chao in [
            vec![(MAX_CHAO_OWNER + 1, 0)],
            vec![(0, MAX_CARRIED_CHAO as u8)],
        ] {
            let bad = Message::Game {
                from: SERVER,
                body: Body::ChaoLocked { chao },
            };
            assert!(Message::decode(&bad.encode()).is_err());
        }
    }

    #[test]
    fn the_host_writes_only_a_chaos_life_into_a_save() {
        assert!(save_field(0x80, 1) && save_field(0x12C, 0x2B) && save_field(0x494, 2));
        // Not its name, garden or DNA (its identity), nor past a field's end.
        assert!(!save_field(0x12, 7) && !save_field(0x81, 1) && !save_field(0x438, 4));
        assert!(!save_field(0x86, 9) && !save_field(0x80, 0));
        let bad = Message::Game {
            from: SERVER,
            body: Body::SaveChao {
                identity: 1,
                nth: 0,
                spans: vec![(0x81, vec![2])],
            },
        };
        assert!(Message::decode(&bad.encode()).is_err());
    }

    #[test]
    fn an_old_hello_has_no_key_and_is_still_read() {
        let mut w = Writer::default();
        w.u8(tag::HELLO)
            .u16(21)
            .str("0.0.1")
            .bytes(&[0; 8])
            .str("Old");
        match Message::decode(&w.buf) {
            Ok(Message::Hello { protocol, key, .. }) => assert_eq!((protocol, key), (21, [0; 16])),
            other => panic!("{other:?}"),
        }
        w.u8(1);
        assert!(Message::decode(&w.buf).is_err());
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

    #[test]
    fn skin_paths_stay_inside_the_skins_folder() {
        assert!(skin_path_ok(
            "my_skin/sounds/happy/05 a longer line - and more (2).wav"
        ));
        assert!(skin_path_ok("my_skin/model.sa2mdl"));
        for bad in [
            "../evil",
            "big/../../evil",
            "big/./x",
            "/etc/passwd",
            "big",
            "big/",
            "Big/x",
            "big//x",
            "big/.hidden",
            "big\\x",
            "big/x:y",
            "big/what?.wav",
            "big/name ",
            "big/name.",
        ] {
            assert!(!skin_path_ok(bad), "{bad}");
        }
        assert!(skin_id_ok("race_car_2"));
        assert!(!skin_id_ok("") && !skin_id_ok("MySkin") && !skin_id_ok("a b"));
        // A list naming a bad path is refused whole.
        let bad = Message::Game {
            from: 0,
            body: Body::SkinList {
                page: 0,
                pages: 1,
                files: vec![SkinFile {
                    path: "../x".into(),
                    size: 1,
                    hash: 0,
                }],
            },
        };
        assert!(Message::decode(&bad.encode()).is_err());
    }
}

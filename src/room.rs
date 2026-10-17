//! Rooms: a player creates a room (F9 > Room) and others join it. Each player's state
//! carries their room (the creator's id); a room exists while its creator is in it. Whoever in
//! the room picks something takes the others who are at the same place along:
//!
//! - Stages (story too): every stage starts together for the room (together.rs).
//! - Stage Select: a pick takes the room's players on Stage Select into the same stage,
//!   mission and character. The pick is noticed when Stage Select's confirm (0x686E00, then
//!   0x6721D5) has written the stage and switched the menu to its leave step (sub-menu 5); a
//!   member gets the same values written and its menu switched to that step, so it leaves like
//!   after a real pick.
//! - Race entrance: a confirmed race takes the room's players there along (below); the start
//!   and live lanes are in race.rs.
//! - Karate: a fight becomes live karate against a room player there (karate_live.rs).

use std::sync::Mutex;

use crate::hud;
use crate::log;

const GAME_MODE: usize = 0x0174_B040;
const NEXT_GAME_MODE: usize = 0x0174_B044;
/// Main loop mode that runs the menus.
const MODE_MENUS: i32 = 0xE;
/// Main loop mode that loads a stage.
const MODE_LOAD_STAGE: i32 = 4;
const CURRENT_MENU: usize = 0x01D7_BB10;
const CURRENT_SUB_MENU: usize = 0x01D7_BB14;
const MENU_STAGE_SELECT: i32 = 3;
/// Stage Select's step after a stage was picked (fade out, then the menus exit).
const SUB_MENU_LEAVING: i32 = 5;

const CURRENT_LEVEL: usize = 0x0193_4B70;
const LAST_LEVEL: usize = 0x0193_4B84;
const CURRENT_CHARACTER: usize = 0x0193_4B80;
const MISSION_NUM: usize = 0x0174_AFE3;
/// Bytes Stage Select's confirm clears before a stage.
const CLEARED_ON_PICK: [usize; 3] = [0x0174_AFE0, 0x0174_AFDE, 0x0174_AFC0];
/// Where the menus return to after the stage (6 = Stage Select).
const MENU_RETURN: usize = 0x01A5_59E8;
const MENU_RETURN_STAGE_SELECT: i32 = 6;
/// Stage Select's working data: two pointers whose fields the confirm clears.
const STAGE_SELECT_STATE: usize = 0x00C7_7F48;
const STAGE_SELECT_STATE2: usize = 0x00C7_7F44;
const MENU_CONTROLS_VISIBLE: usize = 0x01AE_E2FC;

// Chao Race entrance (re/chao-race-sync.md section 1).
const CURRENT_CHAO_AREA: usize = 0x0134_046C;
const AREA_ENTRANCE: i32 = 5;
const RACE_CATEGORY: usize = 0x01A0_FBD3;
const RACE_COURSE: usize = 0x01A0_FBD4;
const RACE_LEVEL: usize = 0x01A0_FBD5;
/// The entrance menu's work struct pointer: +4 = result (1 = confirm), +0xE8 = its object.
const ENTRANCE_WORK: usize = 0x01A5_CC20;
/// Lane 0's Chao record (set once the player chose a Chao for the race).
const LANE0_RECORD: usize = 0x01A5_CC00;
/// Entrance menu states (`Data1[0]` of its object): course menus, confirm, "press A".
const ENTRANCE_COURSE_MENUS: std::ops::RangeInclusive<u8> = 0x11..=0x13;
const ENTRANCE_CONFIRM: u8 = 0x14;
const ENTRANCE_CONFIRMED: u8 = 0x15;
const ENTRANCE_PRESS_A: u8 = 0x16;
/// `GoToChaoArea(area)` 0x52B5B0 (cdecl); area 4 = the race.
const GO_TO_CHAO_AREA: usize = 0x0052_B5B0;
const AREA_RACE: i32 = 4;
/// How long a follower keeps a race pick to apply while they get to the course list.
const RACE_PICK_KEEP: std::time::Duration = std::time::Duration::from_secs(20);
/// Frames to wait at the "press A" prompt before going (the game itself waits 30).
const PRESS_A_WAIT: u32 = 32;

/// A Chao Race picked by a leader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RacePick {
    pub category: u8,
    pub course: u8,
    pub level: u8,
}

/// A room: its creator's player id. The creator leads; members are taken along wherever
/// they are together (stage starts, Stage Select, the race entrance, karate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomId {
    pub owner: u8,
}

/// `PlayerState` encoding of "no room" (owner, kind); a room is (owner, its `Mode`'s kind).
/// Any kind other than 0 is a room, so builds that only know kind 1 still see co-op rooms.
pub const NO_ROOM: (u8, u8) = (0xFF, 0);

/// How a room plays stages, chosen by its creator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Everyone has their own enemies and item boxes.
    #[default]
    Race,
    /// Enemies and item boxes destroyed by anyone are gone for everyone in that stage.
    Coop,
}

impl Mode {
    pub const ALL: [Mode; 2] = [Mode::Race, Mode::Coop];

    fn kind(self) -> u8 {
        match self {
            Mode::Race => 1,
            Mode::Coop => 2,
        }
    }

    fn from_kind(kind: u8) -> Self {
        if kind == 2 { Mode::Coop } else { Mode::Race }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Race => "Race",
            Mode::Coop => "Co-op",
        }
    }

    /// `RoomMode=` in config.ini.
    pub fn parse(text: &str) -> Self {
        match text.trim().to_ascii_lowercase().as_str() {
            "coop" | "co-op" => Mode::Coop,
            _ => Mode::Race,
        }
    }

    pub fn ini(self) -> &'static str {
        match self {
            Mode::Race => "Race",
            Mode::Coop => "Coop",
        }
    }
}

impl RoomId {
    pub fn from_wire(owner: u8, kind: u8) -> Option<Self> {
        (kind != 0 && owner < 8).then_some(Self { owner })
    }
}

/// Our part in our room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Off,
    Lead,
    Follow,
}

/// Another player and their room.
#[derive(Clone, Debug)]
pub struct Other {
    pub id: u8,
    pub name: String,
    pub room: Option<RoomId>,
    /// Their room's `PlayerState` kind (the room's mode, as its creator sets it).
    pub kind: u8,
}

/// A choice in the F9 room row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Leave,
    Create,
    Join(RoomId),
}

/// A stage picked by a leader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub level: i16,
    pub mission: u8,
    pub character: u8,
}

struct Room {
    /// The room we're in, our player id, and the others with their rooms.
    room: Option<RoomId>,
    me: Option<u8>,
    others: Vec<Other>,
    /// We already reported the pick that is on screen now.
    reported: bool,
    /// Our pick, to send.
    outgoing: Option<Pick>,
    /// A leader's pick, to apply (with the leader's name).
    incoming: Option<(Pick, String)>,
    /// Chao Race picks: ours to send, a leader's to apply (with when it arrived).
    race_reported: bool,
    race_outgoing: Option<RacePick>,
    race_incoming: Option<(RacePick, String, std::time::Instant)>,
    /// We confirmed a leader's race for this player: skip the "press A" prompt.
    race_going: Option<u32>,
    /// Our mode setting, used for rooms we create.
    mode: Mode,
}

static ROOM: Mutex<Room> = Mutex::new(Room {
    room: None,
    me: None,
    others: Vec::new(),
    reported: false,
    outgoing: None,
    incoming: None,
    race_reported: false,
    race_outgoing: None,
    race_incoming: None,
    race_going: None,
    mode: Mode::Race,
});

fn lock() -> std::sync::MutexGuard<'static, Room> {
    ROOM.lock().unwrap_or_else(|e| e.into_inner())
}

/// Our part in our room.
fn role(r: &Room) -> Role {
    match r.room {
        Some(room) if Some(room.owner) == r.me => Role::Lead,
        Some(_) => Role::Follow,
        None => Role::Off,
    }
}

fn owner_name(r: &Room, owner: u8) -> String {
    if Some(owner) == r.me {
        "Your".into()
    } else {
        let name = r
            .others
            .iter()
            .find(|o| o.id == owner)
            .map_or("?", |o| o.name.as_str());
        format!("{name}'s")
    }
}

/// Our room for `PlayerState` (owner, kind), or `NO_ROOM`.
pub fn wire() -> (u8, u8) {
    let r = lock();
    r.room
        .map_or(NO_ROOM, |room| (room.owner, mode_of(&r).kind()))
}

/// The mode of the room we're in: ours if we created it, else its creator's.
fn mode_of(r: &Room) -> Mode {
    match role(r) {
        Role::Lead | Role::Off => r.mode,
        Role::Follow => r
            .others
            .iter()
            .find(|o| Some(o.id) == r.room.map(|room| room.owner))
            .map_or(Mode::Race, |o| Mode::from_kind(o.kind)),
    }
}

/// The room's mode right now (`Race` when not in a room).
pub fn mode() -> Mode {
    let r = lock();
    if r.room.is_some() {
        mode_of(&r)
    } else {
        Mode::Race
    }
}

/// Our mode setting (applies to rooms we create; members follow the creator's).
pub fn set_mode(mode: Mode) {
    lock().mode = mode;
}

/// We created the room we're in (only the creator sets its mode).
pub fn is_leader() -> bool {
    role(&lock()) == Role::Lead
}

/// Call every frame with our id and the other players' rooms. Leaves a room whose creator is
/// gone or closed it.
pub fn update(me: Option<u8>, others: Vec<Other>) {
    let mut r = lock();
    let previous = std::mem::replace(&mut r.others, others);
    r.me = me;
    let was_in = |id: u8, room: RoomId| previous.iter().any(|p| p.id == id && p.room == Some(room));
    if me.is_some() {
        // Someone opened a room.
        for o in &r.others {
            let theirs = RoomId { owner: o.id };
            if o.room == Some(theirs) && !was_in(o.id, theirs) && r.room != Some(theirs) {
                hud::toast(format!("{} created a room (F9 > Room to join)", o.name));
            }
        }
        // Players joining and leaving our room (its creator leaving closes it, below).
        if let Some(room) = r.room {
            for o in r.others.iter().filter(|o| o.room == Some(room)) {
                if !was_in(o.id, room) {
                    hud::toast(format!("{} joined the room", o.name));
                }
            }
            for p in previous
                .iter()
                .filter(|p| p.room == Some(room) && p.id != room.owner)
            {
                if !r
                    .others
                    .iter()
                    .any(|o| o.id == p.id && o.room == Some(room))
                {
                    hud::toast(format!("{} left the room", p.name));
                }
            }
            // The creator switched the room between Race and Co-op.
            if room.owner != me.unwrap_or(0xFF) {
                let kind_of = |list: &[Other]| {
                    list.iter()
                        .find(|o| o.id == room.owner && o.room == Some(room))
                        .map(|o| Mode::from_kind(o.kind))
                };
                if let (Some(before), Some(now)) = (kind_of(&previous), kind_of(&r.others))
                    && before != now
                {
                    hud::toast(format!("The room is now {}", now.label()));
                }
            }
        }
    }
    if let Some(room) = r.room {
        let open = Some(room.owner) == me
            || r.others
                .iter()
                .any(|o| o.id == room.owner && o.room == Some(room));
        if !open || me.is_none() {
            let name = previous
                .iter()
                .find(|p| p.id == room.owner)
                .map_or_else(|| "The".to_string(), |p| format!("{}'s", p.name));
            r.room = None;
            reset_picks(&mut r);
            if me.is_some() {
                hud::toast(format!("{name} room closed"));
            }
        }
    }
}

fn reset_picks(r: &mut Room) {
    r.incoming = None;
    r.race_incoming = None;
    r.race_going = None;
}

/// Rooms open right now (created by someone in them), other than ours.
fn open_rooms(r: &Room) -> Vec<RoomId> {
    r.others
        .iter()
        .filter_map(|o| o.room.filter(|room| room.owner == o.id))
        .filter(|room| Some(*room) != r.room)
        .collect()
}

/// The F9 room row's choices with their labels.
pub fn choices() -> Vec<(Choice, String)> {
    let r = lock();
    let mut out = Vec::new();
    if r.me.is_none() {
        return out;
    }
    for room in open_rooms(&r) {
        let count = r.others.iter().filter(|o| o.room == Some(room)).count();
        out.push((
            Choice::Join(room),
            format!("Join {} room ({count})", owner_name(&r, room.owner)),
        ));
    }
    if role(&r) != Role::Lead {
        out.push((Choice::Create, "Create a room".into()));
    }
    if r.room.is_some() {
        out.push((Choice::Leave, "Leave the room".into()));
    }
    out
}

/// Creates, joins or leaves a room.
pub fn choose(choice: Choice) -> String {
    let mut r = lock();
    let Some(me) = r.me else {
        return "Not connected".into();
    };
    reset_picks(&mut r);
    match choice {
        Choice::Leave => {
            r.room = None;
            "Left the room".into()
        }
        Choice::Create => {
            r.room = Some(RoomId { owner: me });
            "Created a room".into()
        }
        Choice::Join(room) => {
            r.room = Some(room);
            format!("Joined {} room", owner_name(&r, room.owner))
        }
    }
}

/// "Sam's room: You, Sam, Alex", while in a room.
pub fn summary() -> Option<String> {
    let r = lock();
    let room = r.room?;
    let mut names = vec!["You".to_string()];
    names.extend(
        r.others
            .iter()
            .filter(|o| o.room == Some(room))
            .map(|o| o.name.clone()),
    );
    Some(format!(
        "{} room: {}",
        owner_name(&r, room.owner),
        names.join(", ")
    ))
}

/// A player's name ("?" if unknown).
pub fn name_of(id: u8) -> String {
    let r = lock();
    if Some(id) == r.me {
        return "You".into();
    }
    r.others
        .iter()
        .find(|o| o.id == id)
        .map_or_else(|| "?".into(), |o| o.name.clone())
}

/// In a room (creator or member).
pub fn in_room() -> bool {
    role(&lock()) != Role::Off
}

fn shares(r: &Room, id: u8) -> bool {
    r.room
        .is_some_and(|room| r.others.iter().any(|o| o.id == id && o.room == Some(room)))
}

/// Player `id` is in our room.
pub fn shares_room(id: u8) -> bool {
    shares(&lock(), id)
}

/// The other players in our room.
pub fn members() -> Vec<u8> {
    let r = lock();
    let Some(room) = r.room else {
        return Vec::new();
    };
    r.others
        .iter()
        .filter(|o| o.room == Some(room))
        .map(|o| o.id)
        .collect()
}

/// Network: a leader picked a Chao Race.
pub fn received_race(pick: RacePick, from: u8, name: &str) {
    let mut r = lock();
    if shares(&r, from) {
        r.race_incoming = Some((pick, name.to_string(), std::time::Instant::now()));
    }
}

/// Our Chao Race pick to send, if we just confirmed a race as leader.
pub fn take_race_outgoing() -> Option<RacePick> {
    ROOM.lock()
        .unwrap_or_else(|e| e.into_inner())
        .race_outgoing
        .take()
}

/// The entrance menu's (work struct, state object Data1), while at the race entrance.
fn entrance_menu() -> Option<(usize, usize)> {
    if read::<i32>(CURRENT_CHAO_AREA) != AREA_ENTRANCE {
        return None;
    }
    let work: usize = read(ENTRANCE_WORK);
    if work == 0 {
        return None;
    }
    let master: usize = read(work + 0xE8);
    if master == 0 {
        return None;
    }
    let data1: usize = read(master + 0x34);
    (data1 != 0).then_some((work, data1))
}

/// Chao Race part of the room: report the leader's race, take followers along.
fn race_frame(r: &mut Room, connected: bool) {
    let menu = entrance_menu();
    let state = menu.map(|(_, data1)| read::<u8>(data1));

    // Leader: the entrance's confirm step has the race in the three bytes.
    let confirmed = matches!(menu, Some((work, _)) if read::<i32>(work + 4) == 1)
        && state == Some(ENTRANCE_CONFIRMED);
    if role(r) != Role::Off && confirmed {
        if !r.race_reported {
            r.race_reported = true;
            let pick = RacePick {
                category: read(RACE_CATEGORY),
                course: read(RACE_COURSE),
                level: read(RACE_LEVEL),
            };
            log!("room: picked race {pick:?}");
            if connected && pick.category <= 4 {
                r.race_outgoing = Some(pick);
                hud::toast("Room: taking everyone along to this race");
            }
        }
    } else if state != Some(ENTRANCE_PRESS_A) {
        r.race_reported = false;
    }

    // Follower: confirm the leader's race once our Chao is chosen and the course list is open.
    if let Some((pick, leader, at)) = r.race_incoming.clone() {
        let ready = menu.is_some_and(|(_, data1)| {
            ENTRANCE_COURSE_MENUS.contains(&read::<u8>(data1)) && read::<usize>(LANE0_RECORD) != 0
        });
        if let (true, Some((work, data1))) = (ready, menu) {
            r.race_incoming = None;
            log!("room: {leader} picked race {pick:?}, going along");
            hud::toast(format!("Room: {leader} picked a race"));
            write(RACE_CATEGORY, pick.category);
            write(RACE_COURSE, pick.course);
            write(RACE_LEVEL, pick.level);
            write(work + 4, 1i32);
            write(data1, ENTRANCE_CONFIRM);
            write(data1 + 1, 0u8);
            r.race_going = Some(0);
        } else if at.elapsed() > RACE_PICK_KEEP {
            r.race_incoming = None;
            // Only worth a message while in Chao World, where the player could go along.
            if read::<i16>(CURRENT_LEVEL) == crate::game::LEVEL_CHAO_WORLD {
                hud::toast(format!(
                    "Room: {leader} started a race (choose your Chao and open the course list to go along)"
                ));
            }
        }
    }

    // Skip the "press A" prompt of a race we confirmed for the leader.
    if let Some(frames) = r.race_going {
        match state {
            Some(ENTRANCE_PRESS_A) if frames >= PRESS_A_WAIT => {
                r.race_going = None;
                log!("room: going to the race");
                // SAFETY: the game's cdecl GoToChaoArea, as the prompt itself calls it.
                unsafe {
                    let go: extern "C" fn(i32) = std::mem::transmute(GO_TO_CHAO_AREA);
                    go(AREA_RACE);
                }
            }
            Some(ENTRANCE_CONFIRM | ENTRANCE_CONFIRMED | ENTRANCE_PRESS_A) => {
                r.race_going = Some(frames + u32::from(state == Some(ENTRANCE_PRESS_A)));
            }
            _ => r.race_going = None,
        }
    }
}

/// Network: a leader picked a stage.
pub fn received(pick: Pick, from: u8, name: &str) {
    let mut r = lock();
    if shares(&r, from) {
        r.incoming = Some((pick, name.to_string()));
    }
}

/// Our pick to send to everyone, if we just picked a stage as leader.
pub fn take_outgoing() -> Option<Pick> {
    ROOM.lock()
        .unwrap_or_else(|e| e.into_inner())
        .outgoing
        .take()
}

fn read<T: Copy>(address: usize) -> T {
    // SAFETY: fixed static addresses in the game's data section.
    unsafe { (address as *const T).read_volatile() }
}

fn write<T>(address: usize, value: T) {
    // SAFETY: fixed static addresses in the game's data section.
    unsafe { (address as *mut T).write_volatile(value) }
}

pub fn on_stage_select() -> bool {
    read::<i32>(GAME_MODE) == MODE_MENUS && read::<i32>(CURRENT_MENU) == MENU_STAGE_SELECT
}

/// Call every frame on the game thread.
pub fn on_frame(connected: bool) {
    let mut r = ROOM.lock().unwrap_or_else(|e| e.into_inner());
    // Nothing is picked into or out of a local two-player game.
    if r.room.is_none() || crate::game::two_player() {
        return;
    }
    race_frame(&mut r, connected);
    if role(&r) == Role::Off {
        return;
    }
    let selecting = on_stage_select();
    let leaving = selecting && read::<i32>(CURRENT_SUB_MENU) == SUB_MENU_LEAVING;

    if leaving && read::<i32>(NEXT_GAME_MODE) == MODE_LOAD_STAGE {
        if !r.reported {
            r.reported = true;
            let pick = Pick {
                level: read(CURRENT_LEVEL),
                mission: read(MISSION_NUM),
                character: read::<i32>(CURRENT_CHARACTER) as u8,
            };
            log!("room: picked {pick:?}, taking the room along");
            // Only real stages (Chao World and menus aren't room picks).
            let stage =
                (1..=100).contains(&pick.level) && pick.level != crate::game::LEVEL_CHAO_WORLD;
            if connected && stage {
                r.outgoing = Some(pick);
                hud::toast("Room: taking everyone along");
            }
        }
    } else {
        r.reported = false;
    }

    if let Some((pick, leader)) = r.incoming.take() {
        if selecting && !leaving {
            log!("room: {leader} picked {pick:?}, going along");
            hud::toast(format!("Room: {leader} picked a stage"));
            go(pick);
        } else {
            log!("room: {leader} picked {pick:?}, but we're not on Stage Select");
            // Only worth a message in the menus (not in the middle of a stage or Chao World).
            if read::<i32>(GAME_MODE) == MODE_MENUS {
                hud::toast(format!(
                    "Room: {leader} started a stage (be on Stage Select to go along)"
                ));
            }
        }
    }

    if selecting && !leaving && connected {
        hud::banner("Room: a stage anyone here picks takes the room along");
    }
}

/// Does what Stage Select does when a stage is picked, with the leader's values.
pub fn go(pick: Pick) {
    write::<i16>(LAST_LEVEL, read(CURRENT_LEVEL));
    write(CURRENT_LEVEL, pick.level);
    write(MISSION_NUM, pick.mission);
    write(CURRENT_CHARACTER, i32::from(pick.character));
    for address in CLEARED_ON_PICK {
        write(address, 0u8);
    }
    write(NEXT_GAME_MODE, MODE_LOAD_STAGE);
    write(MENU_RETURN, MENU_RETURN_STAGE_SELECT);
    for (pointer, fields) in [(STAGE_SELECT_STATE, 2), (STAGE_SELECT_STATE2, 1)] {
        let base: usize = read(pointer);
        if base != 0 {
            for i in 0..fields {
                write(base + 4 * i, 0u32);
            }
        }
    }
    write(MENU_CONTROLS_VISIBLE, 0i32);
    write(CURRENT_SUB_MENU, SUB_MENU_LEAVING);
}

#[cfg(test)]
mod tests {
    use super::{Mode, NO_ROOM, RoomId};

    #[test]
    fn rooms_round_trip_through_player_state() {
        assert_eq!(RoomId::from_wire(3, 1), Some(RoomId { owner: 3 }));
        assert_eq!(RoomId::from_wire(NO_ROOM.0, NO_ROOM.1), None);
        assert_eq!(RoomId::from_wire(9, 1), None);
        // A co-op room is still a room to builds that only know kind 1.
        assert_eq!(
            RoomId::from_wire(3, Mode::Coop.kind()),
            Some(RoomId { owner: 3 })
        );
        for mode in Mode::ALL {
            assert_eq!(Mode::from_kind(mode.kind()), mode);
            assert_eq!(Mode::parse(mode.ini()), mode);
        }
    }
}

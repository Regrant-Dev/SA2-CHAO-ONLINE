//! A garden Chao's mind: the behaviour stack, the think tick and the behaviours, as the game
//! runs them (re/chao-ai.md §1, §2, §5). Behaviours are named by the game's function
//! addresses so they can be checked against the notes.

use super::land::{Garden, V3};
use super::physics::{Body, FACING_WALL, GRAVITY};
use super::picker::{Senses, ToyEvent};
use super::record::{Record, off};

/// The game's `rand()` (MSVC): 0..32767.
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub fn rand(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.0 >> 16) & 0x7FFF
    }

    /// `rand() / 32768`, in 0..1 (the game scales by the double at `0x904E90`).
    pub fn r(&mut self) -> f32 {
        self.rand() as f32 / 32768.0
    }

    /// `trunc(n · r)`, as the game's `n + trunc(-m·r)`-style rolls come out.
    pub fn below(&mut self, n: u32) -> i32 {
        (n as f32 * self.r()) as i32
    }
}

/// A behaviour: a game function the stack can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    /// `0x565BE0`: stand about 61..181 frames, then ask the picker.
    Settle,
    /// `0x562D20`: pick a land cell and install a walk.
    Wander,
    /// `0x55C3C0` / `0x55C430`: stand up before walking.
    StandUp,
    StandUpSlow,
    /// `0x5A3280`, `0x5A3380`, `0x5A3460`, `0x5A3580`: the walks by run points.
    WalkSlow,
    Walk,
    Jog,
    Run,
    /// `0x5A0F40`: a hop off a slope or a ledge.
    Hop,
    /// `0x5630C0`: a flutter for good fliers.
    Glide,
    /// `0x56B740`: turn away from a wall.
    SpinAway,
    /// `0x562D50`: trip over, lie there, get up and carry on.
    Stumble,
    /// `0x5A1020`: thirsty, head for a water cell.
    GoToWater,
    /// Any other game behaviour, by address: run by `acts`, or, if not built yet, the Chao
    /// stands for a while and moves on, so the picker's choices stay the game's.
    Game(u32),
}

impl Behaviour {
    pub fn address(self) -> u32 {
        match self {
            Self::Settle => 0x565BE0,
            Self::Wander => 0x562D20,
            Self::StandUp => 0x55C3C0,
            Self::StandUpSlow => 0x55C430,
            Self::WalkSlow => 0x5A3280,
            Self::Walk => 0x5A3380,
            Self::Jog => 0x5A3460,
            Self::Run => 0x5A3580,
            Self::Hop => 0x5A0F40,
            Self::Glide => 0x5630C0,
            Self::SpinAway => 0x56B740,
            Self::Stumble => 0x562D50,
            Self::GoToWater => 0x5A1020,
            Self::Game(address) => address,
        }
    }
}

/// A turn in a Chao's life the server must act on (the game does these to the save and the
/// garden: evolution `0x568630`, death `0x550620`, rebirth `0x568570`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeEvent {
    /// It went into a cocoon (`0x568D30` started).
    Cocooned,
    /// The cocoon opens: evolve it now (the game's `0x568630`).
    Evolve,
    /// It's gone: its record is cleared.
    Died,
    /// It turns back into an egg (reincarnation count +1, new Chao from its DNA).
    Reborn,
}

/// What a Chao is headed for: its ALW target (`entry+0x44`, set by `0x5304F0` and kept until
/// the next one). The walks and turns to "the linked object" (`0x56BA80`, `0x56B6C0`) read
/// its position live.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    /// A fruit, by the server's id (`Senses::fruit`).
    Fruit(u32),
    /// A garden toy by kind (`Senses::toy`): 2 TV, 3 radio, 8 box.
    Toy(u8),
    /// Something that stays put (a tree).
    At(V3),
}

impl Link {
    /// The toy kind, when it is a toy.
    pub fn toy(self) -> Option<u8> {
        match self {
            Self::Toy(kind) => Some(kind),
            _ => None,
        }
    }
}

/// The fruit in a Chao's hands: a mutual ALW link (`0x56CFB0`), with the fruit's own count
/// of bites left (`fruit D+0x10`) and its bite timer (`D+6`), which the eat behaviour runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hands {
    pub fruit: u32,
    /// Fruit kind (`fruit D+8`), the row of the fruit table.
    pub kind: u8,
    pub bites: u8,
    pub(super) chew: u16,
}

/// What a Chao did with a fruit this frame, for the server to apply to the fruit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FruitEvent {
    /// Took it into its hands (`0x56CFB0`). `player_drops`: player 1 first drops whatever it
    /// holds (`0x46E5E0`, the take-offered path).
    Took { id: u32, player_drops: bool },
    /// A bite (`0x545C20`): `left` bites remain; at 0 the fruit is eaten up and goes away.
    Bite { id: u32, left: u8 },
    /// Let go of it (`0x530690`); `vel`: thrown away with this velocity (eat modes 9/10).
    Dropped { id: u32, vel: Option<V3> },
}

/// The ledge climb (`0x562EB0`, acts/swim.rs).
const CLIMB: u32 = 0x562EB0;

/// What a Chao does with an item, for the server to carry out (the items are its).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemEvent {
    /// Takes the item of this touch kind it touches: player 1 drops it if held (`0x46E5E0`)
    /// and it goes into the Chao's hands (`0x56CFB0`).
    Take(u8),
    /// Lets go of what it holds where it is (`0x530690`).
    LetGo,
    /// Throws what it holds with this velocity and lets go (`0x569550`).
    Throw([f32; 3]),
    /// Has put on the hat it held (now its `rec+0xD5`): the hat's record is freed and the
    /// hat deleted (`0x569430`, `0x52F680`).
    Wear,
    /// Its hat fell off (`0x54A540`): a hat of `kind` at the Chao with its velocity, facing
    /// `rot_y`, saved in the free slot.
    HatOff { kind: u8, rot_y: u16, vel: [f32; 3] },
}

/// What a Chao did to the garden's trees this frame, for the server to apply (the trees are
/// the trees' keeper's, re/chao-garden-sync.md "Shared trees").
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TreeEvent {
    /// Planted the seed in its hands (`0x567B10` mode 3): the seed's save record is freed
    /// (`0x52F680`) and the seed deleted (MainSub `0x46F720`) either way. With `spot_pos`
    /// (the `Senses::plant_spot` it touched then), that empty spot gets the seed's facing
    /// (`Data1+0xC`) and ALW mailbox `+0x34 = 5`, `+0x36 = seed_kind`, so its GrowTree
    /// (mode 0, `0x530710`) becomes a seed of tree kind `seed_kind + 1`. Without one the
    /// seed is just gone.
    Plant { spot_pos: Option<V3>, seed_kind: u8 },
    /// Poured water (`0x568460`): a free-standing 0xAD sphere (`0x55A850`, not the Chao's
    /// `D+0x40` signal) at `pos`, `radius` wide, for `frames` frames. A tree touching it
    /// (`0x54B1F0(0xAD)` in GrowTree modes 1, 4, 7) has its life `Data1+0x3C` set to 10000:
    /// a seed sprouts (mode 2), a growing or grown tree is refreshed.
    Water { pos: V3, radius: f32, frames: u16 },
}

/// What a behaviour asks of the think tick (`0x53DC40`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Next {
    Keep,
    Pop,
    Choose,
}

/// A signal a Chao gives off (`0x55A920`): a sphere of `kind` that Chao touching it sense
/// (`0x486880`), the sender included, while it lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitted {
    pub kind: u8,
    pub pos: V3,
    pub radius: f32,
    /// Frames left (task `+6`), counted down after every frame, gone after the one at 0;
    /// -1: until the behaviour changes (`+1` = 0).
    pub frames: i32,
    /// It's the one at `D+0x40`, deleted when a behaviour is set (`0x53D760`). An older one
    /// lives out its frames.
    pub(super) current: bool,
}

/// Stack slots (`D+0x43C[16]`, `D+0x47C[16]`); pushes stop at 15.
const SLOTS: usize = 16;

/// Motion ids the brain sets (the client plays them).
pub mod motion {
    pub const WALK_SLOW: u16 = 0x73;
    pub const WALK: u16 = 0x64;
    pub const JOG: u16 = 0x65;
    pub const RUN: u16 = 0x66;
    pub const HOP: u16 = 0x8A;
    pub const GLIDE: u16 = 0x82;
    pub const TRIP: u16 = 0x9F;
    pub const GET_UP: u16 = 0xA0;
}

#[derive(Clone, Debug)]
pub struct Chao {
    pub body: Body,
    pub rec: Record,
    /// The motion it plays (`D+0xAC` table, id at `+8`).
    pub motion: u16,
    pub(super) stack: [Option<Behaviour>; SLOTS],
    pub(super) limits: [i32; SLOTS],
    pub(super) count: usize,
    pub(super) index: usize,
    /// The behaviour before the last set or pop (`D+0x430`).
    pub(super) previous: Option<Behaviour>,
    /// The behaviour's sub-state and counters (`D+0x1B6`, `D+0x1C0`, `D+0x1C4`).
    pub(super) mode: u16,
    pub(super) count_a: i32,
    pub(super) count_b: i32,
    /// Frames since its motion started, for the end pulse.
    pub(super) player: super::motion::MotionPlayer,
    /// Frames alive (`D+0x30`) and the walk step's counter (`D+6`).
    pub(super) frame: u32,
    pub(super) steps: u16,
    /// Think timers `D+0x1D4..=0x210`, counted down every frame (`0x562C00`).
    pub(super) timers: [i32; 16],
    /// Face: eyes and mouth ids (`D+0x692`, `D+0x69E`) and their timers (-1 until changed).
    pub eyes: i16,
    pub mouth: i16,
    pub(super) eyes_left: i32,
    pub(super) mouth_left: i32,
    /// The emotion ball (`D+0x6EC`): current state, its frames left, queued state and length.
    pub bubble: (i16, i32),
    pub(super) bubble_next: Option<(i16, i32)>,
    /// The current emotion ball's whole length (`D+0x6F4`); `bubble.1` counts down from it.
    pub bubble_length: i32,
    /// Sounds made this frame, for the players to hear (taken by the server each frame).
    pub sounds: Vec<u16>,
    /// What the current behaviour keeps between frames beyond its mode and counters
    /// (`D+0x1B8`, `D+0x1BC`, `D+0x1C8`, ...), by name of use.
    pub(super) phase: u16,
    pub(super) patience: i32,
    pub(super) memo: [f32; 4],
    /// Set by a behaviour; taken by the server.
    pub life_event: Option<LifeEvent>,
    /// Held by a player (Status `D+4 & 0x8000`); set by the server. The brain clears it when
    /// the Chao wriggles free (`0x46E5E0` in `0x565DC0`): the server then empties the hands.
    pub held: bool,
    /// What it did with items this frame (taken by the server).
    pub items: Vec<ItemEvent>,
    /// A held Chao's turn (`W+0x18` target, `W+0x1C` now), set by the held check `0x566720`
    /// and eased by `0x565DC0`; kept across the held behaviours.
    pub(super) held_turn: [u16; 2],
    /// The garden it is in (`*0x134046C`: 1 Neutral, 2 Hero, 3 Dark), set every frame.
    pub(super) area: u8,
    /// Water flags (`D+0x1B4`): 1 went in, 4 in the water this frame (`0x561630`). Kept
    /// across behaviours, unlike the per-behaviour state.
    pub(super) water: u16,
    /// The ALW target (`entry+0x44`): what `0x56BA80`/`0x56B6C0` walk to and face. Kept
    /// across behaviours, like the game's.
    pub(super) link: Option<Link>,
    /// The fruit it holds, if any.
    pub hands: Option<Hands>,
    /// Its ALW mailbox (`entry+0x34`): 4 the fruit in its hands is finished, 6 it was taken
    /// away. Read and cleared by the eat behaviour.
    pub(super) mail: u16,
    /// Fruit taken, bitten or let go this frame (taken by the server, like `sounds`).
    pub fruit_events: Vec<FruitEvent>,
    /// The toy it is linked with both ways (`entry+0x40`, `0x530640`): the horse (7). The
    /// toy's partner is the last Chao to link with it (`ToyEvent::Link`); cleared when an
    /// interrupt fires (`0x530690` at `0x53DC1E`).
    pub toy_link: Option<u8>,
    /// What it did to the garden's toys this frame (taken by the server, like `sounds`).
    pub toy_events: Vec<ToyEvent>,
    /// What it did to the garden's trees this frame (taken by the server, like `sounds`).
    pub tree_events: Vec<TreeEvent>,
    /// Toys and instruments in its hands, by attach node 4, 11 and 35 (`garden::PROP_NODES`):
    /// the model's game address (`0x5669B0`), 0 none. Hidden on every set (`0x53D760`).
    pub props: [u32; 3],
    /// The drum in front of it (`D+0x660` model set, `+0x66C` 1.0; `0x53D760` zeroes `+0x66C`).
    pub drum: bool,
    /// The picture book in front of it (`D+0x38`, made by `0x55E180`): the mode byte the Chao
    /// last wrote (0 new, 2 turn a page, 4 close); deleted on every set (`0x53D760`).
    pub book: Option<u8>,
    /// `D+0xA0 & 2`: physics moves it (`0x796780`, `0x54FE78`); set again on every set.
    pub(super) moves: bool,
    /// Signals it gives off now (`0x55A920`; kinds 0x97, 0xA5..0xA9), for the server to
    /// deliver to the others (a Chao senses its own itself).
    pub signals: Vec<Emitted>,
    /// The song it sings (`D+0x41C`, written after `0x5AB2C0` is set; zeroed by every set):
    /// the song object `0x5AB200` carries it.
    pub song: Option<u8>,
    /// LONELY change for the performer whose signal of this kind it joined this frame
    /// (`0x59D0D0`, `0x59C3D0` write the sender's `rec+0x13E`); taken by the server.
    pub performer_lonely: Option<(u8, i32)>,
}

impl Chao {
    /// A Chao standing at `pos` (CreateChao, then the stack init `0x53DA80`).
    pub fn new(pos: [f32; 3], rot_y: u16, rec: Record) -> Self {
        let mut chao = Self {
            body: Body::new(pos, rot_y),
            rec,
            motion: 0,
            stack: [None; SLOTS],
            limits: [0; SLOTS],
            count: 0,
            index: 0,
            previous: None,
            mode: 0,
            count_a: 0,
            count_b: 0,
            player: super::motion::MotionPlayer::new(None),
            frame: 0,
            steps: 0,
            timers: [0; 16],
            eyes: 0,
            mouth: 0,
            eyes_left: -1,
            mouth_left: -1,
            bubble: (0, 0),
            bubble_next: None,
            bubble_length: 0,
            sounds: Vec::new(),
            phase: 0,
            patience: 0,
            memo: [0.0; 4],
            life_event: None,
            held: false,
            items: Vec::new(),
            held_turn: [0; 2],
            area: 0,
            water: 0,
            link: None,
            hands: None,
            mail: 0,
            fruit_events: Vec::new(),
            toy_link: None,
            toy_events: Vec::new(),
            tree_events: Vec::new(),
            props: [0; 3],
            drum: false,
            book: None,
            moves: true,
            signals: Vec::new(),
            song: None,
            performer_lonely: None,
        };
        chao.set(Behaviour::Settle, -1);
        chao
    }

    pub fn current(&self) -> Option<Behaviour> {
        self.stack.get(self.index).copied().flatten()
    }

    /// One frame: think (`0x53DC40`), then move (`0x796780`, `0x53DF00`).
    pub fn frame(
        &mut self,
        garden: &Garden,
        garden_index: usize,
        senses: &dyn Senses,
        rng: &mut Rng,
    ) {
        self.frame = self.frame.wrapping_add(1);
        self.area = garden_index as u8 + 1;
        self.think(garden, senses, rng);
        self.face_and_ball();
        // The motion advances after the AI (`0x7938D0`), so behaviours see last frame's pulses.
        self.player.advance();
        self.motion = self.player.shown() as u16;
        if self.moves {
            self.body.step(garden, garden_index);
        } else {
            self.body.step_unmoved(garden, garden_index);
        }
        // The signal tasks' own frame (`0x55A6A0`).
        self.signals.retain_mut(|s| match s.frames {
            0 => false,
            f if f > 0 => {
                s.frames -= 1;
                true
            }
            _ => true,
        });
    }

    /// `AL_SetBehavior` (`0x53D890`): the stack becomes just `b`.
    pub(super) fn set(&mut self, b: Behaviour, limit: i32) {
        self.reset_state();
        self.previous = self.current();
        // `0x53D760`: a different behaviour starts with the default face.
        if self.previous != Some(b) {
            self.set_eyes(0, -1);
            self.set_mouth(0, -1);
        }
        self.clean_up();
        self.stack = [None; SLOTS];
        self.stack[0] = Some(b);
        self.limits[0] = limit;
        self.count = 1;
        self.index = 0;
    }

    /// `0x53D970`: queue `b` after the current one.
    pub(super) fn push(&mut self, b: Behaviour) {
        if self.count < SLOTS - 1 {
            self.stack[self.count] = Some(b);
            self.limits[self.count] = -1;
            self.count += 1;
        }
    }

    /// `0x53D9B0`: on to the next queued behaviour; false when there's none. (Like the game,
    /// a push after a pop writes at `count`, which can be a slot in use.)
    pub(super) fn pop(&mut self) -> bool {
        self.reset_state();
        self.previous = self.current();
        self.count = self.count.saturating_sub(1);
        self.index += 1;
        self.current().is_some()
    }

    /// `0x53DA00`: swap the previous behaviour back in and start it over.
    pub(super) fn resume_previous(&mut self) {
        let now = self.current();
        if let Some(slot) = self.stack.get_mut(self.index) {
            *slot = self.previous;
        }
        self.previous = now;
        self.reset_state();
        self.clean_up();
    }

    /// What `0x53D760` puts away on every set: hand props (`0x566AF0`), the drum (`D+0x66C`
    /// = 0), the book (`D+0x38`); physics back on (`D+0xA0 |= 2`).
    fn clean_up(&mut self) {
        self.props = [0; 3];
        self.drum = false;
        self.book = None;
        self.moves = true;
        // The signal at `D+0x40` (`0x53D760`) and the song (`D+0x41C`, zeroed by the set).
        self.signals.retain(|s| !s.current);
        self.song = None;
    }

    /// Gives off a signal (`0x55A920`; `frames` ≤ 0: until the behaviour changes). It becomes
    /// the one at `D+0x40`; the one before stays until its frames run out.
    pub(super) fn emit(&mut self, kind: u8, pos: V3, radius: f32, frames: i32) {
        for s in &mut self.signals {
            s.current = false;
        }
        self.signals.push(Emitted {
            kind,
            pos,
            radius,
            frames: if frames > 0 { frames } else { -1 },
            current: true,
        });
    }

    pub(super) fn reset_state(&mut self) {
        self.mode = 0;
        self.count_a = 0;
        self.count_b = 0;
        self.phase = 0;
        self.patience = 3600;
    }

    /// Eyes for `frames` (-1: until changed) (`0x53A4B0`).
    pub(super) fn set_eyes(&mut self, id: i16, frames: i32) {
        self.eyes = id;
        self.eyes_left = frames;
    }

    /// Mouth for `frames` (-1: until changed) (`0x53A5A0`).
    pub(super) fn set_mouth(&mut self, id: i16, frames: i32) {
        self.mouth = id;
        self.mouth_left = frames;
    }

    /// Queues an emotion-ball state for `frames` (`0x53D660`), with its sound.
    pub(super) fn emotion_ball(&mut self, state: i16, frames: i32) {
        self.bubble_next = Some((state, frames));
        self.sounds.push(0x100E);
    }

    /// A sound at the Chao (`0x4372E0`; the game plays it only in the active garden).
    pub(super) fn sound(&mut self, id: u16) {
        self.sounds.push(id);
    }

    /// Face timers and the emotion ball, every frame (`0x53A3E0`, `0x53CA40`, simplified:
    /// a queued state replaces the current one at once).
    fn face_and_ball(&mut self) {
        for (left, part) in [
            (&mut self.eyes_left, &mut self.eyes),
            (&mut self.mouth_left, &mut self.mouth),
        ] {
            if *left > 0 {
                *left -= 1;
                if *left == 0 {
                    *part = 0;
                    *left = -1;
                }
            }
        }
        if let Some(next) = self.bubble_next.take() {
            self.bubble = next;
            self.bubble_length = next.1;
        } else if self.bubble.1 > 0 {
            self.bubble.1 -= 1;
            if self.bubble.1 == 0 {
                self.bubble.0 = 0;
            }
        }
    }

    /// Blends into motion `id` (`0x793C40`).
    pub(super) fn play(&mut self, id: u16) {
        self.motion = id;
        self.player.set_link(i32::from(id));
    }

    /// Blends into motion `id` over `frames` frames (`0x793D30`).
    pub(super) fn play_blend(&mut self, id: u16, frames: u16) {
        self.motion = id;
        self.player.set_blend(i32::from(id), frames);
    }

    /// Blends into motion `id` at the phase the current one is at (`0x793D90`).
    pub(super) fn play_phase(&mut self, id: u16) {
        self.motion = id;
        self.player.set_keep_phase(i32::from(id));
    }

    /// Straight into motion `id`, no blend (`0x793E40`).
    pub(super) fn play_now(&mut self, id: u16) {
        self.motion = id;
        self.player.set(i32::from(id));
    }

    /// The motion's end pulse (`D+0xAC & 1`).
    pub(super) fn motion_done(&self) -> bool {
        self.player.ended()
    }

    /// The hold pulse (`D+0xAC & 8`): a holding motion has reached its end.
    pub(super) fn motion_held(&self) -> bool {
        self.player.cycled()
    }

    /// The posture class (`D+0xAE`): 0 standing, 1 sitting, 2 lying, 3 on the belly, 4/5
    /// other. Without the game's motion table, from the motion's id.
    pub(super) fn posture(&self) -> u16 {
        if self.player.table().is_some() {
            self.player.posture()
        } else {
            super::motion::fallback_posture(self.motion)
        }
    }

    /// Whether the game's motion table is in (else motions are the 60-frame stand-in).
    pub(super) fn has_motions(&self) -> bool {
        self.player.table().is_some()
    }

    /// Gives this Chao the game's motion table.
    pub fn set_motions(&mut self, table: Option<std::sync::Arc<super::motion::MotionTable>>) {
        self.player.set_table(table);
    }

    /// The think tick (`0x53DC40`, normal path).
    fn think(&mut self, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) {
        for t in &mut self.timers {
            if *t > 0 {
                *t -= 1;
            }
        }
        super::acts::interrupts(self, garden, senses, rng);
        let next = match self.current() {
            Some(b) => self.run(b, garden, senses, rng),
            None => Next::Pop,
        };
        let i = self.index;
        if let Some(limit) = self.limits.get_mut(i).filter(|l| **l > 0) {
            *limit -= 1;
            if *limit == 0 {
                self.pop_or_settle();
                return;
            }
        }
        match next {
            Next::Keep => {}
            Next::Choose => self.choose(garden, senses, rng),
            Next::Pop => self.pop_or_settle(),
        }
    }

    pub(super) fn pop_or_settle(&mut self) {
        if !self.pop() {
            self.set(Behaviour::Settle, -1);
        }
    }

    fn run(&mut self, b: Behaviour, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) -> Next {
        match b {
            Behaviour::Settle => self.settle(rng),
            Behaviour::Wander => self.go_to(1, garden, rng),
            Behaviour::StandUp | Behaviour::StandUpSlow => Next::Pop,
            Behaviour::WalkSlow => self.walk(0x73, 0.038, None, garden, senses, rng),
            Behaviour::Walk => self.walk(0x64, 0.055, Some((85, 1, 0.03)), garden, senses, rng),
            Behaviour::Jog => {
                let k = 0.028 + 7e-6 * f32::from(self.rec.scaled(off::RUN_POINTS));
                self.walk(0x65, k, Some((100, 0, 0.02)), garden, senses, rng)
            }
            Behaviour::Run => {
                let k = 0.028 + 7e-6 * f32::from(self.rec.scaled(off::RUN_POINTS));
                self.walk(0x66, k, None, garden, senses, rng)
            }
            Behaviour::Hop => self.hop(),
            Behaviour::Glide => self.glide(rng),
            Behaviour::SpinAway => self.spin_away(rng),
            Behaviour::Stumble => self.stumble(),
            Behaviour::GoToWater => self.go_to(9, garden, rng),
            Behaviour::Game(address) => super::acts::run(self, address, garden, senses, rng)
                .unwrap_or_else(|| self.unbuilt()),
        }
    }

    /// `0x565BE0`.
    fn settle(&mut self, rng: &mut Rng) -> Next {
        if self.mode == 0 {
            // Sitting or lying (posture 1..3) gets up with 0x3E; the posture isn't known here
            // until the motion table comes in, so standing is assumed.
            let r = rng.r();
            self.play(match r {
                r if r < 0.15 => 3,
                r if r < 0.5 => 0,
                r if r < 0.6 => 0xEE,
                r if r < 0.7 => 0xEF,
                _ => 1,
            });
            if r < 0.15 {
                self.emotion_ball(3, 180);
                self.sound(0x601C);
            }
            self.set_eyes(0, -1);
            self.set_mouth(0, -1);
            self.count_a = 60 + rng.below(121);
            self.mode = 1;
        }
        let t = self.count_a;
        self.count_a -= 1;
        if t > 0 {
            if self.body.on_ground() {
                for v in &mut self.body.vel {
                    *v *= 0.8;
                }
            }
            Next::Keep
        } else {
            Next::Choose
        }
    }

    /// Stands about, then moves on (see `Behaviour::Game`).
    fn unbuilt(&mut self) -> Next {
        if self.mode == 0 {
            self.play(0);
            self.count_a = 300;
            self.mode = 1;
        }
        self.count_a -= 1;
        if self.count_a > 0 {
            Next::Keep
        } else {
            Next::Pop
        }
    }

    /// `0x562D20` (land, cell value 1) and `0x5A1020` (water, 9): a random point in such a
    /// cell, then the walk for its run points (`0x5A3650`).
    fn go_to(&mut self, cell: i32, garden: &Garden, rng: &mut Rng) -> Next {
        if let Some((x, z)) = random_cell_point(garden, cell, rng) {
            self.body.target[0] = x;
            self.body.target[2] = z;
        }
        let run = self.rec.scaled(off::RUN_POINTS);
        let (stand, walk) = match run {
            r if r < 150 => (Behaviour::StandUpSlow, Behaviour::WalkSlow),
            r if r < 2000 => (Behaviour::StandUp, Behaviour::Walk),
            r if r < 4000 => (Behaviour::StandUp, Behaviour::Jog),
            _ => (Behaviour::StandUp, Behaviour::Run),
        };
        self.set(stand, -1);
        self.push(walk);
        Next::Keep
    }

    /// The four walks: their motion and drive, an occasional stumble (`every`, `at`,
    /// chance), then the walk step `0x5A3000`.
    fn walk(
        &mut self,
        id: u16,
        k: f32,
        stumble: Option<(i32, i32, f32)>,
        garden: &Garden,
        senses: &dyn Senses,
        rng: &mut Rng,
    ) -> Next {
        if self.mode == 0 {
            self.play(id);
            self.mode = 1;
        }
        if let Some((every, at, chance)) = stumble {
            let old = self.count_a;
            self.count_a += 1;
            if old % every == at && rng.r() < chance {
                self.set(Behaviour::Stumble, -1);
            }
        }
        self.body.drive(k);
        self.walk_step(garden, senses, rng)
    }

    /// `0x5A3000`. The steps that change behaviour don't return, so several can fire in a
    /// frame and the last wins.
    fn walk_step(&mut self, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) -> Next {
        if self.body.target_distance2() < 36.0 {
            return Next::Pop;
        }
        // Into a wall: climb it at a ledge (0xCB, `0x486760`), else turn away.
        if self.body.flags & FACING_WALL != 0 {
            if senses.touch(0xCB).is_some() {
                self.set(Behaviour::Game(CLIMB), -1);
                self.body.pos[1] += 0.1;
            } else {
                self.set(Behaviour::SpinAway, -1);
            }
        }
        // After 30 steps, a pillar, fence or grave (0xCC, `0x54B1F0`) turns it away too.
        let old = self.steps;
        self.steps = self.steps.wrapping_add(1);
        if old > 30 && senses.touch(0xCC).is_some() {
            self.set(Behaviour::SpinAway, -1);
        }
        self.body.turn_to_target(0x120);
        if self.body.on_ground() {
            if self.body.floor_normal[1] < 0.7 {
                self.body.vel[1] += 0.55;
                self.set(Behaviour::Hop, -1);
            }
        } else if self.body.pos[1] - self.body.floor_y > 2.0 {
            self.body.vel[1] += 0.55;
            let target = self.body.target;
            let water_target = garden.cell_at(target[0], target[2]) == Some(9);
            let flyer = self.rec.scaled(off::FLY_POINTS) >= 400;
            if water_target || !flyer || rng.r() < 0.6 {
                self.set(Behaviour::Hop, -1);
            } else {
                self.set(Behaviour::Glide, -1);
            }
        }
        if self.rec.need_latch() {
            self.rec.add_need(off::TEDIOUS, -100);
            self.rec.add_need(off::TIRE, 300);
        }
        Next::Keep
    }

    /// `0x5A0F40`.
    fn hop(&mut self) -> Next {
        if self.mode == 0 {
            self.play(motion::HOP);
            let (s, c) = self.body.facing();
            self.body.vel = [s * 0.4, 0.35, c * 0.4];
            self.mode = 1;
            self.count_a = 0;
        } else if self.body.on_ground() {
            self.set(Behaviour::Wander, -1);
        }
        self.body.accel[1] = -GRAVITY * 0.45;
        Next::Keep
    }

    /// `0x5630C0`.
    fn glide(&mut self, rng: &mut Rng) -> Next {
        if self.mode == 0 {
            self.play(motion::GLIDE);
            let (s, c) = self.body.facing();
            self.body.vel = [s * 0.4, 0.35, c * 0.4];
            self.count_a = 300 + rng.below(301);
            self.mode = 1;
        } else if self.body.on_ground() {
            self.set(Behaviour::Wander, -1);
            return Next::Keep;
        }
        self.body.turn_to_target(0x100);
        let (s, c) = self.body.facing();
        self.body.accel[0] = s * 0.05 - self.body.vel[0] * 0.1;
        self.body.accel[2] = c * 0.05 - self.body.vel[2] * 0.1;
        if self.count_a > 0 {
            self.count_a -= 1;
            self.body.accel[1] = -GRAVITY * 0.95;
        } else {
            self.body.accel[1] = -GRAVITY * 0.92;
        }
        if self.body.pos[1] > 100.0 {
            self.body.pos[1] = 100.0;
        }
        Next::Keep
    }

    /// `0x56B740`: ±0x16C BAMS a frame for 45..90 frames.
    fn spin_away(&mut self, rng: &mut Rng) -> Next {
        if self.mode == 0 {
            self.play(motion::WALK);
            self.mode = if rng.r() < 0.5 { 1 } else { 2 };
            self.count_a = 45 + rng.below(46);
        }
        self.count_a -= 1;
        if self.count_a <= 0 {
            return Next::Pop;
        }
        let turn: u16 = 0x16C;
        self.body.rot_y = if self.mode == 1 {
            self.body.rot_y.wrapping_add(turn)
        } else {
            self.body.rot_y.wrapping_sub(turn)
        };
        Next::Keep
    }

    /// `0x562D50`: one counter through modes 1 and 2, then get up and resume (`0x53DA00`).
    fn stumble(&mut self) -> Next {
        if self.mode == 0 {
            self.play(motion::TRIP);
            self.count_a = 0;
            self.mode = 1;
        }
        match self.mode {
            1 => {
                let old = self.count_a;
                self.count_a += 1;
                if old > 60 {
                    self.mode = 2;
                } else {
                    self.body.drive(0.055);
                }
            }
            2 => {
                let old = self.count_a;
                self.count_a += 1;
                if old > 300 {
                    self.play(motion::GET_UP);
                    self.mode = 3;
                }
            }
            _ => {
                if self.motion_done() {
                    self.resume_previous();
                }
            }
        }
        Next::Keep
    }
}

/// `0x534F80`: a random point in a random cell of value `value` (y not set).
pub fn random_cell_point(garden: &Garden, value: i32, rng: &mut Rng) -> Option<(f32, f32)> {
    use super::land::GRID_COLS;
    let cells: Vec<usize> = (0..garden.grid.len())
        .filter(|&i| garden.grid[i] == value)
        .collect();
    if cells.is_empty() {
        return None;
    }
    let i = ((rng.r() * (cells.len() as f32 - 1e-5)) as usize).min(cells.len() - 1);
    let (col, row) = (cells[i] % GRID_COLS, cells[i] / GRID_COLS);
    let x = (rng.r() + col as f32 - 16.0) * 10.0;
    let z = (rng.r() + row as f32 - 20.0) * 10.0;
    Some((x, z))
}

#[cfg(test)]
mod tests {
    use super::super::land::{LAND_EXPORT, PeFile};
    use super::*;

    #[test]
    fn the_rng_is_msvcs() {
        let mut rng = Rng::new(1);
        assert_eq!([rng.rand(), rng.rand(), rng.rand()], [41, 18467, 6334]);
    }

    #[test]
    fn a_new_chao_settles_then_wanders() {
        use super::super::land::Tri;
        // A floor at y = 0 (with none it would fall into the water).
        let garden = Garden {
            tris: vec![
                Tri {
                    v: [[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [-50.0, 0.0, 50.0]],
                    flags: 1,
                },
                Tri {
                    v: [[50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [-50.0, 0.0, 50.0]],
                    flags: 1,
                },
            ],
            grid: vec![1; super::super::land::GRID_CELLS],
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        };
        // A child: an adult with no life left would lie down to die once it stands still.
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, super::super::record::TYPE_CHILD);
        let mut chao = Chao::new([0.0; 3], 0, rec);
        let mut rng = Rng::new(7);
        assert_eq!(chao.current(), Some(Behaviour::Settle));
        let mut seen = Vec::new();
        for _ in 0..400 {
            chao.frame(&garden, 0, &super::super::picker::Alone, &mut rng);
            if seen.last() != chao.current().as_ref() {
                seen.extend(chao.current());
            }
        }
        // Settle → wander → stand up → (pop) slow walk (run points 0).
        assert_eq!(
            &seen[..4],
            &[
                Behaviour::Settle,
                Behaviour::Wander,
                Behaviour::StandUpSlow,
                Behaviour::WalkSlow
            ]
        );
    }

    /// A garden's SET objects as the game has them in memory: `_s` then `_u` entries, swapped
    /// from the files' big-endian (`0x487FC0`; re/garden-collision.md §8.3).
    struct Set(Vec<u8>);

    impl super::super::land::Mem for Set {
        fn bytes(&self, addr: u32, len: usize) -> Option<&[u8]> {
            let o = addr.checked_sub(0x1000)? as usize;
            self.0.get(o..o + len)
        }
    }

    fn installed_set(name: &str) -> (Set, u32) {
        let dir = "/mnt/SSD1/SteamLibrary/steamapps/common/Sonic Adventure 2/resource/gd_PC/";
        let mut out = Vec::new();
        for part in ["_s", "_u"] {
            let Ok(d) = std::fs::read(format!("{dir}set_chao_{name}{part}.bin")) else {
                continue;
            };
            let count = u32::from_be_bytes(d[0..4].try_into().unwrap()) as usize;
            for k in 0..count {
                let e = &d[0x20 + k * 0x20..0x40 + k * 0x20];
                for half in e[..8].chunks(2) {
                    out.extend([half[1], half[0]]);
                }
                for word in e[8..].chunks(4) {
                    out.extend(word.iter().rev());
                }
            }
        }
        let count = (out.len() / 0x20) as u32;
        (Set(out), count)
    }

    /// What a Chao touches: the garden's ledges where it is.
    struct Ledges<'a>(&'a [super::super::land::Ledge], super::super::land::V3);

    impl Senses for Ledges<'_> {
        fn touch(&self, kind: u8) -> Option<super::super::land::V3> {
            (kind == 0xCB)
                .then(|| self.0.iter().find(|l| l.touches(self.1)).map(|l| l.pos))
                .flatten()
        }
    }

    #[test]
    fn chao_roam_the_installed_gardens() {
        let (Ok(dll), Ok(exe)) = (
            std::fs::read(
                "/mnt/SSD1/SteamLibrary/steamapps/common/Sonic Adventure 2/resource/gd_PC/DLL/Win32/Data_DLL_orig.dll",
            ),
            std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/re/bin/sonic2app.dumped.exe"
            )),
        ) else {
            return;
        };
        let (dll, exe) = (PeFile::parse(dll).unwrap(), PeFile::parse(exe).unwrap());
        for (g, name) in LAND_EXPORT.iter().enumerate() {
            let mut garden = Garden::read(&dll, dll.export(name).unwrap(), &exe, g).unwrap();
            let (set, count) = installed_set(["neut", "hero", "dark"][g]);
            garden.ledges = super::super::land::read_ledges(
                &set,
                0x1000,
                count,
                super::super::land::CLIMB_TYPE[g],
            )
            .unwrap();
            garden.blockers = super::super::land::read_blockers(&set, 0x1000, count, g).unwrap();
            // Hero: 1 HASIRA (1 shape) and 2 HASIRAKAKE (5 each); Dark: 7 fences, 6 graves.
            assert_eq!(garden.blockers.len(), [0, 11, 13][g]);
            assert_eq!(garden.ledges.len(), [4, 7, 11][g]);
            let mut rng = Rng::new(1234 + g as u32);
            let mut herd: Vec<Chao> = (0..8)
                .map(|i| {
                    let (x, z) = random_cell_point(&garden, 1, &mut rng).unwrap();
                    let mut rec = Record::default();
                    rec.set_u8(off::TYPE, super::super::record::TYPE_CHILD);
                    rec.set_u16(off::RUN_POINTS, [0, 100, 800, 1500][i % 4]);
                    rec.set_u16(off::FLY_POINTS, [0, 200][i % 2]);
                    Chao::new([x, 30.0, z], 0, rec)
                })
                .collect();
            let mut arrivals = 0;
            let mut time = std::collections::BTreeMap::new();
            let mut lowest = f32::MAX;
            for _ in 0..36_000 {
                for chao in &mut herd {
                    let before = chao.current();
                    chao.frame(&garden, g, &Ledges(&garden.ledges, chao.body.pos), &mut rng);
                    if before.is_some_and(|b| {
                        matches!(
                            b,
                            Behaviour::WalkSlow | Behaviour::Walk | Behaviour::Jog | Behaviour::Run
                        )
                    }) && chao.current() == Some(Behaviour::Settle)
                    {
                        arrivals += 1;
                    }
                    lowest = lowest.min(chao.body.pos[1]);
                    *time.entry(format!("{:?}", chao.current())).or_insert(0u32) += 1;
                }
            }
            eprintln!("{name}: {arrivals} walks finished, lowest y {lowest:.2}; frames {time:?}");
            for chao in &herd {
                let [x, y, z] = chao.body.pos;
                assert!(y > -10.0, "{name}: fell through at {x:.1},{y:.1},{z:.1}");
                assert!(
                    (-215.0..=190.0).contains(&x) && (-210.0..=200.0).contains(&z),
                    "{name}: left the garden at {x:.1},{z:.1}"
                );
            }
            assert!(arrivals > 8 * 10, "{name}: only {arrivals} walks finished");
        }
    }
}

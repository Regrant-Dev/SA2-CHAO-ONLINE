//! How the arena shows a battle (re/chao-battle-visuals.md): the Chao's motions for each kind of
//! move, signs of their status, the ROM's effect sprites flying from the user to its target,
//! sparkles and other particles, a white flash and a camera shake on strong hits, and sounds.
//!
//! `Director` takes the narrator's cues and says, frame by frame, what the arena does (where
//! each Chao stands, which motions, sounds, emotion balls and particles start); game.rs carries
//! that out. With the `BattleEffects` setting off it only plays the karate motions: an attack
//! for a damaging move, a wind-up for a status move, the hits, the knock-down and the result.
//! No game memory here.
//!
//! A move's kind comes from its ROM record:
//!
//! | kind | moves | the Chao |
//! |---|---|---|
//! | sound | the ROM's sound-move list | sings |
//! | rest | full heal and sleep | lies down to sleep |
//! | heal | the healing effects | eats |
//! | protect | the shielding and bracing effects | guards |
//! | raise self | status moves raising the user's stats | cheers |
//! | lower foe | status moves lowering the foe's stats, provoking | taunts |
//! | charge | other status moves | winds up |
//! | contact | damaging, with the contact flag | punches or kicks, stepping in |
//! | water | other water moves | swims a stroke in a splash |
//! | flying | other flying moves | flaps and glides up |
//! | spread | other physical moves hitting both foes or everyone | hops and lands |
//! | physical | other physical moves | throws |
//! | special | the rest | takes a focus pose |

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::data::{MoveData, Target, flags, types};
use super::effects as fx;
use super::effects_gfx::{Book, Draw, Key, Particle, Scene, Template, status};
use super::event::Slot;
use super::fighter::Ailment;
use super::narrate::{Cue, Sign};
use super::team::Dex;

static ENABLED: AtomicBool = AtomicBool::new(true);

/// The `BattleEffects` setting.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Chao motions (`al_motion_table` ids, re/chao-battle-visuals.md 1).
pub mod motion {
    pub const STANCE: u16 = 0x255;
    pub const WIND_UP: u16 = 0x242;
    pub const ATTACKS: [u16; 4] = [0x243, 0x244, 0x246, 0x247];
    pub const HIT: [u16; 3] = [0x249, 0x24A, 0x24B];
    pub const DOWN: u16 = 0x24C;
    pub const WON: [u16; 2] = [0x24D, 0x24E];
    pub const LOST: [u16; 2] = [0x24F, 0x250];
    pub const DRAW: u16 = 0x1E0;
    pub const HOP: u16 = 0x8A;
    pub const THROW: u16 = 0xBA;
    pub const SWIM: u16 = 0x91;
    pub const FLAP: u16 = 0x82;
    pub const FOCUS: [u16; 3] = [0x253, 0x254, 0x26D];
    pub const SING: u16 = 0x143;
    pub const CHEER: [u16; 3] = [0x24D, 0x24E, 0x1EC];
    pub const TAUNT: [u16; 5] = [0x253, 0x254, 0x26D, 0x187, 0x181];
    pub const GUARD: u16 = 0x241;
    /// Lying down to sleep (it goes on to the sleeping loop).
    pub const SLEEP: u16 = 0x12;
    pub const EAT: u16 = 0xA9;
    pub const BITE: u16 = 0xAF;
    pub const STUMBLE: u16 = 0x9F;
    /// A short shudder (an illness motion).
    pub const SHUDDER: u16 = 0xF8;
}

/// Sounds (re/chao-battle-visuals.md 4).
pub mod sound {
    pub const ATTACK_VOICES: [u32; 3] = [0x6070, 0x6071, 0x606F];
    pub const SPLASH: u32 = 0x1020;
    pub const SINGING: u32 = 0x1026;
    pub const JOY: [u32; 5] = [0x602F, 0x6030, 0x6031, 0x6032, 0x6033];
    pub const SNORES: [u32; 4] = [0x6008, 0x6009, 0x600A, 0x600B];
    pub const THUD: u32 = 0x100A;
    pub const SICK: u32 = 0x606E;
}

/// Emotion ball states.
mod ball {
    pub const EXCLAIM: u8 = 1;
    pub const SWIRL: u8 = 5;
}

/// Frames a Chao appears for before its stance starts.
const FIRST_STANCE: u16 = 3;
/// Frames after an action before the rest pose comes back.
const STANCE_AFTER: u16 = 50;
/// An attack's step toward its target: how far and how many frames out and back.
const STEP_DISTANCE: f32 = 3.0;
const STEP_FRAMES: u16 = 30;
/// A hop's frames in the air.
const HOP_FRAMES: u16 = 24;
/// A sprite's flight from the user to its target, and a burst's life (frames).
const FLY_FRAMES: u16 = 20;
const BURST_FRAMES: u16 = 24;
/// Copies of each flying sprite, this many frames apart.
const FLY_COPIES: u16 = 2;
const FLY_GAP: u16 = 4;
/// Sprites alive at once at most.
const MAX_SPRITES: usize = 32;
/// World units per GBA pixel.
const PIXEL: f32 = 0.08;
/// Heights above a Chao's feet: its chest and the top of its head.
const CHEST: f32 = 1.3;
const HEAD: f32 = 2.6;
/// A flight's arc height.
const ARC: f32 = 0.8;
/// The flash's opacity at its start and length; the shake's size and length.
const FLASH_ALPHA: f32 = 0.55;
const FLASH_FRAMES: u16 = 10;
const SHAKE_SIZE: f32 = 0.35;
const SHAKE_FRAMES: u16 = 14;
/// A shiver's sideways size.
const SHIVER: f32 = 0.12;
/// The ice block's height (about a Chao's).
const ICE_SIZE: f32 = 3.0;
/// Frames a Chao lies down for after Rest before standing back up, should it not fall asleep
/// (the sleep cue comes after the attack's beat and maybe a cure's message).
const REST_LIE: u16 = 180;
/// Frames between a lasting status's signs while a Chao rests.
const STATUS_EVERY: u32 = 150;

/// What a move looks like on the Chao (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Contact,
    Physical,
    Spread,
    Water,
    Flying,
    Special,
    Sound,
    RaiseSelf,
    LowerFoe,
    Protect,
    Rest,
    Heal,
    Charge,
}

/// Status effects that raise the user's own stats.
fn raises_self(effect: u8) -> bool {
    matches!(
        effect,
        fx::USER_UP_FIRST..=fx::USER_UP_LAST | fx::USER_UP2_FIRST..=fx::USER_UP2_LAST
    ) || [
        fx::FOCUS,
        fx::MAX_ATTACK_HALF_HP,
        fx::CURL,
        fx::SHRINK,
        fx::STORE_UP,
        fx::POWER_UP_ELECTRIC,
        fx::USER_DEFENSES_UP,
        fx::USER_ATTACK_DEFENSE_UP,
        fx::USER_SPECIALS_UP,
        fx::USER_ATTACK_SPEED_UP,
    ]
    .contains(&effect)
}

/// Status effects that lower a foe's stats or goad it.
fn lowers_foe(effect: u8) -> bool {
    matches!(
        effect,
        fx::TARGET_DOWN_FIRST..=fx::TARGET_DOWN_LAST
            | fx::TARGET_DOWN2_FIRST..=fx::TARGET_DOWN2_LAST
    ) || [
        fx::TARGET_ATTACK_DEFENSE_DOWN,
        fx::PROVOKE,
        fx::CONFUSE_ATTACK_UP2,
        fx::CONFUSE_SP_ATTACK_UP,
    ]
    .contains(&effect)
}

/// The kind of move `m` (`sound_moves`: the ROM's sound-move list).
pub fn classify(m: &MoveData, sound_moves: &[u16]) -> Kind {
    if sound_moves.contains(&m.id) {
        return Kind::Sound;
    }
    if m.power == 0 {
        return match m.effect {
            fx::SLEEP_HEAL => Kind::Rest,
            fx::HEAL_HALF
            | fx::HEAL_HALF_B
            | fx::WEATHER_HEAL_A
            | fx::WEATHER_HEAL_B
            | fx::WEATHER_HEAL_C
            | fx::RELEASE_HEAL
            | fx::ROOT
            | fx::DELAYED_HEAL
            | fx::CURE_SELF => Kind::Heal,
            fx::SHIELD | fx::BRACE => Kind::Protect,
            e if raises_self(e) => Kind::RaiseSelf,
            e if lowers_foe(e) => Kind::LowerFoe,
            _ => Kind::Charge,
        };
    }
    if m.flags & flags::CONTACT != 0 {
        return Kind::Contact;
    }
    match m.type_id {
        types::WATER => Kind::Water,
        types::FLYING => Kind::Flying,
        t if types::is_physical(t) => {
            if matches!(m.target, Target::BothFoes | Target::AllOthers) {
                Kind::Spread
            } else {
                Kind::Physical
            }
        }
        _ => Kind::Special,
    }
}

impl Kind {
    /// Its effect sprites show on the user rather than flying at the target.
    fn on_self(self) -> bool {
        matches!(
            self,
            Kind::RaiseSelf | Kind::Protect | Kind::Rest | Kind::Heal
        )
    }
}

/// Something the arena does now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    Motion {
        slot: Slot,
        id: u16,
        blend: u32,
    },
    Sound(u32),
    /// An emotion ball over the Chao at `slot`.
    Ball {
        slot: Slot,
        state: u8,
        frames: u32,
    },
    Particles {
        kind: Particle,
        at: [f32; 3],
        count: u32,
    },
}

/// A frame's work: where each Chao stands (position, lift off the floor) and what starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub places: Vec<(Slot, [f32; 3], f32)>,
    pub orders: Vec<Order>,
}

/// Something a Chao does later (in a stand's list, with the frames until then).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Later {
    Motion(u16, u32),
    Sound(u32),
    Particles(Particle, u32),
    Shake(u16),
}

/// How a sprite moves over its life.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Path {
    /// From `from` to `to` in an arc.
    Fly,
    /// At `to`, growing a little.
    Burst,
    /// Up from `to`.
    Rise,
    /// Around `to`.
    Orbit,
    /// Still, standing on `to`, about a Chao's height.
    Stay,
}

#[derive(Clone, Debug, PartialEq)]
struct Sprite {
    look: Template,
    path: Path,
    from: [f32; 3],
    to: [f32; 3],
    /// Mirrored (flying the other way).
    flip: bool,
    /// A status sign on the Chao at this slot (it goes when the status does).
    on: Option<Slot>,
    delay: u16,
    age: u16,
    life: u16,
}

/// One Chao in the ring.
#[derive(Clone, Debug, PartialEq)]
struct Stand {
    home: [f32; 3],
    /// An attack's step toward this point: frames played.
    step: Option<([f32; 3], u16)>,
    /// A hop or glide: frame, length, height.
    lift: Option<(u16, u16, f32)>,
    /// Frames of shivering left.
    shiver: u16,
    later: Vec<(u16, Later)>,
    /// Frames until the rest pose (0: none due).
    rest_in: u16,
    ailment: Option<Ailment>,
    confused: bool,
    /// Down or done: no rest pose any more.
    down: bool,
    /// Frames resting, for a lasting status's signs.
    idle: u32,
    /// Its last motion was lying down to sleep (it stays in the sleeping loop).
    lying: bool,
}

impl Stand {
    fn new(home: [f32; 3]) -> Stand {
        Stand {
            home,
            step: None,
            lift: None,
            shiver: 0,
            later: Vec::new(),
            rest_in: FIRST_STANCE,
            ailment: None,
            confused: false,
            down: false,
            idle: 0,
            lying: false,
        }
    }

    fn chest(&self) -> [f32; 3] {
        [self.home[0], self.home[1] + CHEST, self.home[2]]
    }

    fn head(&self) -> [f32; 3] {
        [self.home[0], self.home[1] + HEAD, self.home[2]]
    }

    fn feet(&self) -> [f32; 3] {
        self.home
    }

    /// Busy with an action (the rest pose and status signs wait).
    fn busy(&self) -> bool {
        self.rest_in > 0 || !self.later.is_empty() || self.step.is_some() || self.lift.is_some()
    }
}

/// The arena's director for one battle.
pub struct Director {
    effects: bool,
    /// Where each slot stands.
    homes: [[[f32; 3]; 2]; 2],
    stands: [[Option<Stand>; 2]; 2],
    sprites: Vec<Sprite>,
    flash: u16,
    shake: u16,
    /// Varies the motions and voices.
    pick: usize,
    frame: u32,
    dex: Option<Arc<Dex>>,
    book: Option<Arc<Book>>,
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

impl Director {
    /// `homes`: where each slot stands; `effects`: the full show (else the karate
    /// motions only); `dex` and `book` give each move's kind and sprites.
    pub fn new(
        effects: bool,
        homes: [[[f32; 3]; 2]; 2],
        dex: Option<Arc<Dex>>,
        book: Option<Arc<Book>>,
    ) -> Director {
        Director {
            effects,
            homes,
            stands: [[None, None], [None, None]],
            sprites: Vec::new(),
            flash: 0,
            shake: 0,
            pick: 0,
            frame: 0,
            dex,
            book,
        }
    }

    fn stand(&mut self, slot: Slot) -> Option<&mut Stand> {
        self.stands.get_mut(slot.side)?.get_mut(slot.pos)?.as_mut()
    }

    fn stand_ref(&self, slot: Slot) -> Option<&Stand> {
        self.stands.get(slot.side)?.get(slot.pos)?.as_ref()
    }

    fn home(&self, slot: Slot) -> [f32; 3] {
        self.homes
            .get(slot.side)
            .and_then(|s| s.get(slot.pos))
            .copied()
            .unwrap_or([0.0; 3])
    }

    fn chest_of(&self, slot: Slot) -> [f32; 3] {
        let h = self.home(slot);
        [h[0], h[1] + CHEST, h[2]]
    }

    fn head_of(&self, slot: Slot) -> [f32; 3] {
        let h = self.home(slot);
        [h[0], h[1] + HEAD, h[2]]
    }

    /// The kind of move `id` (with the full show off: contact for a damaging one, charge for a
    /// status move).
    fn kind(&self, id: u16, status: bool) -> Kind {
        let plain = if status { Kind::Charge } else { Kind::Contact };
        if !self.effects {
            return plain;
        }
        self.dex
            .as_ref()
            .and_then(|d| Some(classify(&d.move_data(id)?, d.sound_moves())))
            .unwrap_or(plain)
    }

    fn add_sprite(&mut self, s: Sprite) {
        if self.sprites.len() >= MAX_SPRITES {
            self.sprites.remove(0);
        }
        self.sprites.push(s);
    }

    /// A status sprite (`status::*`) on the Chao at `slot`, at `to` along `path`.
    fn status_sprite(&mut self, slot: Slot, index: u8, path: Path, to: [f32; 3], life: u16) {
        let Some(look) = self.book.as_ref().and_then(|b| b.status(index)).cloned() else {
            return;
        };
        self.add_sprite(Sprite {
            look,
            path,
            from: to,
            to,
            flip: false,
            on: Some(slot),
            delay: 0,
            age: 0,
            life,
        });
    }

    /// The status sprites on the Chao at `slot` go.
    fn clear_status(&mut self, slot: Slot) {
        self.sprites.retain(|s| s.on != Some(slot));
    }

    /// Notes which Chao lie asleep after `orders`' motions.
    fn note_motions(&mut self, orders: &[Order]) {
        for o in orders {
            if let Order::Motion { slot, id, .. } = *o
                && let Some(s) = self.stand(slot)
            {
                s.lying = id == motion::SLEEP;
            }
        }
    }

    /// Move `id` only works on its user or the whole field (screens, weather and the like).
    fn on_user(&self, id: u16) -> bool {
        self.dex
            .as_ref()
            .and_then(|d| d.move_data(id))
            .is_some_and(|m| m.power == 0 && m.target == Target::User)
    }

    /// Takes one cue (game.rs makes or removes the Chao for `Show` and `Hide` first).
    pub fn cue(&mut self, cue: &Cue) -> Vec<Order> {
        self.pick = self.pick.wrapping_add(1);
        let pick = self.pick;
        let mut out = Vec::new();
        match *cue {
            Cue::Show { slot, .. } => {
                self.clear_status(slot);
                let home = self.home(slot);
                if let Some(s) = self
                    .stands
                    .get_mut(slot.side)
                    .and_then(|s| s.get_mut(slot.pos))
                {
                    *s = Some(Stand::new(home));
                }
            }
            Cue::Hide { slot } => {
                self.clear_status(slot);
                if let Some(s) = self
                    .stands
                    .get_mut(slot.side)
                    .and_then(|s| s.get_mut(slot.pos))
                {
                    *s = None;
                }
            }
            Cue::Attack {
                by,
                toward,
                status,
                move_id,
            } => self.attack(by, toward, status, move_id, &mut out),
            Cue::Hurt { target, heavy } => {
                if let Some(s) = self.stand(target) {
                    let id = motion::HIT[usize::from(heavy).min(motion::HIT.len() - 1)];
                    out.push(Order::Motion {
                        slot: target,
                        id,
                        blend: 1,
                    });
                    s.rest_in = STANCE_AFTER;
                }
            }
            Cue::Faint { at } => {
                self.clear_status(at);
                let effects = self.effects;
                if let Some(s) = self.stand(at) {
                    out.push(Order::Motion {
                        slot: at,
                        id: motion::DOWN,
                        blend: 1,
                    });
                    s.rest_in = 0;
                    s.down = true;
                    s.step = None;
                    s.lift = None;
                    s.later.clear();
                    if effects {
                        // Falls, then cries.
                        s.later
                            .push((40, Later::Motion(motion::LOST[pick % 2], 15)));
                        out.push(Order::Ball {
                            slot: at,
                            state: ball::SWIRL,
                            frames: 300,
                        });
                    }
                }
            }
            Cue::Result { winner } => {
                for side in 0..2 {
                    for pos in 0..2 {
                        let slot = Slot::new(side, pos);
                        let Some(s) = self.stand(slot) else { continue };
                        let id = match winner {
                            None => motion::DRAW,
                            Some(w) if w == side => motion::WON[pick % 2],
                            Some(_) => motion::LOST[pick % 2],
                        };
                        s.rest_in = 0;
                        s.down = true;
                        s.step = None;
                        s.lift = None;
                        s.later.clear();
                        out.push(Order::Motion {
                            slot,
                            id,
                            blend: 20,
                        });
                    }
                }
            }
            Cue::Ailment { slot, ailment } => {
                if !self.effects {
                    return out;
                }
                let Some(s) = self.stand(slot) else {
                    return out;
                };
                s.ailment = ailment;
                s.idle = 0;
                let confused = s.confused;
                self.clear_status(slot);
                if confused {
                    self.status_sprite(
                        slot,
                        status::CONFUSION,
                        Path::Orbit,
                        self.head_of(slot),
                        90,
                    );
                }
                let Some(s) = self.stand(slot) else {
                    return out;
                };
                match ailment {
                    None => {
                        if !s.busy() {
                            s.rest_in = 1;
                        }
                    }
                    Some(Ailment::Sleep) => {
                        // Lies down now (or stays lying) and rests asleep after.
                        s.rest_in = 1;
                        self.sign(slot, Sign::Snoring, &mut out);
                    }
                    Some(Ailment::Paralysis) => self.sign(slot, Sign::Paralyzed, &mut out),
                    Some(Ailment::Freeze) => self.sign(slot, Sign::Frozen, &mut out),
                    Some(Ailment::Burn) => self.sign(slot, Sign::Burned, &mut out),
                    Some(Ailment::Poison | Ailment::BadPoison) => {
                        self.sign(slot, Sign::Poisoned, &mut out)
                    }
                }
            }
            Cue::Confusion { slot, on } => {
                if !self.effects {
                    return out;
                }
                if let Some(s) = self.stand(slot) {
                    s.confused = on;
                    s.idle = 0;
                }
                if !on {
                    // Its other status sprites come back with the next rest.
                    self.clear_status(slot);
                }
                if on {
                    self.sign(slot, Sign::Confused, &mut out);
                }
            }
            Cue::Sign { slot, sign } => {
                if self.effects {
                    self.sign(slot, sign, &mut out);
                }
            }
            Cue::Sound(_) => {}
        }
        self.note_motions(&out);
        out
    }

    fn attack(
        &mut self,
        by: Slot,
        toward: Option<Slot>,
        status: bool,
        move_id: u16,
        out: &mut Vec<Order>,
    ) {
        let kind = self.kind(move_id, status);
        let pick = self.pick;
        let target = toward.map(|t| self.home(t));
        let target_chest = toward.map(|t| self.chest_of(t));
        let Some(s) = self.stand(by) else { return };
        let chest = s.chest();
        let motion = |id: u16, blend: u32| Order::Motion {
            slot: by,
            id,
            blend,
        };
        s.rest_in = STANCE_AFTER;
        s.later.clear();
        // When the sprites leave (frames after the cue).
        let launch: u16 = match kind {
            Kind::Contact => {
                out.push(motion(motion::ATTACKS[pick % motion::ATTACKS.len()], 15));
                out.push(Order::Sound(
                    sound::ATTACK_VOICES[pick % sound::ATTACK_VOICES.len()],
                ));
                s.step = target.map(|t| (t, 0));
                6
            }
            Kind::Charge => {
                out.push(motion(motion::WIND_UP, 20));
                12
            }
            Kind::Physical => {
                out.push(motion(motion::THROW, 10));
                out.push(Order::Sound(sound::ATTACK_VOICES[pick % 3]));
                10
            }
            Kind::Spread => {
                out.push(motion(motion::HOP, 8));
                s.lift = Some((0, HOP_FRAMES, 1.6));
                s.later
                    .push((HOP_FRAMES, Later::Particles(Particle::Dust, 6)));
                s.later.push((HOP_FRAMES, Later::Shake(8)));
                HOP_FRAMES
            }
            Kind::Water => {
                out.push(motion(motion::SWIM, 10));
                out.push(Order::Sound(sound::SPLASH));
                out.push(Order::Particles {
                    kind: Particle::Splash,
                    at: chest,
                    count: 6,
                });
                s.later.push((15, Later::Particles(Particle::Splash, 6)));
                10
            }
            Kind::Flying => {
                out.push(motion(motion::FLAP, 8));
                s.lift = Some((0, 40, 1.2));
                12
            }
            Kind::Special => {
                out.push(motion(motion::FOCUS[pick % motion::FOCUS.len()], 15));
                out.push(Order::Particles {
                    kind: Particle::Sparkle,
                    at: chest,
                    count: 4,
                });
                14
            }
            Kind::Sound => {
                out.push(motion(motion::SING, 10));
                out.push(Order::Sound(sound::SINGING));
                8
            }
            Kind::RaiseSelf => {
                out.push(motion(motion::CHEER[pick % motion::CHEER.len()], 12));
                out.push(Order::Sound(sound::JOY[pick % sound::JOY.len()]));
                s.later.push((5, Later::Particles(Particle::Sparkle, 8)));
                6
            }
            Kind::LowerFoe => {
                out.push(motion(motion::TAUNT[pick % motion::TAUNT.len()], 12));
                10
            }
            Kind::Protect => {
                out.push(motion(motion::GUARD, 6));
                out.push(Order::Particles {
                    kind: Particle::Sparkle,
                    at: chest,
                    count: 6,
                });
                4
            }
            Kind::Rest => {
                out.push(motion(motion::SLEEP, 20));
                s.lying = true;
                s.rest_in = REST_LIE;
                s.later
                    .push((30, Later::Sound(sound::SNORES[pick % sound::SNORES.len()])));
                10
            }
            Kind::Heal => {
                out.push(motion(motion::EAT, 10));
                s.later.push((27, Later::Motion(motion::BITE, 8)));
                s.later.push((20, Later::Particles(Particle::Sparkle, 8)));
                10
            }
        };
        if !self.effects {
            return;
        }
        // The sprites: from the user's chest to the target's (or on the user).
        let Some(look) = self.book.as_ref().map(|b| b.for_move(move_id)) else {
            return;
        };
        let to = if kind.on_self() || self.on_user(move_id) {
            chest
        } else {
            target_chest.unwrap_or(chest)
        };
        let flip = by.side == 1;
        let flies = !look.fly.is_empty() && to != chest;
        let mut extra = 0;
        for t in &look.fly {
            for copy in 0..FLY_COPIES {
                let (path, from) = if flies {
                    (Path::Fly, chest)
                } else {
                    (Path::Burst, to)
                };
                self.add_sprite(Sprite {
                    look: t.clone(),
                    path,
                    from,
                    to,
                    flip,
                    on: None,
                    delay: launch + extra + copy * FLY_GAP,
                    age: 0,
                    life: if flies { FLY_FRAMES } else { BURST_FRAMES },
                });
            }
            extra += 2;
        }
        let landed = if flies { launch + FLY_FRAMES } else { launch };
        for (i, t) in look.burst.iter().enumerate() {
            let i = i as u16;
            self.add_sprite(Sprite {
                look: t.clone(),
                path: Path::Burst,
                from: to,
                to,
                flip,
                on: None,
                delay: landed + i * 3,
                age: 0,
                life: BURST_FRAMES,
            });
        }
    }

    /// A moment's sign on the Chao at `slot` (the full show only).
    fn sign(&mut self, slot: Slot, sign: Sign, out: &mut Vec<Order>) {
        let pick = self.pick;
        let Some(s) = self.stand(slot) else { return };
        let (chest, head) = (s.chest(), s.head());
        let motion = |id: u16, blend: u32| Order::Motion { slot, id, blend };
        match sign {
            Sign::Struck { strong } => {
                if strong {
                    self.flash = FLASH_FRAMES;
                    self.shake = SHAKE_FRAMES;
                }
                if let Some(look) = self.book.as_ref().and_then(|b| b.hit()).cloned() {
                    self.add_sprite(Sprite {
                        look,
                        path: Path::Burst,
                        from: chest,
                        to: chest,
                        flip: false,
                        on: None,
                        delay: 0,
                        age: 0,
                        life: BURST_FRAMES / 2,
                    });
                }
            }
            Sign::Confused => {
                out.push(motion(motion::STUMBLE, 8));
                out.push(Order::Ball {
                    slot,
                    state: ball::SWIRL,
                    frames: 150,
                });
                s.later.push((40, Later::Sound(sound::THUD)));
                s.rest_in = 70;
                self.status_sprite(slot, status::CONFUSION, Path::Orbit, head, 90);
            }
            Sign::Flinched => {
                out.push(motion(motion::HIT[0], 4));
                out.push(Order::Ball {
                    slot,
                    state: ball::EXCLAIM,
                    frames: 90,
                });
                s.rest_in = 40;
            }
            Sign::Paralyzed => {
                s.shiver = 36;
                self.status_sprite(slot, status::PARALYSIS, Path::Burst, chest, BURST_FRAMES);
            }
            Sign::Snoring => {
                // Lies down unless it already lies in the sleeping loop.
                if !s.lying {
                    out.push(motion(motion::SLEEP, 15));
                    s.lying = true;
                }
                out.push(Order::Sound(sound::SNORES[pick % sound::SNORES.len()]));
                s.idle = 0;
                self.status_sprite(slot, status::SLEEP, Path::Rise, head, 60);
            }
            Sign::Frozen => {
                s.shiver = 10;
                out.push(Order::Particles {
                    kind: Particle::Snow,
                    at: chest,
                    count: 4,
                });
                // The ice itself stays while it's frozen (`frame`).
            }
            Sign::Burned => {
                out.push(Order::Particles {
                    kind: Particle::Flame,
                    at: chest,
                    count: 5,
                });
                self.status_sprite(slot, status::BURN, Path::Burst, chest, BURST_FRAMES);
            }
            Sign::Poisoned => {
                out.push(motion(motion::SHUDDER, 6));
                out.push(Order::Sound(sound::SICK));
                s.rest_in = 40;
            }
            Sign::Guarded => {
                out.push(motion(motion::GUARD, 4));
                out.push(Order::Particles {
                    kind: Particle::Sparkle,
                    at: chest,
                    count: 4,
                });
                s.rest_in = 40;
            }
            Sign::StatUp | Sign::Healed => out.push(Order::Particles {
                kind: Particle::Sparkle,
                at: chest,
                count: if sign == Sign::Healed { 10 } else { 8 },
            }),
            Sign::StatDown => out.push(Order::Particles {
                kind: Particle::Dust,
                at: s.feet(),
                count: 5,
            }),
        }
    }

    /// One frame on: steps, hops and shivers, what was waiting, the rest poses, a lasting
    /// status's signs, and the sprites.
    pub fn frame(&mut self) -> Out {
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        let effects = self.effects;
        let mut out = Out::default();
        let mut shakes = 0;
        let mut periodic = Vec::new();
        let mut frozen = Vec::new();
        for side in 0..2 {
            for pos in 0..2 {
                let slot = Slot::new(side, pos);
                let Some(s) = self.stand(slot) else { continue };
                let mut at = s.home;
                if let Some((target, f)) = s.step {
                    let t = f32::from(f) / f32::from(STEP_FRAMES);
                    let (dx, dz) = (target[0] - s.home[0], target[2] - s.home[2]);
                    let length = (dx * dx + dz * dz).sqrt().max(1.0);
                    let reach = (t * std::f32::consts::PI).sin() * STEP_DISTANCE;
                    at[0] += dx / length * reach;
                    at[2] += dz / length * reach;
                    s.step = (f < STEP_FRAMES).then_some((target, f + 1));
                }
                let mut lift = 0.0;
                if let Some((f, frames, height)) = s.lift {
                    let t = f32::from(f) / f32::from(frames.max(1));
                    lift = (t * std::f32::consts::PI).sin().max(0.0) * height;
                    s.lift = (f < frames).then_some((f + 1, frames, height));
                }
                if s.shiver > 0 {
                    s.shiver -= 1;
                    at[0] += (frame as f32 * 2.3).sin() * SHIVER;
                    at[2] += (frame as f32 * 3.1).cos() * SHIVER * 0.5;
                }
                out.places.push((slot, at, lift));
                // What was waiting.
                let mut due = Vec::new();
                s.later.retain_mut(|(wait, what)| {
                    if *wait <= 1 {
                        due.push(*what);
                        false
                    } else {
                        *wait -= 1;
                        true
                    }
                });
                for what in due {
                    match what {
                        Later::Motion(id, blend) => {
                            out.orders.push(Order::Motion { slot, id, blend })
                        }
                        Later::Sound(id) => out.orders.push(Order::Sound(id)),
                        Later::Particles(kind, count) => {
                            let at = if kind == Particle::Dust {
                                s.feet()
                            } else {
                                s.chest()
                            };
                            out.orders.push(Order::Particles { kind, at, count })
                        }
                        Later::Shake(frames) => shakes = shakes.max(frames),
                    }
                }
                // The rest pose: asleep, or the fighting stance.
                if s.rest_in > 0 {
                    s.rest_in -= 1;
                    if s.rest_in == 0 && !s.down {
                        let asleep = effects && s.ailment == Some(Ailment::Sleep);
                        let (id, blend) = if asleep {
                            (motion::SLEEP, 20)
                        } else {
                            (motion::STANCE, 25)
                        };
                        // Already lying asleep: the sleeping loop goes on.
                        if !(asleep && s.lying) {
                            out.orders.push(Order::Motion { slot, id, blend });
                        }
                        s.idle = 0;
                    }
                }
                if effects && !s.down && s.ailment == Some(Ailment::Freeze) {
                    frozen.push((slot, s.feet()));
                }
                if effects && !s.down && !s.busy() {
                    s.idle += 1;
                    if s.idle % STATUS_EVERY == 0 {
                        periodic.push((slot, s.ailment, s.confused));
                    }
                }
            }
        }
        // A lasting status shows now and then while the Chao rests.
        for (slot, ailment, confused) in periodic {
            let Some(s) = self.stand_ref(slot) else {
                continue;
            };
            let (chest, head) = (s.chest(), s.head());
            match ailment {
                Some(Ailment::Sleep) => {
                    self.status_sprite(slot, status::SLEEP, Path::Rise, head, 60)
                }
                Some(Ailment::Paralysis) => {
                    if let Some(s) = self.stand(slot) {
                        s.shiver = 16;
                    }
                    self.status_sprite(slot, status::PARALYSIS, Path::Burst, chest, BURST_FRAMES);
                }
                _ => {}
            }
            if confused {
                self.status_sprite(slot, status::CONFUSION, Path::Orbit, head, 90);
            }
        }
        if shakes > 0 {
            self.shake = self.shake.max(shakes);
        }
        for s in &mut self.sprites {
            if s.delay > 0 {
                s.delay -= 1;
            } else {
                s.age += 1;
            }
        }
        self.sprites.retain(|s| s.age < s.life);
        // A frozen Chao sits in a block of ice.
        let ice = self
            .book
            .as_ref()
            .and_then(|b| b.status(status::FREEZE))
            .cloned();
        for (slot, feet) in frozen {
            let iced = self
                .sprites
                .iter()
                .any(|s| s.on == Some(slot) && Some(&s.look) == ice.as_ref());
            if !iced {
                self.status_sprite(slot, status::FREEZE, Path::Stay, feet, STATUS_EVERY as u16);
            }
        }
        self.flash = self.flash.saturating_sub(1);
        self.shake = self.shake.saturating_sub(1);
        self.note_motions(&out.orders);
        out
    }

    /// What's drawn this frame: the sprites in flight and the flash.
    pub fn scene(&self) -> Scene {
        let draws = self.sprites.iter().filter_map(draw_of).collect();
        let flash = if self.flash > 0 {
            FLASH_ALPHA * f32::from(self.flash) / f32::from(FLASH_FRAMES)
        } else {
            0.0
        };
        Scene { draws, flash }
    }

    /// The camera's offset this frame (a shake after a strong hit or a landing).
    pub fn shake(&self) -> [f32; 3] {
        if self.shake == 0 {
            return [0.0; 3];
        }
        let size = SHAKE_SIZE * f32::from(self.shake) / f32::from(SHAKE_FRAMES);
        let f = self.frame as f32;
        [(f * 1.9).sin() * size, (f * 2.7).cos() * size * 0.6, 0.0]
    }

    /// The full show (else the karate motions only).
    pub fn effects(&self) -> bool {
        self.effects
    }

    /// Nothing is in flight or flashing.
    pub fn quiet(&self) -> bool {
        self.sprites.is_empty() && self.flash == 0 && self.shake == 0
    }
}

/// Where and how a sprite shows now (`None` before it starts).
fn draw_of(s: &Sprite) -> Option<Draw> {
    if s.delay > 0 || s.age >= s.life {
        return None;
    }
    let t = f32::from(s.age) / f32::from(s.life.max(1));
    let pi = std::f32::consts::PI;
    let base = f32::from(s.look.width.max(s.look.height)) * PIXEL;
    let (at, size) = match s.path {
        Path::Fly => {
            let mut p = lerp(s.from, s.to, t);
            p[1] += (t * pi).sin() * ARC;
            (p, base)
        }
        Path::Burst => (s.to, base * (0.7 + 0.5 * t)),
        Path::Rise => ([s.to[0], s.to[1] + 1.5 * t, s.to[2]], base),
        Path::Orbit => {
            let a = t * 2.0 * std::f32::consts::TAU;
            (
                [s.to[0] + a.cos() * 0.8, s.to[1], s.to[2] + a.sin() * 0.8],
                base,
            )
        }
        Path::Stay => ([s.to[0], s.to[1] + ICE_SIZE / 2.0, s.to[2]], ICE_SIZE),
    };
    let mut frame = frame_at(&s.look, s.age);
    if s.flip {
        frame.h_flip = !frame.h_flip;
    }
    Some(Draw {
        key: Key {
            tag: s.look.tag,
            width: s.look.width,
            height: s.look.height,
            frame,
        },
        at,
        size,
    })
}

/// The frame of `look`'s animation `age` frames in (looping; a frame of 0 frames counts as 1).
fn frame_at(look: &Template, age: u16) -> super::effects_gfx::Frame {
    let total: u32 = look
        .frames
        .iter()
        .map(|f| u32::from(f.duration.max(1)))
        .sum();
    let mut at = u32::from(age) % total.max(1);
    for f in &look.frames {
        let d = u32::from(f.duration.max(1));
        if at < d {
            return *f;
        }
        at -= d;
    }
    look.frames
        .first()
        .copied()
        .unwrap_or(super::effects_gfx::Frame {
            tile: 0,
            duration: 0,
            h_flip: false,
            v_flip: false,
        })
}

#[cfg(test)]
mod tests {
    use super::super::data::{MoveData, TypeChart};
    use super::super::effects_gfx::{Frame, MoveLook};
    use super::*;

    fn mv(id: u16, type_id: u8, power: u8) -> MoveData {
        MoveData::plain(id, type_id, power)
    }

    #[test]
    fn moves_are_classified_from_their_records() {
        let sound = [9];
        let contact = MoveData {
            flags: flags::CONTACT,
            ..mv(1, types::WATER, 60)
        };
        assert_eq!(classify(&contact, &sound), Kind::Contact);
        assert_eq!(classify(&mv(2, types::WATER, 40), &sound), Kind::Water);
        assert_eq!(classify(&mv(3, types::FLYING, 40), &sound), Kind::Flying);
        assert_eq!(classify(&mv(4, types::ROCK, 50), &sound), Kind::Physical);
        let spread = MoveData {
            target: Target::AllOthers,
            ..mv(5, types::GROUND, 100)
        };
        assert_eq!(classify(&spread, &sound), Kind::Spread);
        assert_eq!(classify(&mv(6, types::FIRE, 90), &sound), Kind::Special);
        // The sound list wins over everything.
        assert_eq!(classify(&mv(9, types::NORMAL, 0), &sound), Kind::Sound);
        let status = |effect| MoveData {
            effect,
            ..mv(7, types::NORMAL, 0)
        };
        assert_eq!(classify(&status(fx::SLEEP_HEAL), &sound), Kind::Rest);
        assert_eq!(classify(&status(fx::HEAL_HALF), &sound), Kind::Heal);
        assert_eq!(classify(&status(fx::SHIELD), &sound), Kind::Protect);
        assert_eq!(
            classify(&status(fx::USER_UP_FIRST), &sound),
            Kind::RaiseSelf
        );
        assert_eq!(
            classify(&status(fx::USER_UP2_LAST), &sound),
            Kind::RaiseSelf
        );
        assert_eq!(
            classify(&status(fx::TARGET_DOWN_FIRST + 1), &sound),
            Kind::LowerFoe
        );
        assert_eq!(classify(&status(fx::PROVOKE), &sound), Kind::LowerFoe);
        assert_eq!(classify(&status(fx::CONFUSE), &sound), Kind::Charge);
    }

    #[test]
    fn every_rom_move_has_a_kind_and_the_kinds_spread_out() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return;
        }
        let rom = crate::gba::rom::Rom::open(&path).expect("ROM opens");
        let dex = Dex::from_rom(&rom).expect("dex");
        let mut counts = std::collections::HashMap::new();
        for m in dex.table() {
            *counts.entry(classify(&m, dex.sound_moves())).or_insert(0) += 1;
        }
        // Every kind is some move's.
        for kind in [
            Kind::Contact,
            Kind::Physical,
            Kind::Spread,
            Kind::Water,
            Kind::Flying,
            Kind::Special,
            Kind::Sound,
            Kind::RaiseSelf,
            Kind::LowerFoe,
            Kind::Protect,
            Kind::Rest,
            Kind::Heal,
            Kind::Charge,
        ] {
            assert!(counts.get(&kind).is_some_and(|&n| n > 0), "{kind:?}");
        }
        assert!(counts[&Kind::Contact] > 50, "{counts:?}");
        assert!(counts[&Kind::Rest] < 5, "{counts:?}");
    }

    fn homes() -> [[[f32; 3]; 2]; 2] {
        [
            [[-5.0, 8.0, 19.0], [-5.0, 8.0, 22.0]],
            [[5.0, 8.0, 19.0], [5.0, 8.0, 22.0]],
        ]
    }

    fn look(tag: u16) -> Template {
        Template {
            tag,
            width: 32,
            height: 16,
            frames: vec![
                Frame {
                    tile: 0,
                    duration: 3,
                    h_flip: false,
                    v_flip: false,
                },
                Frame {
                    tile: 8,
                    duration: 3,
                    h_flip: false,
                    v_flip: false,
                },
            ],
        }
    }

    fn director(effects: bool) -> Director {
        let moves = [
            MoveData {
                flags: flags::CONTACT,
                ..mv(1, types::NORMAL, 40)
            },
            mv(2, types::FIRE, 90),
            MoveData {
                effect: fx::USER_UP_FIRST,
                ..mv(3, types::NORMAL, 0)
            },
            // A wall for the user's side.
            MoveData {
                target: Target::User,
                ..mv(4, types::NORMAL, 0)
            },
        ];
        let dex = Dex::for_tests(&moves, TypeChart::default());
        let mut looks = vec![MoveLook::default(); 5];
        looks[2] = MoveLook {
            fly: vec![look(10_001)],
            burst: vec![look(10_002)],
        };
        looks[4] = looks[2].clone();
        let book = Book::for_tests(looks, Some(look(10_135)), Some(look(10_200)));
        Director::new(effects, homes(), Some(Arc::new(dex)), Some(Arc::new(book)))
    }

    fn show(d: &mut Director) -> (Slot, Slot) {
        let (a, b) = (Slot::new(0, 0), Slot::new(1, 0));
        d.cue(&Cue::Show { slot: a, member: 0 });
        d.cue(&Cue::Show { slot: b, member: 0 });
        (a, b)
    }

    fn motions(orders: &[Order]) -> Vec<u16> {
        orders
            .iter()
            .filter_map(|o| match *o {
                Order::Motion { id, .. } => Some(id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_chao_takes_its_stance_and_steps_in_for_a_contact_move() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let mut stance = 0;
        for _ in 0..FIRST_STANCE {
            stance += motions(&d.frame().orders)
                .iter()
                .filter(|&&m| m == motion::STANCE)
                .count();
        }
        assert_eq!(stance, 2);
        let orders = d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 1,
        });
        assert!(motion::ATTACKS.contains(&motions(&orders)[0]));
        // Out toward the foe and back.
        let mut furthest: f32 = -5.0;
        for _ in 0..=STEP_FRAMES {
            let out = d.frame();
            let (_, at, _) = out.places.iter().find(|(s, _, _)| *s == a).unwrap();
            furthest = furthest.max(at[0]);
        }
        assert!(furthest > -2.5, "{furthest}");
        let back = d.frame();
        assert_eq!(back.places[0].1, [-5.0, 8.0, 19.0]);
    }

    #[test]
    fn a_special_move_poses_and_its_sprites_fly_then_burst() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let orders = d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 2,
        });
        assert!(motion::FOCUS.contains(&motions(&orders)[0]));
        let mut flying = Vec::new();
        let mut bursting = 0;
        for _ in 0..60 {
            d.frame();
            for draw in d.scene().draws {
                match draw.key.tag {
                    10_001 => flying.push(draw.at[0]),
                    10_002 => bursting += 1,
                    _ => {}
                }
            }
        }
        // The flight goes from the user (left) to the target (right), then the burst shows.
        assert!(flying.len() > 10);
        assert!(flying.first().unwrap() < &0.0 && flying.last().unwrap() > &0.0);
        assert!(bursting > 0);
        assert!(d.quiet());
    }

    #[test]
    fn a_move_for_the_users_side_shows_on_the_user() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: true,
            move_id: 4,
        });
        let mut seen = 0;
        for _ in 0..60 {
            d.frame();
            for draw in d.scene().draws {
                seen += 1;
                assert!((draw.at[0] + 5.0).abs() < 0.1, "{draw:?}");
            }
        }
        assert!(seen > 0);
    }

    #[test]
    fn a_strong_hit_flashes_and_shakes_and_a_plain_one_does_not() {
        let mut d = director(true);
        let (_, b) = show(&mut d);
        d.cue(&Cue::Sign {
            slot: b,
            sign: Sign::Struck { strong: false },
        });
        d.frame();
        assert_eq!(d.scene().flash, 0.0);
        assert_eq!(d.shake(), [0.0; 3]);
        assert_eq!(d.scene().draws[0].key.tag, 10_135);
        d.cue(&Cue::Sign {
            slot: b,
            sign: Sign::Struck { strong: true },
        });
        d.frame();
        assert!(d.scene().flash > 0.0);
        assert_ne!(d.shake(), [0.0; 3]);
        for _ in 0..SHAKE_FRAMES {
            d.frame();
        }
        assert_eq!(d.scene().flash, 0.0);
        assert_eq!(d.shake(), [0.0; 3]);
    }

    #[test]
    fn a_sleeping_chao_rests_asleep_and_wakes_to_its_stance() {
        let mut d = director(true);
        let (a, _) = show(&mut d);
        for _ in 0..FIRST_STANCE {
            d.frame();
        }
        let mut seen = motions(&d.cue(&Cue::Ailment {
            slot: a,
            ailment: Some(Ailment::Sleep),
        }));
        for _ in 0..5 {
            seen.extend(motions(&d.frame().orders));
        }
        // It lies down once.
        assert_eq!(seen, [motion::SLEEP]);
        // Asleep through its turn: it stays in the sleeping loop.
        let mut snoring = motions(&d.cue(&Cue::Sign {
            slot: a,
            sign: Sign::Snoring,
        }));
        for _ in 0..=STANCE_AFTER {
            snoring.extend(motions(&d.frame().orders));
        }
        assert!(snoring.is_empty(), "{snoring:?}");
        // Hit while asleep: back to sleeping after.
        d.cue(&Cue::Hurt {
            target: a,
            heavy: 0,
        });
        let mut after = Vec::new();
        for _ in 0..=STANCE_AFTER {
            after.extend(motions(&d.frame().orders));
        }
        assert_eq!(after.last(), Some(&motion::SLEEP));
        d.cue(&Cue::Ailment {
            slot: a,
            ailment: None,
        });
        let woke = motions(&d.frame().orders);
        assert_eq!(woke, [motion::STANCE]);
    }

    #[test]
    fn a_frozen_chao_sits_in_ice_until_it_thaws() {
        let mut d = director(true);
        let (a, _) = show(&mut d);
        let ice = |d: &Director| {
            d.scene()
                .draws
                .iter()
                .filter(|x| x.key.tag == 10_200)
                .count()
        };
        d.cue(&Cue::Ailment {
            slot: a,
            ailment: Some(Ailment::Freeze),
        });
        d.frame();
        assert_eq!(ice(&d), 1);
        // Standing on the floor around the Chao, about its height.
        let draws = d.scene().draws;
        let block = draws.iter().find(|x| x.key.tag == 10_200).unwrap();
        assert_eq!(block.size, ICE_SIZE);
        assert_eq!(block.at[1] - block.size / 2.0, 8.0);
        // Frozen through its turn: still one block.
        d.cue(&Cue::Sign {
            slot: a,
            sign: Sign::Frozen,
        });
        d.frame();
        assert_eq!(ice(&d), 1);
        for _ in 0..400 {
            d.frame();
            assert!(ice(&d) >= 1);
        }
        d.cue(&Cue::Ailment {
            slot: a,
            ailment: None,
        });
        d.frame();
        assert_eq!(ice(&d), 0);
    }

    #[test]
    fn signs_show_balls_and_shivers() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let orders = d.cue(&Cue::Sign {
            slot: a,
            sign: Sign::Flinched,
        });
        assert!(orders.contains(&Order::Ball {
            slot: a,
            state: ball::EXCLAIM,
            frames: 90
        }));
        let orders = d.cue(&Cue::Confusion { slot: b, on: true });
        assert!(orders.iter().any(|o| matches!(
            o,
            Order::Ball {
                state: ball::SWIRL,
                ..
            }
        )));
        d.cue(&Cue::Sign {
            slot: a,
            sign: Sign::Paralyzed,
        });
        let moved = (0..10).any(|_| {
            let out = d.frame();
            out.places.iter().any(|(s, at, _)| *s == a && at[0] != -5.0)
        });
        assert!(moved);
    }

    #[test]
    fn without_the_effects_only_the_karate_motions_play() {
        let mut d = director(false);
        let (a, b) = show(&mut d);
        let orders = d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 2,
        });
        // A special move is a karate attack, with no sprites.
        assert!(motion::ATTACKS.contains(&motions(&orders)[0]));
        let orders = d.cue(&Cue::Attack {
            by: b,
            toward: Some(a),
            status: true,
            move_id: 3,
        });
        assert_eq!(motions(&orders), [motion::WIND_UP]);
        for sign in [
            Sign::Struck { strong: true },
            Sign::Flinched,
            Sign::Confused,
        ] {
            assert!(d.cue(&Cue::Sign { slot: b, sign }).is_empty());
        }
        assert!(
            d.cue(&Cue::Ailment {
                slot: a,
                ailment: Some(Ailment::Sleep)
            })
            .is_empty()
        );
        for _ in 0..60 {
            d.frame();
            assert!(d.scene().draws.is_empty());
        }
        assert!(d.quiet());
    }

    #[test]
    fn a_hop_lifts_and_lands_with_a_shake() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let spread_id = 4;
        let mut dex_moves = vec![MoveData {
            target: Target::BothFoes,
            ..mv(spread_id, types::GROUND, 100)
        }];
        dex_moves.push(mv(1, types::NORMAL, 40));
        d.dex = Some(Arc::new(Dex::for_tests(&dex_moves, TypeChart::default())));
        let orders = d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: spread_id,
        });
        assert_eq!(motions(&orders), [motion::HOP]);
        let mut highest: f32 = 0.0;
        let mut shook = false;
        for _ in 0..40 {
            let out = d.frame();
            highest = highest.max(out.places[0].2);
            shook |= d.shake() != [0.0; 3];
        }
        assert!(highest > 1.0, "{highest}");
        assert!(shook);
    }

    #[test]
    fn animation_frames_loop_by_their_durations() {
        let l = look(1);
        assert_eq!(frame_at(&l, 0).tile, 0);
        assert_eq!(frame_at(&l, 2).tile, 0);
        assert_eq!(frame_at(&l, 3).tile, 8);
        assert_eq!(frame_at(&l, 6).tile, 0);
    }
}

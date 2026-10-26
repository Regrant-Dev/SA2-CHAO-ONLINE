//! How the arena shows a battle (re/chao-battle-visuals.md): the Chao's motions for each kind of
//! move, signs of their status, the catalog's effect sprites moving the way each move's script
//! moves them (`movement`), sparkles and other particles, the target shaking, a flash and a
//! camera shake on strong hits, and sounds.
//!
//! `Director` takes the narrator's cues and says, frame by frame, what the arena does (where
//! each Chao stands, which motions, sounds, emotion balls and particles start); game.rs carries
//! that out. With the `BattleEffects` setting off it only plays the karate motions: an attack
//! for a damaging move, a wind-up for a status move, the hits, the knock-down and the result.
//! No game memory here.
//!
//! A move's kind comes from its catalog record (the first row that fits):
//!
//! | kind | moves | the Chao |
//! |---|---|---|
//! | sound | the catalog's sound-move list | sings or plays a tune |
//! | rest | full heal and sleep | lies down to sleep |
//! | heal | the healing effects | eats, munches or soaks it up |
//! | protect | the shielding and bracing effects | guards, dodges or cowers |
//! | weather | the weather moves | dances |
//! | inflict | status moves giving the foe a status | yawns, spins, swoons, glares or teases |
//! | raise self | status moves raising the user's stats | cheers |
//! | lower foe | status moves lowering the foe's stats, provoking | glares or taunts |
//! | charge | other status moves | winds up |
//! | one-hit | the one-hit knock-outs | its biggest jump or kick |
//! | two-turn | the charging and hiding moves | gets ready, then strikes |
//! | multi-hit | the moves hitting two to five times | a flurry, stepping in for each hit |
//! | recoil | the moves hurting their user | charges head down |
//! | drain | the draining moves | reaches out and soaks it up |
//! | quick | the moves going first | a quick dash |
//! | contact | damaging, with the contact flag | lunges, swings, kicks or pounces |
//! | water | other water moves | swims a stroke in a splash |
//! | flying | other flying moves | flaps and takes off |
//! | spread | other physical moves hitting both foes or everyone | jumps and lands |
//! | physical | other physical moves | throws or swings |
//! | special | the rest | a focus pose, or rears up breathing fire |
//!
//! Within its kind each move has its own motion, picked by its place among the kind's moves
//! (`vary`), so neighbouring moves look different; the sprites leave at that motion's moment.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::data::{MoveData, Target, flags, types};
use super::effects as fx;
use super::effects_gfx::{Book, Draw, Key, Movement, Particle, Place, Scene, Template, status};
use super::event::Slot;
use super::fighter::{Ailment, Hidden};
use super::movement::{self, Way};
use super::narrate::{ATTACK, Cue, Sign};
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
    pub const SING: u16 = 0x143;
    pub const GUARD: u16 = 0x241;
    /// Lying down to sleep (it goes on to the sleeping loop).
    pub const SLEEP: u16 = 0x12;
    pub const EAT: u16 = 0xA9;
    pub const BITE: u16 = 0xAF;
    pub const STUMBLE: u16 = 0x9F;
    /// A short shudder (an illness motion).
    pub const SHUDDER: u16 = 0xF8;
    /// Taking off, which goes on to the flap.
    pub const TAKE_OFF: u16 = 0x83;

    /// A motion for a move, and the frame of its moment (when the move's sprites leave).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Pose {
        pub id: u16,
        pub at: u16,
    }

    const fn p(id: u16, at: u16) -> Pose {
        Pose { id, at }
    }

    /// Jumping lunge, overhand swing, spinning jump kick, head-butt, pounce, diving slam.
    pub const CONTACT: [Pose; 6] = [
        p(0x243, 18),
        p(0x246, 22),
        p(0x244, 38),
        p(0x1BD, 16),
        p(0x1D4, 18),
        p(0x238, 20),
    ];
    /// Seated throw, hop and swing, overhand swing, lean in.
    pub const PHYSICAL: [Pose; 4] = [p(THROW, 10), p(0x1B9, 28), p(0x246, 22), p(0x1C1, 16)];
    /// Hop, high hop, quick hop, bunny hops; the moment is the landing.
    pub const SPREAD: [Pose; 4] = [p(HOP, 24), p(0x259, 26), p(0x207, 20), p(0x135, 22)];
    /// Stroke, short stroke, trick roll, blowing bubbles, pouring.
    pub const WATER: [Pose; 5] = [
        p(SWIM, 10),
        p(0x94, 8),
        p(0x92, 20),
        p(0x146, 20),
        p(0xEB, 24),
    ];
    /// Flap, take-off, airborne, rise and hover.
    pub const FLYING: [Pose; 4] = [p(FLAP, 12), p(TAKE_OFF, 16), p(0x87, 14), p(0x14C, 18)];
    /// Focus poses, leaning in, standing tall.
    pub const SPECIAL: [Pose; 5] = [
        p(0x253, 14),
        p(0x254, 14),
        p(0x26D, 14),
        p(0x11C, 18),
        p(0x1C0, 16),
    ];
    /// Rearing up, as the game lights a flame at its hand (fire and dragon moves).
    pub const FLAME: Pose = p(0x11D, 20);
    /// Singing, and miming a tune or a drum.
    pub const SOUND: [Pose; 6] = [
        p(SING, 8),
        p(0x145, 8),
        p(0x163, 8),
        p(0x165, 8),
        p(0x16B, 8),
        p(0x14F, 8),
    ];
    /// Cheers, a happy hop, a flex.
    pub const CHEER: [Pose; 5] = [
        p(0x24D, 6),
        p(0x24E, 6),
        p(0x1EC, 6),
        p(0x1CF, 6),
        p(0x11A, 8),
    ];
    /// A glare, a wiggle, sways and taunts.
    pub const TAUNT: [Pose; 6] = [
        p(0x1BF, 28),
        p(0x1C3, 12),
        p(0x187, 10),
        p(0x136, 12),
        p(0x18D, 10),
        p(0x181, 10),
    ];
    /// Guard, a dodging sway, cowering.
    pub const GUARDS: [Pose; 3] = [p(GUARD, 4), p(0x248, 4), p(0x1F1, 6)];
    /// Eating (then biting), munching, soaking it up with a spin, pleased.
    pub const HEAL: [Pose; 4] = [p(EAT, 10), p(0xB7, 10), p(0xCB, 10), p(0xFA, 10)];
    /// Wind-up, a lean, noticing, standing tall, leaning in, leaning in and back.
    pub const CHARGE: [Pose; 6] = [
        p(WIND_UP, 12),
        p(0x13F, 14),
        p(0x0D, 10),
        p(0x1C0, 14),
        p(0x11C, 18),
        p(0x1C1, 16),
    ];
    /// Punch flurry, bunny hops, rolling flips, a charging run (they loop through the hits).
    pub const MULTI: [Pose; 4] = [p(0x245, 10), p(0x135, 12), p(0x133, 19), p(0x81, 8)];
    /// A head-down charge, a jumping lunge, a diving slam.
    pub const RECOIL: [Pose; 3] = [p(0x140, 14), p(0x243, 18), p(0x238, 20)];
    /// Soaking it up with a spin, a grab, reaching out.
    pub const DRAIN: [Pose; 3] = [p(0xCB, 10), p(0xCA, 6), p(0xBC, 14)];
    /// Spinning jump kick, double high jump, a big hop, rolling flips.
    pub const ONE_HIT: [Pose; 4] = [p(0x244, 38), p(0x1BB, 30), p(0xCE, 10), p(0x133, 19)];
    /// A dash and pounce, a quick step, a fast run, a run.
    pub const QUICK: [Pose; 4] = [p(0x1D4, 18), p(0x171, 10), p(0x77, 6), p(0x66, 6)];
    /// Dances.
    pub const DANCE: [Pose; 6] = [
        p(0x184, 10),
        p(0x183, 10),
        p(0x17E, 10),
        p(0x15D, 10),
        p(0x138, 10),
        p(0x16F, 10),
    ];
    /// Teasing, for a status with no motion of its own.
    pub const TEASE: [Pose; 3] = [p(0x1C3, 12), p(0x1B9, 28), p(0x1BD, 16)];
    /// For the status given: a yawn (sleep), a spin (confusion), a swoon (infatuation),
    /// mischief (poison), a glare (paralysis).
    pub const YAWN: Pose = p(0x27, 14);
    pub const SPIN: Pose = p(0x17E, 12);
    pub const SWOON: Pose = p(0x18C, 12);
    pub const MISCHIEF: Pose = p(0x203, 8);
    pub const GLARE: Pose = p(0x1BF, 28);
    /// A two-turn move getting ready: rising to hover (into the air), digging (underground),
    /// a diving roll (underwater), else focusing, leaning or winding up.
    pub const RISE_UP: Pose = p(0x14C, 10);
    pub const DIG: Pose = p(0xE5, 10);
    pub const DIVE: Pose = p(0x92, 10);
    pub const READY: [Pose; 3] = [p(0x253, 14), p(0x13F, 14), p(WIND_UP, 12)];
    /// Its second turn: coming down from the air, jumping out of the ground (leaving it, in a
    /// cloud of dust, at `BURST_OUT_TAKE_OFF`).
    pub const SWOOP: Pose = p(0x8C, 12);
    pub const BURST_OUT: Pose = p(0x1BB, 26);
    pub const BURST_OUT_TAKE_OFF: u16 = 13;

    /// Every pose a move can play.
    pub fn all() -> impl Iterator<Item = Pose> {
        [
            &CONTACT[..],
            &PHYSICAL,
            &SPREAD,
            &WATER,
            &FLYING,
            &SPECIAL,
            &SOUND,
            &CHEER,
            &TAUNT,
            &GUARDS,
            &HEAL,
            &CHARGE,
            &MULTI,
            &RECOIL,
            &DRAIN,
            &ONE_HIT,
            &QUICK,
            &DANCE,
            &TEASE,
            &READY,
            &[
                FLAME, YAWN, SPIN, SWOON, MISCHIEF, GLARE, RISE_UP, DIG, DIVE, SWOOP, BURST_OUT,
            ],
        ]
        .into_iter()
        .flatten()
        .copied()
    }
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
    pub const HEART: u8 = 4;
    pub const SWIRL: u8 = 5;
}

/// Frames a Chao appears for before its stance starts.
const FIRST_STANCE: u16 = 3;
/// Frames after an action before the rest pose comes back.
const STANCE_AFTER: u16 = 50;
/// An attack's step toward its target: how far and how many frames out and back.
const STEP_DISTANCE: f32 = 3.0;
const STEP_FRAMES: u16 = 30;
/// A quick move's dash, and a multi-hit move's step for each further hit.
const QUICK_STEP: u16 = 16;
const JAB_STEP: u16 = 20;
/// Frames a multi-hit move's flurry goes on after each hit.
const FLURRY_AFTER: u16 = 40;
/// A hop's frames in the air.
const HOP_FRAMES: u16 = 24;
/// How high a Chao hovers while its two-turn move keeps it up in the air.
const AWAY_LIFT: f32 = 2.0;
/// A damaging move's sprites land on the target this many frames into the attack's beat
/// (the hit follows when the beat ends): no later than `LAND_BY`, no sooner than
/// `LAND_FROM`.
const LAND_BY: u16 = ATTACK - 4;
const LAND_FROM: u16 = LAND_BY - 12;
/// Frames after its move's sprites land on a Chao that the hit's own burst isn't shown
/// (they were its burst).
const LANDED_FOR: u16 = 20;
/// A status sign's burst (frames).
const BURST_FRAMES: u16 = 24;
/// Sprites alive at once at most.
const MAX_SPRITES: usize = 40;
/// World units per source sprite pixel, and the smallest and biggest a sprite is drawn.
const PIXEL: f32 = 0.1;
const MIN_SIZE: f32 = 1.2;
const MAX_SIZE: f32 = 4.0;
/// Heights above a Chao's feet: its chest and the top of its head.
const CHEST: f32 = 1.3;
const HEAD: f32 = 2.6;
/// The flash's opacity at its start and length; the shake's size and length.
const FLASH_ALPHA: f32 = 0.55;
const FLASH_FRAMES: u16 = 10;
const WHITE: u32 = 0xFF_FFFF;
/// A move's tint as a flash over the screen: its opacity and length.
const TINT_ALPHA: f32 = 0.28;
const TINT_FRAMES: u16 = 8;
/// A big hit's own flash.
const BIG_ALPHA: f32 = 0.35;
const SHAKE_SIZE: f32 = 0.35;
const SHAKE_FRAMES: u16 = 14;
/// A target's shudder as a move's sprites land on it.
const STRUCK_SHIVER: u16 = 12;
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
    Weather,
    Inflict,
    OneHit,
    TwoTurn,
    MultiHit,
    Recoil,
    Drain,
    Quick,
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
    ) || [fx::TARGET_ATTACK_DEFENSE_DOWN, fx::PROVOKE].contains(&effect)
}

/// What status a status move's effect gives the foe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gives {
    Sleep,
    Confusion,
    Infatuation,
    Poison,
    Paralysis,
    Burn,
}

fn gives(effect: u8) -> Option<Gives> {
    Some(match effect {
        fx::SLEEP | fx::DROWSE => Gives::Sleep,
        fx::CONFUSE | fx::CONFUSE_ALL | fx::CONFUSE_ATTACK_UP2 | fx::CONFUSE_SP_ATTACK_UP => {
            Gives::Confusion
        }
        fx::INFATUATE => Gives::Infatuation,
        fx::POISON | fx::BAD_POISON => Gives::Poison,
        fx::PARALYZE => Gives::Paralysis,
        fx::BURN => Gives::Burn,
        _ => return None,
    })
}

/// The kind of move `m` (`sound_moves`: the catalog's sound-move list).
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
            fx::RAIN | fx::SUN | fx::SAND | fx::HAIL => Kind::Weather,
            e if gives(e).is_some() => Kind::Inflict,
            e if raises_self(e) => Kind::RaiseSelf,
            e if lowers_foe(e) => Kind::LowerFoe,
            _ => Kind::Charge,
        };
    }
    match m.effect {
        fx::ONE_HIT_KO => return Kind::OneHit,
        e if fx::charges(e) => return Kind::TwoTurn,
        fx::MULTI_HIT | fx::TWO_HITS | fx::TWO_HITS_POISON_CHANCE | fx::TRIPLE_RISING => {
            return Kind::MultiHit;
        }
        fx::RECOIL_QUARTER | fx::RECOIL_THIRD | fx::CRASH_ON_MISS => return Kind::Recoil,
        fx::DRAIN_HALF | fx::DRAIN_SLEEPER => return Kind::Drain,
        _ => {}
    }
    if m.priority > 0 || m.effect == fx::FIRST_TURN_FLINCH {
        return Kind::Quick;
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

/// One of `n` choices for the `nth` move of a kind: neighbours never pick the same one (for n
/// of 3 or more), and each round of `n` moves takes them in a shifted order.
fn vary(nth: usize, n: usize) -> usize {
    let n = n.max(1);
    (nth + nth / n) % n
}

/// Each move's place among the moves of its kind, by id.
fn places(dex: &Dex) -> HashMap<u16, usize> {
    let mut moves = dex.table();
    moves.sort_by_key(|m| m.id);
    let mut seen = HashMap::<Kind, usize>::new();
    moves
        .iter()
        .map(|m| {
            let n = seen.entry(classify(m, dex.sound_moves())).or_insert(0);
            *n += 1;
            (m.id, *n - 1)
        })
        .collect()
}

/// How a Chao plays one move.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Act {
    pose: motion::Pose,
    blend: u32,
    /// Steps toward the target and back over this many frames, reaching furthest at the
    /// pose's moment.
    step: Option<u16>,
    /// Lifts off the floor: frames, height.
    lift: Option<(u16, f32)>,
    /// Lands (or leaves the ground) with dust and a shake at this frame.
    land: Option<u16>,
    /// Frames before the stance comes back.
    hold: u16,
}

/// How a Chao plays the `nth` move of `kind` (`m`: its record; `charging`: the first turn of
/// a two-turn move). The full show only.
fn act(kind: Kind, m: Option<&MoveData>, nth: usize, charging: bool) -> Act {
    use motion::*;
    let pick = |set: &[Pose]| set[vary(nth, set.len())];
    let contact = m.is_some_and(|m| m.flags & flags::CONTACT != 0);
    let step = contact.then_some(STEP_FRAMES);
    let plain = |pose: Pose, blend: u32| Act {
        pose,
        blend,
        step: None,
        lift: None,
        land: None,
        hold: STANCE_AFTER,
    };
    let mut a = match kind {
        Kind::Contact => Act {
            step: Some(STEP_FRAMES),
            ..plain(pick(&CONTACT), 15)
        },
        Kind::Physical => plain(pick(&PHYSICAL), 10),
        Kind::Spread => {
            let pose = pick(&SPREAD);
            Act {
                // The hop's own lift; the others jump in the motion itself.
                lift: (pose.id == HOP).then_some((HOP_FRAMES, 1.6)),
                land: Some(pose.at),
                ..plain(pose, 8)
            }
        }
        Kind::Water => plain(pick(&WATER), 10),
        Kind::Flying => {
            let pose = pick(&FLYING);
            let height = if matches!(pose.id, FLAP | TAKE_OFF) {
                1.2
            } else {
                0.6
            };
            Act {
                lift: Some((40, height)),
                ..plain(pose, 8)
            }
        }
        Kind::Special => {
            let fiery = m.is_some_and(|m| matches!(m.type_id, types::FIRE | types::DRAGON));
            plain(if fiery { FLAME } else { pick(&SPECIAL) }, 15)
        }
        Kind::Sound => plain(pick(&SOUND), 10),
        Kind::RaiseSelf => plain(pick(&CHEER), 12),
        Kind::LowerFoe => plain(pick(&TAUNT), 12),
        Kind::Protect => plain(pick(&GUARDS), 6),
        Kind::Rest => Act {
            hold: REST_LIE,
            ..plain(Pose { id: SLEEP, at: 10 }, 20)
        },
        Kind::Heal => plain(pick(&HEAL), 10),
        Kind::Charge => plain(pick(&CHARGE), 20),
        Kind::Weather => plain(pick(&DANCE), 12),
        Kind::Inflict => {
            let pose = match m.and_then(|m| gives(m.effect)) {
                Some(Gives::Sleep) => YAWN,
                Some(Gives::Confusion) => SPIN,
                Some(Gives::Infatuation) => SWOON,
                Some(Gives::Poison) => MISCHIEF,
                Some(Gives::Paralysis) => GLARE,
                Some(Gives::Burn) => FLAME,
                None => pick(&TEASE),
            };
            plain(pose, 10)
        }
        Kind::OneHit => Act {
            step,
            ..plain(pick(&ONE_HIT), 12)
        },
        Kind::TwoTurn => {
            let hidden = m.and_then(fx::hides);
            if charging {
                let pose = match hidden {
                    Some(Hidden::Air) => RISE_UP,
                    Some(Hidden::Underground) => DIG,
                    Some(Hidden::Underwater) => DIVE,
                    None => pick(&READY),
                };
                Act {
                    lift: (hidden == Some(Hidden::Air)).then_some((40, AWAY_LIFT)),
                    ..plain(pose, 12)
                }
            } else {
                let pose = match hidden {
                    Some(Hidden::Air) => SWOOP,
                    Some(Hidden::Underground) => BURST_OUT,
                    _ if contact => pick(&CONTACT[..3]),
                    // Leaning in or standing tall, never the getting-ready pose again.
                    _ => pick(&SPECIAL[3..]),
                };
                Act {
                    step,
                    land: (hidden == Some(Hidden::Underground)).then_some(BURST_OUT_TAKE_OFF),
                    ..plain(pose, 12)
                }
            }
        }
        Kind::MultiHit => Act {
            step,
            // Still at it as the first hit lands (`Director::flurry`).
            hold: STANCE_AFTER + 20,
            ..plain(pick(&MULTI), 10)
        },
        Kind::Recoil => Act {
            step,
            ..plain(pick(&RECOIL), 10)
        },
        Kind::Drain => plain(pick(&DRAIN), 10),
        Kind::Quick => {
            // A dash, long enough to reach furthest at the moment.
            let pose = pick(&QUICK);
            Act {
                step: Some(QUICK_STEP.max(pose.at + 4)),
                ..plain(pose, 4)
            }
        }
    };
    // A late moment keeps the stance waiting until the step is back.
    if let Some(len) = a.step {
        let back = a.pose.at.saturating_sub(len / 2) + len;
        a.hold = a.hold.max(back + 4);
    }
    a
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
    /// Steps toward this point, out and back over this many frames.
    Step([f32; 3], u16),
}

/// How a sprite moves over its life.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Path {
    Move(Way),
    /// Still, standing on `to`, about a Chao's height.
    Stay,
}

/// A status sign's paths.
const SIGN_BURST: Path = Path::Move(Way::Burst);
const SIGN_RISE: Path = Path::Move(Way::Rise { height: 1.5 });
const SIGN_ORBIT: Path = Path::Move(Way::Orbit {
    radius: 0.8,
    phase: 0.0,
    turns: 2.0,
});

/// Something that happens as a move's sprites land.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Due {
    /// They land on the Chao at the slot, which shudders for this many frames; `burst`: they
    /// reach it (and stand for the hit's burst).
    Land {
        slot: Slot,
        shiver: u16,
        burst: bool,
    },
    /// The camera shakes and the screen flashes white.
    Big,
    /// The screen flashes this colour (0xRRGGBB).
    Tint(u32),
}

/// A BGR555 colour as 0xRRGGBB (`None` for one too dark to show as a flash).
fn tint_colour(c: u16) -> Option<u32> {
    let ch = |s: u16| {
        let x = u32::from(c >> s & 31);
        x << 3 | x >> 2
    };
    let (r, g, b) = (ch(0), ch(5), ch(10));
    (r.max(g).max(b) >= 0x40).then_some(r << 16 | g << 8 | b)
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
    /// Moves with the Chao at this slot, which stood this far from its home when the sprite
    /// was placed.
    follow: Option<(Slot, [f32; 3])>,
    /// The move launch it belongs to (0: none).
    volley: u32,
    delay: u16,
    age: u16,
    life: u16,
}

/// One Chao in the ring.
#[derive(Clone, Debug, PartialEq)]
struct Stand {
    home: [f32; 3],
    /// An attack's step toward this point: frames played, of how many.
    step: Option<([f32; 3], u16, u16)>,
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
    /// Away from the field for its two-turn move (up in the air: it hovers there).
    away: Option<Hidden>,
    /// How far its step and lift put it from its home this frame.
    offset: [f32; 3],
}

/// How far a step toward `target` from `home` reaches, `f` frames of `frames` in (x, z).
fn reach(home: [f32; 3], target: [f32; 3], f: u16, frames: u16) -> [f32; 2] {
    let t = f32::from(f) / f32::from(frames.max(1));
    let (dx, dz) = (target[0] - home[0], target[2] - home[2]);
    let length = (dx * dx + dz * dz).sqrt().max(1.0);
    let r = (t * std::f32::consts::PI).sin() * STEP_DISTANCE;
    [dx / length * r, dz / length * r]
}

/// How high a lift of `frames` up to `height` is `f` frames in.
fn lift_at(f: u16, frames: u16, height: f32) -> f32 {
    let t = f32::from(f) / f32::from(frames.max(1));
    (t * std::f32::consts::PI).sin().max(0.0) * height
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
            away: None,
            offset: [0.0; 3],
        }
    }

    /// How far its step and lift put it from its home on the `n`th frame from now (as `frame`
    /// will).
    fn offset_in(&self, n: u16) -> [f32; 3] {
        let n = n.max(1);
        let step = match self.step {
            Some((target, f, frames)) => Some((target, f + n - 1, frames)),
            None => self.later.iter().find_map(|&(wait, l)| match l {
                Later::Step(target, frames) if n > wait => Some((target, n - wait - 1, frames)),
                _ => None,
            }),
        };
        let mut out = [0.0; 3];
        if let Some((target, f, frames)) = step.filter(|&(_, f, frames)| f <= frames) {
            let [x, z] = reach(self.home, target, f, frames);
            out[0] = x;
            out[2] = z;
        }
        let lift = self
            .lift
            .map(|(f, frames, height)| (f + n - 1, frames, height))
            .filter(|&(f, frames, _)| f <= frames);
        if let Some((f, frames, height)) = lift {
            out[1] = lift_at(f, frames, height);
        }
        if self.away == Some(Hidden::Air) && !lift.is_some_and(|(f, frames, _)| f <= frames / 2) {
            out[1] = AWAY_LIFT;
        }
        out
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

    /// Busy with an action or away (the rest pose and status signs wait).
    fn busy(&self) -> bool {
        self.rest_in > 0
            || !self.later.is_empty()
            || self.step.is_some()
            || self.lift.is_some()
            || self.away.is_some()
    }
}

/// The arena's director for one battle.
pub struct Director {
    effects: bool,
    /// Where each slot stands.
    homes: [[[f32; 3]; 2]; 2],
    stands: [[Option<Stand>; 2]; 2],
    sprites: Vec<Sprite>,
    /// Frames of flash left, of how many, its opacity at the start and colour.
    flash: u16,
    flash_len: u16,
    flash_alpha: f32,
    flash_colour: u32,
    shake: u16,
    /// What happens as sprites land, with the frames until then and their launch.
    due: Vec<(u16, u32, Due)>,
    /// Launches so far (each move's sprites carry theirs).
    volleys: u32,
    /// Frames left that each slot's move sprites have just landed on it.
    landed: [[u16; 2]; 2],
    /// The Chao the last hit's burst was on (a multi-hit move's next hit; other hurts aren't).
    struck: Option<Slot>,
    /// Moves whose look was told this battle (`told`).
    told: Vec<u16>,
    /// Varies the voices and the result poses.
    pick: usize,
    /// Each move's place among its kind's moves (`places`).
    places: HashMap<u16, usize>,
    /// The last move's user and kind, and whether it makes contact (a multi-hit move's
    /// flurry goes on through its hits).
    last: Option<(Slot, Kind, bool)>,
    frame: u32,
    dex: Option<Arc<Dex>>,
    book: Option<Arc<Book>>,
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
            flash_len: FLASH_FRAMES,
            flash_alpha: FLASH_ALPHA,
            flash_colour: WHITE,
            shake: 0,
            due: Vec::new(),
            volleys: 0,
            landed: [[0; 2]; 2],
            struck: None,
            told: Vec::new(),
            pick: 0,
            places: dex.as_deref().map(places).unwrap_or_default(),
            last: None,
            frame: 0,
            dex,
            book,
        }
    }

    /// Move `id`'s place among its kind's moves (its id without the moves' records).
    fn nth(&self, id: u16) -> usize {
        self.places.get(&id).copied().unwrap_or(usize::from(id))
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
            follow: Some((slot, [0.0; 3])),
            volley: 0,
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
                charging,
            } => self.attack(by, toward, status, move_id, charging, &mut out),
            Cue::Hurt { target, heavy } => {
                // A move's hit (its burst came first), not poison, weather and the like.
                if self.struck.take() == Some(target) {
                    self.flurry(target);
                }
                if let Some(s) = self.stand(target) {
                    s.away = None;
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
                    s.away = None;
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
                        s.away = None;
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
                    self.status_sprite(slot, status::CONFUSION, SIGN_ORBIT, self.head_of(slot), 90);
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
        charging: bool,
        out: &mut Vec<Order>,
    ) {
        let kind = self.kind(move_id, status);
        let data = self.dex.as_ref().and_then(|d| d.move_data(move_id));
        let contact = data.is_some_and(|m| m.flags & flags::CONTACT != 0);
        self.last = Some((by, kind, contact));
        let nth = self.nth(move_id);
        let pick = self.pick;
        let effects = self.effects;
        let target = toward.map(|t| self.home(t));
        if effects {
            self.cut_short();
        }
        let Some(s) = self.stand(by) else { return };
        let (chest, feet) = (s.chest(), s.feet());
        let motion = |id: u16, blend: u32| Order::Motion {
            slot: by,
            id,
            blend,
        };
        s.rest_in = STANCE_AFTER;
        s.later.clear();
        let was_away = s.away.take();
        let voice = Order::Sound(sound::ATTACK_VOICES[pick % sound::ATTACK_VOICES.len()]);
        if !effects {
            // The karate motions only.
            if kind == Kind::Contact {
                out.push(motion(motion::ATTACKS[pick % motion::ATTACKS.len()], 15));
                out.push(voice);
                s.step = target.map(|t| (t, 0, STEP_FRAMES));
            } else {
                out.push(motion(motion::WIND_UP, 20));
            }
            return;
        }
        let a = act(kind, data.as_ref(), nth, charging);
        let at = a.pose.at;
        out.push(motion(a.pose.id, a.blend));
        s.rest_in = a.hold;
        if let (Some(frames), Some(t)) = (a.step, target) {
            match at.saturating_sub(frames / 2) {
                0 => s.step = Some((t, 0, frames)),
                wait => s.later.push((wait, Later::Step(t, frames))),
            }
        }
        if let Some((frames, height)) = a.lift {
            s.lift = Some((0, frames, height));
        }
        if was_away == Some(Hidden::Air) && a.lift.is_none() {
            // Down from the air by the moment.
            let down = at.max(1);
            s.lift = Some((down, down * 2, AWAY_LIFT));
        }
        if charging && kind == Kind::TwoTurn {
            s.away = data.as_ref().and_then(fx::hides);
        }
        if let Some(land) = a.land {
            s.later.push((land, Later::Particles(Particle::Dust, 6)));
            s.later.push((land, Later::Shake(8)));
        }
        let sparkle = |count| Order::Particles {
            kind: Particle::Sparkle,
            at: chest,
            count,
        };
        match kind {
            Kind::Contact
            | Kind::Physical
            | Kind::OneHit
            | Kind::MultiHit
            | Kind::Recoil
            | Kind::Quick => out.push(voice),
            Kind::TwoTurn if !charging => out.push(voice),
            Kind::TwoTurn => match data.as_ref().and_then(fx::hides) {
                Some(Hidden::Underground) => out.push(Order::Particles {
                    kind: Particle::Dust,
                    at: feet,
                    count: 8,
                }),
                Some(Hidden::Underwater) => {
                    out.push(Order::Sound(sound::SPLASH));
                    s.later.push((at, Later::Particles(Particle::Splash, 8)));
                }
                Some(Hidden::Air) => s.later.push((at, Later::Particles(Particle::Dust, 5))),
                None => s.later.push((at, Later::Particles(Particle::Sparkle, 8))),
            },
            Kind::Water => {
                out.push(Order::Sound(sound::SPLASH));
                out.push(Order::Particles {
                    kind: Particle::Splash,
                    at: chest,
                    count: 6,
                });
                s.later.push((15, Later::Particles(Particle::Splash, 6)));
            }
            Kind::Special | Kind::Inflict if a.pose == motion::FLAME => {
                s.later.push((at, Later::Particles(Particle::Flame, 5)));
            }
            Kind::Special => out.push(sparkle(4)),
            Kind::Inflict if a.pose == motion::SWOON => out.push(Order::Ball {
                slot: by,
                state: ball::HEART,
                frames: 90,
            }),
            Kind::Inflict | Kind::LowerFoe | Kind::Charge | Kind::Spread | Kind::Flying => {}
            Kind::Sound => out.push(Order::Sound(sound::SINGING)),
            Kind::RaiseSelf => {
                out.push(Order::Sound(sound::JOY[pick % sound::JOY.len()]));
                s.later.push((5, Later::Particles(Particle::Sparkle, 8)));
            }
            Kind::Protect => out.push(sparkle(6)),
            Kind::Weather => s.later.push((at, Later::Particles(Particle::Sparkle, 6))),
            Kind::Rest => {
                s.lying = true;
                s.later
                    .push((30, Later::Sound(sound::SNORES[pick % sound::SNORES.len()])));
            }
            Kind::Heal => {
                if a.pose.id == motion::EAT {
                    s.later.push((27, Later::Motion(motion::BITE, 8)));
                }
                s.later.push((20, Later::Particles(Particle::Sparkle, 8)));
            }
            // As the drained sprites come back.
            Kind::Drain => s
                .later
                .push((at + 24, Later::Particles(Particle::Sparkle, 8))),
        }
        // Getting ready shows no sprites; the move's own show comes with its strike.
        if !charging || kind != Kind::TwoTurn {
            self.launch(by, toward, kind, move_id, at);
        }
    }

    /// A multi-hit move's next hit on `target`: its user, still at it, flurries on, stepping
    /// in again for a contact move.
    fn flurry(&mut self, target: Slot) {
        let Some((by, Kind::MultiHit, contact)) = self.last else {
            return;
        };
        if by == target || !self.effects {
            return;
        }
        let to = self.home(target);
        let Some(s) = self.stand(by).filter(|s| !s.down && s.rest_in > 0) else {
            return;
        };
        s.rest_in = s.rest_in.max(FLURRY_AFTER);
        if contact {
            // Once any step under way is back.
            match s.step {
                None => s.step = Some((to, 0, JAB_STEP)),
                Some((_, f, frames)) => s
                    .later
                    .push((frames.saturating_sub(f) + 1, Later::Step(to, JAB_STEP))),
            }
        }
    }

    /// The flash: `frames` long, `alpha` at its start, in `colour` (0xRRGGBB); a weaker one
    /// doesn't cut a stronger one short.
    fn start_flash(&mut self, frames: u16, alpha: f32, colour: u32) {
        let now = if self.flash > 0 {
            self.flash_alpha * f32::from(self.flash) / f32::from(self.flash_len.max(1))
        } else {
            0.0
        };
        if alpha >= now {
            self.flash = frames;
            self.flash_len = frames;
            self.flash_alpha = alpha;
            self.flash_colour = colour;
        }
    }

    /// Who move `id` from `by` aimed at `toward` lands on: `toward`, and its partner too for a
    /// move that hits both foes, or every other Chao standing for one that hits everyone else.
    fn targets(&self, by: Slot, toward: Slot, id: u16) -> Vec<Slot> {
        let mut out = vec![toward];
        let standing =
            |s: Slot| s != by && s != toward && self.stand_ref(s).is_some_and(|s| !s.down);
        match self
            .dex
            .as_ref()
            .and_then(|d| d.move_data(id))
            .map(|m| m.target)
        {
            Some(Target::BothFoes) => {
                let partner = Slot::new(toward.side, 1 - toward.pos.min(1));
                if standing(partner) {
                    out.push(partner);
                }
            }
            Some(Target::AllOthers) => {
                for side in 0..2 {
                    for pos in 0..2 {
                        let s = Slot::new(side, pos);
                        if standing(s) {
                            out.push(s);
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// The last move's sprites still waiting to start go (its beat was cut short), with what
    /// they were to do as they landed; what's already in flight lands as it would.
    fn cut_short(&mut self) {
        let waiting = |s: &Sprite| s.on.is_none() && s.delay > 0;
        let mut dropped: Vec<u32> = self
            .sprites
            .iter()
            .filter(|s| waiting(s))
            .map(|s| s.volley)
            .collect();
        self.sprites.retain(|s| !waiting(s));
        dropped.retain(|v| !self.sprites.iter().any(|s| s.volley == *v));
        self.due.retain(|(_, v, _)| !dropped.contains(v));
    }

    /// Move `move_id`'s sprites from `by`, starting about `moment` frames from now, the way its
    /// script moves them (on the user for a move on itself), and what they do as they land. A
    /// damaging move's land late in the attack's beat, just before the hit.
    fn launch(&mut self, by: Slot, toward: Option<Slot>, kind: Kind, move_id: u16, moment: u16) {
        let Some(look) = self.book.as_ref().map(|b| b.for_move(move_id)) else {
            return;
        };
        // A move with no sprites may still shake things.
        if look.is_empty() && !look.shake && !look.big && look.flash.is_none() {
            return;
        }
        let aimed = toward.filter(|_| !kind.on_self() && !self.on_user(move_id));
        let damaging = aimed.is_some()
            && self
                .dex
                .as_ref()
                .and_then(|d| d.move_data(move_id))
                .is_some_and(|m| m.power > 0);
        // They reach the target (rather than staying on the user).
        let reaches = aimed.is_some()
            && (look.movement.travels() || look.place == Place::Target || look.impact.is_some());
        let home = self.chest_of(by);
        let lands =
            movement::volley(&look, home, aimed.map_or(home, |t| self.chest_of(t)), 0).lands;
        let launch = if damaging {
            moment.clamp(
                LAND_FROM.saturating_sub(lands),
                LAND_BY.saturating_sub(lands),
            )
        } else {
            moment
        };
        // Where the user is (stepping or lifted) as they leave.
        let offset = self.stand_ref(by).map_or([0.0; 3], |s| s.offset_in(launch));
        let user = std::array::from_fn(|i| home[i] + offset[i]);
        let targets = match aimed {
            Some(t) => self.targets(by, t, move_id),
            None => vec![by],
        };
        self.volleys = self.volleys.wrapping_add(1).max(1);
        let volley = self.volleys;
        let flip = by.side == 1;
        for (n, &t) in targets.iter().enumerate() {
            let target = if aimed.is_some() {
                self.chest_of(t)
            } else {
                user
            };
            let v = movement::volley(&look, user, target, move_id.wrapping_add(n as u16 * 31));
            // What stays on the user moves with it.
            let on_user =
                aimed.is_none() || look.place == Place::User || look.movement == Movement::Gather;
            let anchored = |way: Way| on_user && !matches!(way, Way::Line { .. });
            let follow = |way: Way| anchored(way).then_some((by, offset));
            let shots = look
                .main
                .iter()
                .flat_map(|main| v.shots.iter().map(move |s| (main, *s, follow(s.way))));
            let impact = look
                .impact
                .iter()
                .zip(v.impact)
                .map(|(l, s)| (l, s, aimed.is_none().then_some((by, offset))));
            let new: Vec<Sprite> = shots
                .chain(impact)
                .map(|(look, s, follow)| Sprite {
                    look: look.clone(),
                    path: Path::Move(s.way),
                    from: s.from,
                    to: s.to,
                    flip: flip != s.back,
                    on: None,
                    follow,
                    volley,
                    delay: launch + s.delay,
                    age: 0,
                    life: s.life,
                })
                .collect();
            for sprite in new {
                self.add_sprite(sprite);
            }
            let lands = launch + v.lands;
            if aimed.is_some() {
                // A damaging move's hit follows, with its own reaction.
                let shiver = if look.shake && !damaging {
                    STRUCK_SHIVER
                } else {
                    0
                };
                let land = Due::Land {
                    slot: t,
                    shiver,
                    burst: reaches,
                };
                self.due.push((lands, volley, land));
            }
            if n == 0 {
                if look.big {
                    self.due.push((lands, volley, Due::Big));
                }
                if let Some(c) = look.flash.and_then(tint_colour) {
                    self.due.push((lands, volley, Due::Tint(c)));
                }
            }
        }
    }

    /// A moment's sign on the Chao at `slot` (the full show only).
    fn sign(&mut self, slot: Slot, sign: Sign, out: &mut Vec<Order>) {
        let pick = self.pick;
        let (side, pos) = (slot.side.min(1), slot.pos.min(1));
        let landed = self.landed[side][pos];
        let Some(s) = self.stand(slot) else { return };
        s.away = None;
        let (chest, head) = (s.chest(), s.head());
        let motion = |id: u16, blend: u32| Order::Motion { slot, id, blend };
        match sign {
            Sign::Struck { strong } => {
                self.struck = Some(slot);
                self.landed[side][pos] = 0;
                if strong {
                    self.start_flash(FLASH_FRAMES, FLASH_ALPHA, WHITE);
                    self.shake = SHAKE_FRAMES;
                }
                // The move's own sprites that just landed were its burst.
                let hit = self.book.as_ref().and_then(|b| b.hit()).cloned();
                if let Some(look) = hit.filter(|_| landed == 0) {
                    // A little off the middle, a different way each time.
                    let off = (pick % 3) as f32 * 0.35 - 0.35;
                    let at = [chest[0] + off, chest[1] + off.abs() * 0.5, chest[2]];
                    self.add_sprite(Sprite {
                        look,
                        path: SIGN_BURST,
                        from: at,
                        to: at,
                        flip: false,
                        on: None,
                        follow: None,
                        volley: 0,
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
                self.status_sprite(slot, status::CONFUSION, SIGN_ORBIT, head, 90);
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
                self.status_sprite(slot, status::PARALYSIS, SIGN_BURST, chest, BURST_FRAMES);
            }
            Sign::Snoring => {
                // Lies down unless it already lies in the sleeping loop.
                if !s.lying {
                    out.push(motion(motion::SLEEP, 15));
                    s.lying = true;
                }
                out.push(Order::Sound(sound::SNORES[pick % sound::SNORES.len()]));
                s.idle = 0;
                self.status_sprite(slot, status::SLEEP, SIGN_RISE, head, 60);
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
                self.status_sprite(slot, status::BURN, SIGN_BURST, chest, BURST_FRAMES);
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
                if let Some((target, f, frames)) = s.step {
                    let [x, z] = reach(s.home, target, f, frames);
                    at[0] += x;
                    at[2] += z;
                    s.step = (f < frames).then_some((target, f + 1, frames));
                }
                let mut lift = 0.0;
                let mut rising = false;
                if let Some((f, frames, height)) = s.lift {
                    lift = lift_at(f, frames, height);
                    rising = f <= frames / 2;
                    s.lift = (f < frames).then_some((f + 1, frames, height));
                }
                if s.away == Some(Hidden::Air) && !rising {
                    // Hovers up there until its next turn.
                    lift = AWAY_LIFT;
                    s.lift = None;
                }
                s.offset = [at[0] - s.home[0], lift, at[2] - s.home[2]];
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
                        Later::Step(target, frames) => s.step = Some((target, 0, frames)),
                    }
                }
                // The rest pose: asleep, or the fighting stance.
                if s.rest_in > 0 {
                    s.rest_in -= 1;
                    // Away, it stays in its getting-ready motion.
                    if s.rest_in == 0 && !s.down && s.away.is_none() {
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
                    self.status_sprite(slot, status::SLEEP, SIGN_RISE, head, 60)
                }
                Some(Ailment::Paralysis) => {
                    if let Some(s) = self.stand(slot) {
                        s.shiver = 16;
                    }
                    self.status_sprite(slot, status::PARALYSIS, SIGN_BURST, chest, BURST_FRAMES);
                }
                _ => {}
            }
            if confused {
                self.status_sprite(slot, status::CONFUSION, SIGN_ORBIT, head, 90);
            }
        }
        // What the sprites do as they land.
        for row in &mut self.landed {
            for left in row {
                *left = left.saturating_sub(1);
            }
        }
        let mut landed = Vec::new();
        self.due.retain_mut(|(wait, _, what)| {
            if *wait <= 1 {
                landed.push(*what);
                false
            } else {
                *wait -= 1;
                true
            }
        });
        for what in landed {
            match what {
                Due::Land {
                    slot,
                    shiver,
                    burst,
                } => {
                    if let Some(left) = self
                        .landed
                        .get_mut(slot.side)
                        .and_then(|r| r.get_mut(slot.pos))
                        .filter(|_| burst)
                    {
                        *left = LANDED_FOR;
                    }
                    if let Some(s) = self.stand(slot).filter(|s| !s.down) {
                        s.shiver = s.shiver.max(shiver);
                    }
                }
                Due::Big => {
                    shakes = shakes.max(SHAKE_FRAMES);
                    self.start_flash(FLASH_FRAMES, BIG_ALPHA, WHITE);
                }
                Due::Tint(colour) => self.start_flash(TINT_FRAMES, TINT_ALPHA, colour),
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
        let draws = self
            .sprites
            .iter()
            .filter_map(|s| {
                // Where the Chao it moves with has gone since.
                let shift = s.follow.and_then(|(slot, then)| {
                    let now = self.stand_ref(slot)?.offset;
                    Some(std::array::from_fn(|i| now[i] - then[i]))
                });
                draw_of(s, shift.unwrap_or([0.0; 3]))
            })
            .collect();
        let flash = if self.flash > 0 {
            self.flash_alpha * f32::from(self.flash) / f32::from(self.flash_len.max(1))
        } else {
            0.0
        };
        Scene {
            draws,
            flash,
            flash_colour: self.flash_colour,
        }
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

    /// What move `id` shows, for the log: the first time this battle only, with the full
    /// show.
    pub fn told(&mut self, id: u16) -> Option<String> {
        if !self.effects || self.told.contains(&id) {
            return None;
        }
        self.told.push(id);
        let look = self.book.as_ref()?.for_move(id);
        let data = self.dex.as_ref().and_then(|d| d.move_data(id));
        let kind = self.kind(id, data.is_some_and(|m| m.power == 0));
        let pose = act(kind, data.as_ref(), self.nth(id), false).pose;
        let sheet = |t: &Option<Template>| t.as_ref().map_or("none".into(), |t| t.tag.to_string());
        Some(format!(
            "move {id} is {kind:?} (motion {:#x} at frame {}) and shows {:?} on the {:?} ({} \
             sprites, sheet {}, burst {}{}{}{})",
            pose.id,
            pose.at,
            look.movement,
            look.place,
            look.count,
            sheet(&look.main),
            sheet(&look.impact),
            if look.borrowed {
                ", its type's look"
            } else {
                ""
            },
            if look.big { ", big hit" } else { "" },
            if look.shake { ", shakes" } else { "" },
        ))
    }

    /// The full show (else the karate motions only).
    pub fn effects(&self) -> bool {
        self.effects
    }

    /// Nothing is in flight or flashing.
    pub fn quiet(&self) -> bool {
        self.sprites.is_empty() && self.flash == 0 && self.shake == 0 && self.due.is_empty()
    }
}

/// Where and how a sprite shows now, moved by `shift` (`None` before it starts).
fn draw_of(s: &Sprite, shift: [f32; 3]) -> Option<Draw> {
    if s.delay > 0 || s.age >= s.life {
        return None;
    }
    let t = f32::from(s.age) / f32::from(s.life.max(1));
    let base = (f32::from(s.look.width.max(s.look.height)) * PIXEL).clamp(MIN_SIZE, MAX_SIZE);
    let (at, size) = match s.path {
        Path::Move(way) => {
            let (at, k) = movement::place(way, s.from, s.to, t);
            (at, base * k)
        }
        Path::Stay => ([s.to[0], s.to[1] + ICE_SIZE / 2.0, s.to[2]], ICE_SIZE),
    };
    let at = std::array::from_fn(|i| at[i] + shift[i]);
    let mut frame = frame_at(&s.look, s.age);
    if s.flip {
        frame.h_flip = !frame.h_flip;
    }
    // How long it shows doesn't change the picture.
    frame.duration = 0;
    Some(Draw {
        key: Key {
            tag: s.look.tag,
            width: s.look.width,
            height: s.look.height,
            frame,
            blended: s.look.blended,
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
        assert_eq!(classify(&status(fx::CONFUSE), &sound), Kind::Inflict);
        assert_eq!(classify(&status(fx::PARALYZE), &sound), Kind::Inflict);
        assert_eq!(classify(&status(fx::RAIN), &sound), Kind::Weather);
        assert_eq!(classify(&status(fx::HAIL), &sound), Kind::Weather);
        assert_eq!(classify(&status(fx::IDENTIFY), &sound), Kind::Charge);
        // Damaging moves by what their effect does, before contact and type.
        let hit = |effect| MoveData {
            effect,
            flags: flags::CONTACT,
            ..mv(8, types::NORMAL, 60)
        };
        assert_eq!(classify(&hit(fx::ONE_HIT_KO), &sound), Kind::OneHit);
        assert_eq!(classify(&hit(fx::TWO_TURN), &sound), Kind::TwoTurn);
        assert_eq!(classify(&hit(fx::HIDE_TWO_TURN), &sound), Kind::TwoTurn);
        assert_eq!(classify(&hit(fx::MULTI_HIT), &sound), Kind::MultiHit);
        assert_eq!(classify(&hit(fx::TWO_HITS), &sound), Kind::MultiHit);
        assert_eq!(classify(&hit(fx::RECOIL_THIRD), &sound), Kind::Recoil);
        assert_eq!(classify(&hit(fx::CRASH_ON_MISS), &sound), Kind::Recoil);
        assert_eq!(classify(&hit(fx::DRAIN_HALF), &sound), Kind::Drain);
        assert_eq!(classify(&hit(fx::FIRST_TURN_FLINCH), &sound), Kind::Quick);
        let first = MoveData {
            priority: 1,
            ..hit(fx::HIT)
        };
        assert_eq!(classify(&first, &sound), Kind::Quick);
        assert_eq!(classify(&hit(fx::BURN_CHANCE), &sound), Kind::Contact);
    }

    /// Every kind, for the tests.
    const KINDS: [Kind; 21] = [
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
        Kind::Weather,
        Kind::Inflict,
        Kind::OneHit,
        Kind::TwoTurn,
        Kind::MultiHit,
        Kind::Recoil,
        Kind::Drain,
        Kind::Quick,
    ];

    #[test]
    fn neighbouring_moves_of_a_kind_play_different_motions() {
        // Next-door moves of a kind differ in every kind with a few motions to pick from, and
        // every motion gets its turn.
        for kind in KINDS {
            let mut seen = std::collections::HashSet::new();
            for id in 0..354 {
                let (a, b) = (act(kind, None, id, false), act(kind, None, id + 1, false));
                seen.insert(a.pose.id);
                if !matches!(kind, Kind::Rest | Kind::Inflict | Kind::TwoTurn) {
                    assert_ne!(a.pose, b.pose, "{kind:?} {id}");
                }
            }
            assert!(
                seen.len() >= 3 || matches!(kind, Kind::Rest | Kind::TwoTurn),
                "{kind:?} {seen:x?}"
            );
        }
    }

    #[test]
    fn every_motion_is_one_the_arena_chao_plays_in_time() {
        for pose in motion::all() {
            // In the table, and none that leave their spot, hold things or lie still, nor
            // whose moment comes too late.
            assert!(pose.id <= 0x26D, "{pose:x?}");
            for (from, to) in [
                (0x156, 0x15A),
                (0x25D, 0x261),
                (0x264, 0x267),
                (0x101, 0x10E),
                (0x3A, 0x3A),
                (0x3C, 0x3C),
            ] {
                assert!(!(from..=to).contains(&pose.id), "{pose:x?}");
            }
            assert!(pose.at < 45, "{pose:x?}");
        }
        // The steps reach furthest at the moment and are home before the stance.
        for kind in KINDS {
            for id in 0..354 {
                let a = act(kind, None, id, false);
                if let Some(frames) = a.step {
                    assert!(a.pose.at.saturating_sub(frames / 2) + frames < a.hold);
                }
            }
        }
    }

    #[test]
    fn a_status_move_moves_by_the_status_it_gives() {
        let status = |effect| MoveData {
            effect,
            ..mv(7, types::NORMAL, 0)
        };
        let pose = |effect| act(Kind::Inflict, Some(&status(effect)), 7, false).pose;
        assert_eq!(pose(fx::SLEEP), motion::YAWN);
        assert_eq!(pose(fx::CONFUSE), motion::SPIN);
        assert_eq!(pose(fx::INFATUATE), motion::SWOON);
        assert_eq!(pose(fx::BAD_POISON), motion::MISCHIEF);
        assert_eq!(pose(fx::PARALYZE), motion::GLARE);
        assert_eq!(pose(fx::BURN), motion::FLAME);
        // A hiding move gets ready its own way, and strikes another.
        let hide = |type_id| MoveData {
            effect: fx::HIDE_TWO_TURN,
            flags: flags::CONTACT,
            ..mv(8, type_id, 80)
        };
        let ready = |m: &MoveData| act(Kind::TwoTurn, Some(m), 0, true);
        assert_eq!(ready(&hide(types::FLYING)).pose, motion::RISE_UP);
        assert!(ready(&hide(types::FLYING)).lift.is_some());
        assert_eq!(ready(&hide(types::GROUND)).pose, motion::DIG);
        assert_eq!(ready(&hide(types::WATER)).pose, motion::DIVE);
        let strike = act(Kind::TwoTurn, Some(&hide(types::FLYING)), 0, false);
        assert_eq!(strike.pose, motion::SWOOP);
        assert!(strike.step.is_some());
    }

    #[test]
    fn every_catalog_move_has_a_kind_and_the_kinds_spread_out() {
        let Some(rom) = crate::test_cache::rom() else {
            return;
        };
        let dex = Dex::from_rom(&rom).expect("dex");
        let places = places(&dex);
        let mut counts = std::collections::HashMap::new();
        let mut poses = std::collections::HashMap::<Kind, std::collections::HashSet<u16>>::new();
        for m in dex.table() {
            let kind = classify(&m, dex.sound_moves());
            *counts.entry(kind).or_insert(0) += 1;
            let pose = act(kind, Some(&m), places[&m.id], false).pose;
            poses.entry(kind).or_default().insert(pose.id);
        }
        println!("{counts:?}");
        // Every kind is some move's.
        for kind in KINDS {
            assert!(counts.get(&kind).is_some_and(|&n| n > 0), "{kind:?}");
        }
        assert!(counts[&Kind::Contact] > 40, "{counts:?}");
        assert!(counts[&Kind::Rest] < 5, "{counts:?}");
        // The big kinds play all their motions.
        assert_eq!(poses[&Kind::Contact].len(), motion::CONTACT.len());
        assert_eq!(poses[&Kind::Special].len(), motion::SPECIAL.len() + 1);
        // Four common plain contact moves (ids only) each look different.
        let shown: std::collections::HashSet<u16> = [1, 10, 33, 64]
            .iter()
            .map(|&id| {
                let m = dex.move_data(id).expect("move");
                act(
                    classify(&m, dex.sound_moves()),
                    Some(&m),
                    places[&id],
                    false,
                )
                .pose
                .id
            })
            .collect();
        assert_eq!(shown.len(), 4);
    }

    #[test]
    fn every_catalog_moves_sprites_land_before_its_hit() {
        let Some(rom) = crate::test_cache::rom() else {
            return;
        };
        let dex = Arc::new(Dex::from_rom(&rom).expect("dex"));
        let book = Arc::new(Book::from_rom(&rom));
        for m in dex.table().into_iter().filter(|m| m.power > 0) {
            let mut d = Director::new(true, homes(), Some(dex.clone()), Some(book.clone()));
            let (a, b) = show(&mut d);
            d.cue(&Cue::Attack {
                by: a,
                toward: Some(b),
                status: false,
                move_id: m.id,
                charging: false,
            });
            for &(at, _, what) in &d.due {
                if matches!(what, Due::Land { .. }) {
                    assert!(at <= LAND_BY, "move {} lands at {at}", m.id);
                }
            }
        }
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
            blended: false,
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
        let mut looks = vec![MoveLook::default(); 6];
        looks[2] = MoveLook {
            impact: Some(look(10_002)),
            ..MoveLook::of(Movement::Straight, look(10_001))
        };
        looks[4] = looks[2].clone();
        looks[5] = MoveLook {
            shake: true,
            big: true,
            flash: Some(0x7C00),
            count: 5,
            ..MoveLook::of(Movement::Rain, look(10_003))
        };
        let book = Book::for_tests(looks, Some(look(10_135)), Some(look(10_200)));
        Director::new(effects, homes(), Some(Arc::new(dex)), Some(Arc::new(book)))
    }

    /// `d` with these moves' records instead.
    fn set_moves(d: &mut Director, moves: &[MoveData]) {
        let dex = Dex::for_tests(moves, TypeChart::default());
        d.places = places(&dex);
        d.dex = Some(Arc::new(dex));
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
            charging: false,
        });
        assert_eq!(
            motions(&orders),
            [act(Kind::Contact, None, d.nth(1), false).pose.id]
        );
        // Out toward the foe and back.
        let mut furthest: f32 = -5.0;
        for _ in 0..STANCE_AFTER {
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
            charging: false,
        });
        assert_eq!(motions(&orders), [motion::FLAME.id]);
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
            charging: false,
        });
        let mut seen = 0;
        for _ in 0..60 {
            d.frame();
            for draw in d.scene().draws {
                seen += 1;
                assert!((draw.at[0] + 5.0).abs() < 1.2, "{draw:?}");
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
            charging: false,
        });
        // A special move is a karate attack, with no sprites.
        assert!(motion::ATTACKS.contains(&motions(&orders)[0]));
        let orders = d.cue(&Cue::Attack {
            by: b,
            toward: Some(a),
            status: true,
            move_id: 3,
            charging: false,
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
    fn a_two_turn_move_gets_ready_without_sprites_then_strikes_with_them() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let moves = [MoveData {
            effect: fx::TWO_TURN,
            ..mv(2, types::GRASS, 120)
        }];
        set_moves(&mut d, &moves);
        let attack = |d: &mut Director, charging| {
            let orders = d.cue(&Cue::Attack {
                by: a,
                toward: Some(b),
                status: false,
                move_id: 2,
                charging,
            });
            let mut drawn = 0;
            for _ in 0..60 {
                d.frame();
                drawn += d.scene().draws.len();
            }
            (motions(&orders), drawn)
        };
        let (ready, drawn) = attack(&mut d, true);
        assert!(
            motion::READY.iter().any(|p| [p.id] == ready[..]),
            "{ready:x?}"
        );
        assert_eq!(drawn, 0);
        let (strike, drawn) = attack(&mut d, false);
        assert_ne!(strike, ready);
        assert!(drawn > 0);
    }

    #[test]
    fn a_multi_hit_moves_flurry_goes_on_through_its_hits() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let moves = [MoveData {
            effect: fx::MULTI_HIT,
            flags: flags::CONTACT,
            ..mv(1, types::NORMAL, 15)
        }];
        set_moves(&mut d, &moves);
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 1,
            charging: false,
        });
        let stance = |out: &Out| {
            out.orders
                .iter()
                .any(|o| matches!(*o, Order::Motion { slot, id: motion::STANCE, .. } if slot == a))
        };
        // Three hits, a quick beat apart: no stance between them, a step in for each.
        for _ in 0..3 {
            let mut furthest: f32 = -5.0;
            for _ in 0..30 {
                let out = d.frame();
                assert!(!stance(&out));
                furthest = furthest.max(out.places[0].1[0]);
            }
            assert!(furthest > -4.0, "{furthest}");
            d.cue(&Cue::Sign {
                slot: b,
                sign: Sign::Struck { strong: false },
            });
            d.cue(&Cue::Hurt {
                target: b,
                heavy: 0,
            });
        }
        let mut back = false;
        for _ in 0..FLURRY_AFTER {
            back |= stance(&d.frame());
        }
        assert!(back);
        // A hurt that isn't one of its hits (poison, weather) doesn't send it in again.
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 1,
            charging: false,
        });
        for _ in 0..40 {
            d.frame();
        }
        d.cue(&Cue::Hurt {
            target: b,
            heavy: 0,
        });
        let mut furthest: f32 = -5.0;
        for _ in 0..30 {
            furthest = furthest.max(d.frame().places[0].1[0]);
        }
        assert!(furthest < -4.9, "{furthest}");
    }

    #[test]
    fn a_hop_lifts_and_lands_with_a_shake() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        // Picks the hop.
        let spread_id = 5;
        let mut dex_moves = vec![MoveData {
            target: Target::BothFoes,
            ..mv(spread_id, types::GROUND, 100)
        }];
        dex_moves.push(mv(1, types::NORMAL, 40));
        set_moves(&mut d, &dex_moves);
        let orders = d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: spread_id,
            charging: false,
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
    fn a_rain_falls_on_the_target_and_shakes_and_tints_as_it_lands() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let mut dex_moves = vec![mv(5, types::ROCK, 75)];
        dex_moves.push(mv(1, types::NORMAL, 40));
        set_moves(&mut d, &dex_moves);
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 5,
            charging: false,
        });
        let (mut highest, mut drops, mut shivered, mut flash, mut shook) =
            (0f32, 0, false, 0u32, false);
        for _ in 0..movement::MAX_FRAMES + 20 {
            let out = d.frame();
            for draw in d.scene().draws.iter().filter(|x| x.key.tag == 10_003) {
                drops += 1;
                highest = highest.max(draw.at[1]);
                assert!((draw.at[0] - 5.0).abs() <= 1.0, "{draw:?}");
            }
            shivered |= out.places.iter().any(|(s, at, _)| *s == b && at[0] != 5.0);
            if d.scene().flash > 0.0 {
                flash = d.scene().flash_colour;
            }
            shook |= d.shake() != [0.0; 3];
        }
        assert!(drops > 20, "{drops}");
        assert!(highest > 8.0 + CHEST + 3.0, "{highest}");
        // The hit that follows is its reaction: the sprites landing don't shudder it too.
        assert!(!shivered && shook);
        // A big hit's white flash wins over the blue tint landing with it.
        assert_eq!(flash, WHITE);
        assert!(d.quiet());
        assert_eq!(tint_colour(0x7C00), Some(0x0000FF));
        assert_eq!(tint_colour(0x001F), Some(0xFF0000));
        assert_eq!(tint_colour(0x0421), None);
    }

    #[test]
    fn a_move_on_both_foes_lands_on_each_in_doubles() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let c = Slot::new(1, 1);
        d.cue(&Cue::Show { slot: c, member: 1 });
        let spread = MoveData {
            target: Target::BothFoes,
            ..mv(2, types::FIRE, 90)
        };
        d.dex = Some(Arc::new(Dex::for_tests(&[spread], TypeChart::default())));
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 2,
            charging: false,
        });
        let mut ends = std::collections::HashSet::new();
        for _ in 0..movement::MAX_FRAMES {
            d.frame();
            for draw in d.scene().draws.iter().filter(|x| x.key.tag == 10_002) {
                ends.insert(draw.at[2] as i32);
            }
        }
        // Bursts on the foe at the front (z 19) and the one behind (z 22).
        assert!(ends.contains(&19) && ends.contains(&22), "{ends:?}");
    }

    #[test]
    fn a_new_move_drops_the_last_ones_sprites_still_waiting() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 2,
            charging: false,
        });
        d.frame();
        assert!(!d.sprites.is_empty());
        assert!(d.sprites.iter().all(|s| s.delay > 0));
        // The foe's move comes before any of them started (its beat was skipped).
        d.cue(&Cue::Attack {
            by: b,
            toward: Some(a),
            status: false,
            move_id: 1,
            charging: false,
        });
        assert!(d.sprites.is_empty());
        // Nor does what they were to do as they landed.
        assert!(d.due.is_empty(), "{:?}", d.due);
    }

    #[test]
    fn a_moves_look_is_told_once_a_battle_with_the_full_show() {
        let mut d = director(true);
        let told = d.told(2).expect("told");
        assert!(
            told.contains("Straight") && told.contains("10001"),
            "{told}"
        );
        assert_eq!(d.told(2), None);
        assert_eq!(director(false).told(2), None);
    }

    #[test]
    fn sprites_are_drawn_big_enough_to_see() {
        let mut small = look(1);
        small.width = 8;
        small.height = 8;
        let sprite = |look: Template| Sprite {
            look,
            path: SIGN_BURST,
            from: [0.0; 3],
            to: [0.0; 3],
            flip: false,
            on: None,
            follow: None,
            volley: 0,
            delay: 0,
            age: 0,
            life: 10,
        };
        let size = draw_of(&sprite(small), [0.0; 3]).unwrap().size;
        assert!(size >= MIN_SIZE * 0.8 - 1e-3, "{size}");
        let mut huge = look(1);
        huge.width = 64;
        huge.height = 64;
        assert!(draw_of(&sprite(huge), [0.0; 3]).unwrap().size <= MAX_SIZE * 1.3);
        let _ = Place::User;
    }

    #[test]
    fn a_damaging_moves_sprites_land_late_in_its_beat_and_stand_for_its_burst() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 2,
            charging: false,
        });
        let land = d
            .due
            .iter()
            .find_map(|&(at, _, what)| matches!(what, Due::Land { .. }).then_some(at))
            .expect("lands");
        assert!((LAND_FROM..=LAND_BY).contains(&land), "{land}");
        for _ in 0..ATTACK {
            d.frame();
        }
        // The hit comes as the beat ends: no second burst of the plain hit.
        d.cue(&Cue::Sign {
            slot: b,
            sign: Sign::Struck { strong: false },
        });
        d.frame();
        assert!(d.scene().draws.iter().all(|x| x.key.tag != 10_135));
        // A later hit has its own.
        d.cue(&Cue::Sign {
            slot: b,
            sign: Sign::Struck { strong: false },
        });
        d.frame();
        assert!(d.scene().draws.iter().any(|x| x.key.tag == 10_135));
    }

    #[test]
    fn a_move_on_everyone_else_lands_on_the_users_partner_too() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let (ally, c) = (Slot::new(0, 1), Slot::new(1, 1));
        d.cue(&Cue::Show {
            slot: ally,
            member: 1,
        });
        d.cue(&Cue::Show { slot: c, member: 1 });
        let all = MoveData {
            target: Target::AllOthers,
            ..mv(2, types::FIRE, 90)
        };
        d.dex = Some(Arc::new(Dex::for_tests(&[all], TypeChart::default())));
        // From the back: aimed first at its own partner.
        d.cue(&Cue::Attack {
            by: ally,
            toward: Some(a),
            status: false,
            move_id: 2,
            charging: false,
        });
        let mut on: Vec<Slot> = d
            .due
            .iter()
            .filter_map(|&(_, _, what)| match what {
                Due::Land { slot, .. } => Some(slot),
                _ => None,
            })
            .collect();
        on.sort_by_key(|s| (s.side, s.pos));
        assert_eq!(on, [a, b, c]);
    }

    #[test]
    fn sprites_on_the_user_move_with_it_as_it_steps() {
        let moves = [MoveData {
            flags: flags::CONTACT,
            ..mv(1, types::NORMAL, 40)
        }];
        let dex = Dex::for_tests(&moves, TypeChart::default());
        let mut looks = vec![MoveLook::default(); 2];
        looks[1] = MoveLook {
            place: Place::User,
            count: 2,
            ..MoveLook::of(Movement::Orbit, look(10_001))
        };
        let book = Book::for_tests(looks, None, None);
        let mut d = Director::new(true, homes(), Some(Arc::new(dex)), Some(Arc::new(book)));
        let (a, b) = show(&mut d);
        d.cue(&Cue::Attack {
            by: a,
            toward: Some(b),
            status: false,
            move_id: 1,
            charging: false,
        });
        let mut seen = 0;
        for _ in 0..80 {
            let out = d.frame();
            let chao = out.places.iter().find(|(s, _, _)| *s == a).unwrap().1;
            for draw in d.scene().draws {
                seen += 1;
                assert!((draw.at[0] - chao[0]).abs() < 1.7, "{draw:?} {chao:?}");
            }
        }
        assert!(seen > 0);
    }

    #[test]
    fn a_chao_up_in_the_air_hovers_until_its_strike() {
        let mut d = director(true);
        let (a, b) = show(&mut d);
        let moves = [MoveData {
            effect: fx::HIDE_TWO_TURN,
            flags: flags::CONTACT,
            ..mv(2, types::FLYING, 90)
        }];
        set_moves(&mut d, &moves);
        let attack = |d: &mut Director, charging| {
            d.cue(&Cue::Attack {
                by: a,
                toward: Some(b),
                status: false,
                move_id: 2,
                charging,
            })
        };
        attack(&mut d, true);
        let mut seen = Vec::new();
        let mut lift = 0.0;
        for _ in 0..200 {
            let out = d.frame();
            seen.extend(out.orders.iter().filter_map(|o| match *o {
                Order::Motion { slot, id, .. } if slot == a => Some(id),
                _ => None,
            }));
            lift = out.places[0].2;
        }
        // Still up there, in its hover, through the foe's turn.
        assert_eq!(lift, AWAY_LIFT);
        assert!(seen.is_empty(), "{seen:x?}");
        attack(&mut d, false);
        let mut lifts = Vec::new();
        for _ in 0..30 {
            lifts.push(d.frame().places[0].2);
        }
        assert!(lifts[0] > 1.5 && *lifts.last().unwrap() == 0.0, "{lifts:?}");
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

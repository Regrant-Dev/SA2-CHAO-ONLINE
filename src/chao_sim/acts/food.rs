//! Fruit: begging for the one a player holds, going for the nearest, taking one offered,
//! and eating (re/chao-ai.md §2A.4, §6.3; re/chao-garden-items.md §1), decoded from the
//! game's bytes. While a Chao holds a fruit the eat behaviour also runs the fruit's side of
//! the meal (`0x545C20`: its bite timer, the bites' effects on the Chao, mailbox 4 when it's
//! finished), as if the fruit updated right after the Chao; the server moves the fruit and
//! hears about it through `Chao::fruit_events`.

use super::super::brain::{Behaviour, Chao, FruitEvent, Hands, Link, Next, Rng};
use super::super::land::Garden;
use super::super::picker::{Fruit, Senses};
use super::super::record::{Record, TYPE_CHILD, off};
use super::reach::{self, turn_to};

/// The behaviours here, by game address.
pub(super) const BEG: u32 = 0x59E8C0;
pub(super) const BEG_WAIT: u32 = 0x59EA00;
pub(super) const GO_EAT: u32 = 0x5614D0;
pub(super) const PICK_UP: u32 = 0x5613C0;
pub(super) const EAT: u32 = 0x5607C0;
pub(super) const TAKE_OFFERED: u32 = 0x5606D0;

/// The fruit table's stat columns (`0x8A6448 + kind·20 + 4`: swim, fly, run, power,
/// stamina, luck, intelligence, unused), read from the exe.
const FRUIT_STATS: [[i16; 8]; 24] = [
    [0, 0, 0, 0, 40, 0, 0, 0],
    [0, 0, 0, 0, 99, 0, 0, 0],
    [0, 0, 0, 0, 40, 0, 0, 0],
    [0, 0, 0, 0, 50, 0, 0, 0],
    [0, 0, 0, 0, 30, 0, 0, 0],
    [0, 0, 0, 0, 40, 0, 0, 0],
    [0, 0, 0, 0, 40, 0, 0, 0],
    [0, 0, 0, 0, 40, 0, 0, 0],
    [0, 0, 0, 0, 40, 0, 0, 0],
    [0, 0, 0, 0, 40, 0, 0, 0],
    [0, 0, 0, 0, 40, 0, 0, 0],
    [50, 50, 50, 50, 50, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 50, 0],
    [30, -20, -20, 30, 10, 0, 0, 0],
    [20, 50, -10, -10, 30, 0, 0, 0],
    [40, -30, 40, -30, 20, 0, 0, 0],
    [0, -10, 30, 40, 20, 0, 0, 0],
    [-20, 30, 30, -20, 10, 0, 0, 0],
    [-30, 40, -30, 40, 20, 0, 0, 0],
    [30, 10, 30, 20, -50, 0, 0, 0],
    [0, 0, 0, 0, 50, 50, 0, 0],
    [0, 0, 0, 0, 80, 100, 0, 0],
    [0, 0, 0, 0, 40, 40, 0, 0],
    [0, 0, 0, 0, 40, 40, 0, 0],
];

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    let _ = garden;
    Some(match address {
        BEG => beg(chao, senses, rng),
        BEG_WAIT => beg_wait(chao, rng),
        GO_EAT => go_eat(chao, senses),
        PICK_UP => pick_up(chao, senses),
        EAT => eat(chao, senses, rng),
        TAKE_OFFERED => take_offered(chao, senses),
        _ => return None,
    })
}

/// Eyes 4 and, unless the mouth is 1, mouth 3 or 2 (50/50), as the begs start.
fn begging_face(chao: &mut Chao, rng: &mut Rng) {
    chao.set_eyes(4, -1);
    if chao.mouth != 1 {
        let mouth = if rng.r() < 0.5 { 3 } else { 2 };
        chao.set_mouth(mouth, -1);
    }
}

/// `0x59E8C0`: beg for the fruit a player holds. Motion 0x76 (blend 10); walks to player 1
/// (turn 0x120, drive 0.055 on the ground); done within 10 (squared 3-D < 100) or after
/// 1001 frames. With no player position known it is done at once (the game always has one).
fn beg(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play_blend(0x76, 10);
        begging_face(chao, rng);
        chao.mode = 1;
        chao.count_a = 0;
        chao.count_b = 0;
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old > 1000 {
        return Next::Pop;
    }
    let Some(player) = senses.player_pos() else {
        return Next::Pop;
    };
    let p = chao.body.pos;
    let (dx, dy, dz) = (player[0] - p[0], player[1] - p[1], player[2] - p[2]);
    if dy * dy + dx * dx + dz * dz < 100.0 {
        return Next::Pop;
    }
    turn_to(chao, player, 0x120);
    if chao.body.on_ground() {
        let (s, c) = chao.body.facing();
        chao.body.accel[0] = s * 0.055;
        chao.body.accel[2] = c * 0.055;
    }
    Next::Keep
}

/// `0x59EA00`: after begging, motion 0x18E (blend 25) with the begging face for 181..301
/// frames.
fn beg_wait(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0x18E, 25);
            begging_face(chao, rng);
            chao.mode = 1;
            chao.count_a = 180 + rng.below(121);
        }
        1 => {}
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a -= 1;
    if old > 0 { Next::Keep } else { Next::Pop }
}

/// `0x5614D0`: go and eat the nearest fruit: link it, then stand up, notice, walk to it,
/// pick it up, eat. Done at once with no fruit; while another Chao holds it, it waits (the
/// game returns a pointer, which the tick ignores).
fn go_eat(chao: &mut Chao, senses: &dyn Senses) -> Next {
    let Some(fruit) = senses.nearest_fruit() else {
        return Next::Pop;
    };
    if fruit.taken {
        return Next::Keep;
    }
    chao.link = Some(Link::Fruit(fruit.id));
    chao.set(Behaviour::StandUp, -1);
    for b in [reach::NOTICE, reach::WALK_TO_LINK, PICK_UP, EAT] {
        chao.push(Behaviour::Game(b));
    }
    Next::Keep
}

/// `0x5613C0`: pick up the linked fruit. Motion 0xBC, sound 0x1001; at the motion's end it
/// takes the fruit unless another Chao holds it (`0x56CFB0`), and is done either way. After
/// 1800 frames it resets (returns 3).
fn pick_up(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match chao.mode {
        0 => {
            chao.play(0xBC);
            chao.sound(0x1001);
            chao.mode = 1;
            chao.patience = 1800;
        }
        1 | 2 => {
            if chao.mode == 1 && chao.motion_done() {
                // No link: the game reads an uninitialised stack slot; nothing is taken.
                let linked = match chao.link {
                    Some(Link::Fruit(id)) => senses.fruit(id),
                    _ => None,
                };
                if let Some(fruit) = linked.filter(|f| !f.taken) {
                    grab(chao, fruit, false, senses);
                    chao.mode = 2;
                    chao.count_a = 0;
                    chao.count_b = 0;
                }
            }
            if chao.motion_done() {
                return Next::Pop;
            }
        }
        _ => {}
    }
    chao.patience -= 1;
    if chao.patience <= 0 {
        chao.set(Behaviour::Settle, -1);
    }
    Next::Keep
}

/// `0x5606D0`: take a fruit it touched (the fruit interrupt `0x560640` set `W+0x54` to it).
/// Motion 0xCA (blend 15); turns 0x600 a frame; facing it (< 0xB6 left) and touching a fruit
/// no Chao holds: player 1 drops what it holds (`0x46E5E0`), it takes the fruit and eats
/// (`AL_SetBehavior(0x5607C0)`; the game also opens the fruit's balloon, `0x5659E0`). Facing
/// and touching none: on the 61st such frame, sound 0x6037, done.
fn take_offered(chao: &mut Chao, senses: &dyn Senses) -> Next {
    if chao.mode == 0 {
        chao.play_blend(0xCA, 15);
        chao.mode = 1;
        chao.count_a = 60;
    }
    if chao.body.turn_to_target(0x600) >= 0xB6 {
        return Next::Keep;
    }
    match senses.touching_fruit() {
        Some(fruit) if fruit.taken => Next::Keep,
        Some(fruit) => {
            grab(chao, fruit, true, senses);
            chao.set(Behaviour::Game(EAT), -1);
            Next::Keep
        }
        None => {
            let old = chao.count_a;
            chao.count_a -= 1;
            if old > 0 {
                return Next::Keep;
            }
            chao.sound(0x6037);
            Next::Pop
        }
    }
}

/// `0x56CFB0`: take `fruit` into its hands (a mutual link; the fruit starts its eaten mode
/// with its bite timer at 0). Taken whole out of a player's hand (the fruit's `0x545AF0`
/// when the player lets go): AddHappiness +1 (`0x1312D08`) by 0.025 for a child, 0.01 for
/// an adult (`0x1312D0C`, `0x1312D10`).
fn grab(chao: &mut Chao, fruit: Fruit, player_drops: bool, senses: &dyn Senses) {
    chao.hands = Some(Hands {
        fruit: fruit.id,
        kind: fruit.kind,
        bites: fruit.bites,
        chew: 0,
    });
    chao.fruit_events.push(FruitEvent::Took {
        id: fruit.id,
        player_drops,
    });
    if player_drops && fruit.held && fruit.bites == 4 {
        let align = if chao.rec.u8(off::TYPE) == TYPE_CHILD {
            0.025
        } else {
            0.01
        };
        add_happiness(chao, 1, align, senses.player_character());
    }
}

/// `0x530690` on the hands: let go of the fruit (also what every interrupt that fires does,
/// `0x53DC1E`). An eaten-up fruit is gone already and isn't reported.
pub(super) fn let_go(chao: &mut Chao) {
    if let Some(h) = chao.hands.take()
        && h.bites > 0
    {
        chao.fruit_events.push(FruitEvent::Dropped {
            id: h.fruit,
            vel: None,
        });
    }
}

/// Dark adult (`0x535390`): types 7, 10, ..., 22.
fn dark_adult(rec: &Record) -> bool {
    let t = i32::from(rec.u8(off::TYPE)) - 7;
    (0..=15).contains(&t) && t % 3 == 0
}

/// Hero adult (`0x535360`): types 6, 9, ..., 21.
fn hero_adult(rec: &Record) -> bool {
    let t = i32::from(rec.u8(off::TYPE)) - 6;
    (0..=16).contains(&t) && t % 3 == 0
}

/// The taste test (`0x5608F3`, table `0x5613AC`): hero fruit (5) for dark adults, dark fruit
/// (6) for hero adults, and kinds 7/8/9 by the taste byte `rec+0x157`.
fn dislikes(rec: &Record, kind: u8) -> bool {
    let taste = rec.u8(0x157);
    match kind {
        5 => dark_adult(rec),
        6 => hero_adult(rec),
        7 => matches!(taste, 2 | 4),
        8 => matches!(taste, 0 | 5),
        9 => matches!(taste, 1 | 3),
        _ => false,
    }
}

/// The current motion (`D+0xB0`).
fn current_motion(chao: &Chao) -> i32 {
    chao.player.current
}

/// `0x5607C0`: eat the fruit in its hands (14 modes, table `0x561374`). Ends once it holds
/// nothing, so at once with no fruit. `memo[0]` is the eating style `D+0x41C` (which
/// `AL_SetBehavior` zeroed on the way in).
fn eat(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    let character = senses.player_character();
    let mut bite = false;
    if chao.mode == 0 {
        chao.play(0xA9);
        chao.mode = 1;
        chao.count_b = 100 + rng.below(300);
        chao.memo[0] = 0.0;
    }
    match chao.mode {
        1 => first_taste(chao, character, rng),
        2 => {
            if chao.count_b > 0 {
                chao.count_b -= 1;
            } else if chao.motion_done() {
                chao.count_b = 60;
                chao.play(0xB6);
                chao.set_eyes(4, -1);
                chao.mode = 3;
                chao.sound(0x6039);
            }
        }
        3 => {
            if chao.count_b > 0 {
                chao.count_b -= 1;
            } else if chao.motion_done() {
                chao.mode = 4;
            }
        }
        4 | 6 => {
            if chao.mode == 4 {
                start_chewing(chao);
            }
            bite = chew(chao, rng);
        }
        5 => {
            if chao.motion_done() {
                chao.count_b -= 1;
                if chao.count_b <= 0 {
                    chao.play(0xAD);
                    chao.mode = 8;
                }
            }
        }
        7 => {
            if chao.motion_done() {
                chao.count_b -= 1;
                if chao.count_b <= 0 {
                    if rng.r() < 0.65 {
                        chao.play(0xAD);
                        chao.mode = 8;
                    } else {
                        match current_motion(chao) {
                            0xB0 => chao.play_blend(0xB1, 25),
                            0xB1 => chao.play_blend(0xB0, 25),
                            _ => {}
                        }
                        chao.count_b = pause(rng);
                    }
                }
            }
        }
        8 => {
            if chao.motion_done() {
                chao.mode = 4;
                chao.count_b = 180 + rng.below(400);
            }
        }
        9 | 10 => {
            if chao.mode == 9 {
                chao.play(0xBA);
                chao.mode = 10;
            }
            throw_away(chao);
        }
        // 11..13, after the meal: unreachable, as mode 11 is only set right after letting
        // go, so the tail ends the behaviour first.
        11 => {
            let old = chao.count_b;
            chao.count_b -= 1;
            if old <= 0 {
                if rng.r() < 0.6 {
                    chao.play(0xA6);
                    chao.mode = 12;
                    chao.count_a = 0;
                    chao.count_b = 0;
                    chao.set_eyes(4, -1);
                    chao.set_mouth(2, -1);
                } else if rng.r() < 0.2 {
                    chao.play(0x52);
                    chao.mode = 13;
                } else {
                    return Next::Pop;
                }
            }
        }
        12 => {
            if chao.motion_done() {
                chao.play(0xA5);
                chao.mode = 11;
                chao.count_b = 60 + rng.below(100);
                chao.set_eyes(0, -1);
                chao.set_mouth(3, -1);
            }
        }
        13 if chao.motion_held() => return Next::Pop,
        _ => {}
    }
    let next = eat_tail(chao, character);
    fruit_frame(chao, senses, bite, rng);
    next
}

/// Eat mode 1, at the end of 0xA9: straight to chewing (a picky, hungry-enough Chao: p150 < 0,
/// p14A < 0, HUNGER ≤ 5000; or r ≤ 0.5), else motion 0xB4 for 120 frames first (mode 2).
/// Then the taste test: a fruit it dislikes → mode 9, AddHappiness −1 by −0.1
/// (`0x1312D14`), sound 0x603A.
fn first_taste(chao: &mut Chao, character: Option<u8>, rng: &mut Rng) {
    if !chao.motion_done() {
        return;
    }
    let held = chao.hands;
    let rec = &chao.rec;
    if rec.p(0x150) < 0 && rec.p(off::PERSONALITY) < 0 && rec.u16(off::HUNGER) <= 5000 {
        chao.mode = 4;
    } else if rng.r() > 0.5 {
        chao.play(0xB4);
        chao.mode = 2;
        chao.count_b = 120;
    } else {
        chao.mode = 4;
    }
    if let Some(h) = held
        && dislikes(&chao.rec, h.kind)
    {
        chao.mode = 9;
        add_happiness(chao, -1, -0.1, character);
        chao.sound(0x603A);
    }
}

/// Eat mode 4: a bite's motion by style `D+0x41C`, chosen once: 1 (p150 > 50 or HUNGER >
/// 8000) motion 0xB3, sound 0x603C; 2 motion 0xAF, sound 0x603B. Then eyes 4, mode 6.
fn start_chewing(chao: &mut Chao) {
    let style = chao.memo[0] as i32;
    let greedy =
        style == 1 || style == 0 && (chao.rec.p(0x150) > 50 || chao.rec.u16(off::HUNGER) > 8000);
    match style {
        0..=2 if greedy => {
            chao.play(0xB3);
            chao.memo[0] = 1.0;
            chao.sound(0x603C);
        }
        0..=2 => {
            chao.play(0xAF);
            chao.memo[0] = 2.0;
            chao.sound(0x603B);
        }
        _ => {}
    }
    chao.mode = 6;
    chao.set_eyes(4, -1);
}

/// A pause between bites: 10 frames' worth of motion ends (r < 0.3), else 40..239.
fn pause(rng: &mut Rng) -> i32 {
    if rng.r() < 0.3 {
        10
    } else {
        40 + rng.below(200)
    }
}

/// Eat mode 6, the bite loop (`0x560BC0`): ask the fruit for a bite (mailbox 2, returned as
/// true while it holds one). Mailbox 4 (finished): motion 0xA5 (blend 60), 180..379 frames,
/// mode 11, let go, eyes 4, mouth 2, sound 0x6042. Otherwise the chewing budget runs down;
/// spent: gluttons (p150 > 50) 40% motion 0xB7 for 2..6 ends, sound 0x603D, eyes 1 (mode
/// 5), else 0xB1/0xB0 for a pause, eyes 0 (mode 7). Then, once a need period (`0x536420`),
/// HUNGER and TIRE −2400 (`0x1312DA6`); HUNGER 0 → mode 9.
fn chew(chao: &mut Chao, rng: &mut Rng) -> bool {
    let asked = chao.hands.is_some();
    let mail = std::mem::take(&mut chao.mail);
    if mail == 4 {
        chao.count_b = 180 + rng.below(200);
        chao.play_blend(0xA5, 60);
        chao.mode = 11;
        let_go(chao);
        chao.set_eyes(4, -1);
        chao.set_mouth(2, -1);
        chao.sound(0x6042);
        return asked;
    }
    chao.count_b -= 1;
    if chao.count_b <= 0 {
        if chao.rec.p(0x150) > 50 && rng.r() < 0.4 {
            chao.play(0xB7);
            chao.mode = 5;
            chao.count_b = 2 + rng.below(5);
            chao.sound(0x603D);
            chao.set_eyes(1, -1);
        } else {
            let id = if rng.r() < 0.5 { 0xB1 } else { 0xB0 };
            chao.play(id);
            chao.count_b = pause(rng);
            chao.set_eyes(0, -1);
            chao.mode = 7;
        }
    }
    if chao.rec.need_latch() {
        chao.rec.add_need(off::HUNGER, -2400);
        chao.rec.add_need(off::TIRE, -2400);
        if chao.rec.u16(off::HUNGER) == 0 {
            chao.mode = 9;
        }
    }
    asked
}

/// Eat modes 9/10: motion 0xBA; once it plays past frame 4, throw the fruit forward-left
/// (`(sin, 0.7, cos)(rot + 0x71C7)·(0.3, 1, 0.3)` into the fruit's velocity), close the
/// balloon (`0x565B90`, the client's) and let go; mode 13.
fn throw_away(chao: &mut Chao) {
    if current_motion(chao) != 0xBA || chao.player.frame <= 4.0 {
        return;
    }
    chao.mode = 13;
    if let Some(h) = chao.hands.take() {
        let a = f32::from(chao.body.rot_y.wrapping_add(0x71C7)) * std::f32::consts::TAU / 65536.0;
        let k = f64::from(0.3f32);
        let vel = [
            (f64::from(a.sin()) * k) as f32,
            0.7,
            (f64::from(a.cos()) * k) as f32,
        ];
        chao.fruit_events.push(FruitEvent::Dropped {
            id: h.fruit,
            vel: Some(vel),
        });
    }
}

/// The tail every eat frame runs (`0x561197`): still holding the fruit → go on. Otherwise
/// read the mailbox: 6 (a player took the fruit away) → AddHappiness −1 by −0.1
/// (`0x1312D18`), anger `0x12D` and `0x12E` +200 (the balloon is the client's); done.
fn eat_tail(chao: &mut Chao, character: Option<u8>) -> Next {
    if chao.hands.is_some() {
        return Next::Keep;
    }
    if std::mem::take(&mut chao.mail) == 6 {
        add_happiness(chao, -1, -0.1, character);
        chao.rec.add_emotion(0x12D, 200);
        chao.rec.add_emotion(0x12E, 200);
    }
    Next::Pop
}

/// The fruit's frame in a Chao's hands (`0x545C20`), after the Chao's: finished last frame →
/// gone. Asked for a bite (`asked`): its timer `D+6` counts, and past 180 a bite
/// (`0x545790`; the fourth and last also: hero fruit alignment +0.15, dark −0.15, heart fruit
/// BREED +10000) and, when none are left, mailbox 4. Grabbed by a player → mailbox 6 and
/// let go (no event: the server moved it).
fn fruit_frame(chao: &mut Chao, senses: &dyn Senses, asked: bool, rng: &mut Rng) {
    let Some(mut h) = chao.hands else {
        return;
    };
    let Some(fruit) = senses.fruit(h.fruit).filter(|_| h.bites > 0) else {
        chao.hands = None;
        return;
    };
    if fruit.taken {
        // A crossed pickup can move the fruit to another Chao. The losing Chao must stop
        // eating it instead of gaining stats from an item visible in someone else's hands.
        chao.hands = None;
        return;
    }
    if asked {
        let old = h.chew;
        h.chew += 1;
        if old > 180 {
            h.bites -= 1;
            h.chew = 0;
            fruit_bite(&mut chao.rec, h.kind, h.bites == 0, rng);
            chao.fruit_events.push(FruitEvent::Bite {
                id: h.fruit,
                left: h.bites,
            });
            if h.bites == 0 {
                match h.kind {
                    5 => add_alignment(&mut chao.rec, 0.15),
                    6 => add_alignment(&mut chao.rec, -0.15),
                    10 => chao.rec.add_need(off::BREED, 10_000),
                    _ => {}
                }
                chao.mail = 4;
            }
        }
    }
    chao.hands = Some(h);
    if fruit.held {
        chao.mail = 6;
        chao.hands = None;
    }
}

/// A bite's effect on the Chao (`0x545790`): evolution +0.005 child / +0.00125 adult
/// (`0x536110`, clamped 0..1.2), life +3 up to its maximum; a child's maximum +5 (`0x5362D0`,
/// to 9999; adults skip it); then the fruit's stats (`0x535F00`, cat 3).
pub(super) fn fruit_bite(rec: &mut Record, kind: u8, last: bool, rng: &mut Rng) {
    let child = rec.u8(off::TYPE) == TYPE_CHILD;
    let evolution = rec.f32(off::EVOLUTION) + if child { 0.005 } else { 0.00125 };
    rec.set_f32(off::EVOLUTION, evolution.clamp(0.0, 1.2));
    let max = i32::from(rec.u16(off::MAX_LIFE));
    let life = (i32::from(rec.u16(off::LIFE)) + 3).min(max).max(0);
    rec.set_u16(off::LIFE, life as u16);
    if child {
        let max = (max + 5).clamp(0, 9999);
        rec.set_u16(off::MAX_LIFE, max as u16);
        if i32::from(rec.u16(off::LIFE)) > max {
            rec.set_u16(off::LIFE, max as u16);
        }
    }
    if let Some(row) = FRUIT_STATS.get(usize::from(kind)) {
        grow_stats(rec, row, Some(last), rng);
    }
}

/// `GrowStats` (`0x535F00`) with a table row: progress `rec+0x20+i` gains the row's value
/// (fruit, `quarter = Some(last)`: a quarter, plus the remainder on the last bite), to no
/// less than 0 (8-bit, so it wraps above 255). Past 99 a stat levels up (`rec+0x30+i`, to
/// 99) and gains points (`0x535E90`: `3·grade + 11 + 0..4` into `rec+0x38+2i`, to 4000);
/// at level 99 progress stops at 99. (The stat-up display copy `0x536040` and the Steam
/// achievement are the client's.)
pub(super) fn grow_stats(rec: &mut Record, row: &[i16; 8], quarter: Option<bool>, rng: &mut Rng) {
    for (i, &v) in row.iter().enumerate() {
        let v = i32::from(v);
        let add = match quarter {
            Some(last) => v / 4 + if last { v % 4 } else { 0 },
            None => v,
        };
        let p = rec.u8(0x20 + i);
        let sum = i32::from(p) + add;
        rec.set_u8(0x20 + i, if sum < 0 { 0 } else { sum as u8 });
    }
    for i in 0..8 {
        let p = rec.u8(0x20 + i);
        if p <= 99 {
            continue;
        }
        let level = rec.u8(0x30 + i);
        if level >= 99 {
            rec.set_u8(0x20 + i, 99);
            continue;
        }
        rec.set_u8(0x20 + i, p - 100);
        rec.set_u8(0x30 + i, level + 1);
        let grade = u16::from(rec.u8(0x28 + i)).wrapping_mul(3);
        let roll = (f64::from(rng.r()) * -4.999_899_864_196_777) as i32 as u16;
        let gain = grade.wrapping_sub(roll).wrapping_add(11);
        let points = rec.u16(0x38 + 2 * i).wrapping_add(gain);
        rec.set_u16(0x38 + 2 * i, points.min(4000));
    }
}

/// Alignment `rec+0xB0` plus `by`, clamped ±1 (`0x536260`; it also marks the look dirty).
pub(super) fn add_alignment(rec: &mut Record, by: f32) {
    let v = rec.f32(off::ALIGNMENT) + by;
    rec.set_f32(off::ALIGNMENT, v.clamp(-1.0, 1.0));
}

/// `AddHappiness` (`0x536320`): happiness `rec+0x82` plus `delta` (±100); alignment by
/// `align`, signed by the local player's character (Sonic, Tails, Knuckles, 8: +; Shadow,
/// Eggman, Rouge: −; the walkers 6/7: none); its bond (`0x53E120`); emotion ball 4 (delta
/// > 0) or 5 for 180 frames.
pub(super) fn add_happiness(chao: &mut Chao, delta: i32, align: f32, character: Option<u8>) {
    let h = (i32::from(chao.rec.i16(off::HAPPINESS)) + delta).clamp(-100, 100);
    chao.rec.set_u16(off::HAPPINESS, h as i16 as u16);
    match character {
        Some(0 | 2 | 4 | 8) => add_alignment(&mut chao.rec, align),
        Some(1 | 3 | 5) => add_alignment(&mut chao.rec, -align),
        _ => {}
    }
    bond(&mut chao.rec, delta, character);
    chao.emotion_ball(if delta > 0 { 4 } else { 5 }, 180);
}

/// `0x53E120`: the bond with the local player's character (0..5, else bond 0): like `+0`
/// plus `delta` (±100). A kind act (delta > 0) also eases `+1` (the fear the scared
/// candidate reads) by `(300 + p14C)·10/300 + (300 + p155)·10/300` (`0x1312D6A` = 10; to
/// 0..200). Then `0x53E280`: `+4` one more (to 0xFFFF), `+2` to `2·(+2 / 3)`.
fn bond(rec: &mut Record, delta: i32, character: Option<u8>) {
    let b = off::BONDS + 6 * usize::from(character.filter(|&c| c <= 5).unwrap_or(0));
    let like = (i32::from(rec.i8(b)) + delta).clamp(-100, 100);
    rec.set_u8(b, like as i8 as u8);
    if delta > 0 {
        let ease = |p: i32| (p + 300) * 10 / 300;
        let fear = i32::from(rec.u8(b + 1)) - ease(rec.p(0x14C)) - ease(rec.p(0x155));
        rec.set_u8(b + 1, fear.clamp(0, 200) as u8);
    }
    let count = rec.u16(b + 4);
    if count < 0xFFFF {
        rec.set_u16(b + 4, count + 1);
    }
    rec.set_u16(b + 2, rec.u16(b + 2) / 3 * 2);
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, Tri, V3};
    use super::super::super::record::Record;
    use super::*;
    use std::cell::RefCell;

    fn garden() -> Garden {
        Garden {
            tris: vec![
                Tri {
                    v: [
                        [-100.0, 0.0, -100.0],
                        [100.0, 0.0, -100.0],
                        [-100.0, 0.0, 100.0],
                    ],
                    flags: 1,
                },
                Tri {
                    v: [
                        [100.0, 0.0, -100.0],
                        [100.0, 0.0, 100.0],
                        [-100.0, 0.0, 100.0],
                    ],
                    flags: 1,
                },
            ],
            grid: vec![1; GRID_CELLS],
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    /// One fruit (or none) and maybe a player, kept up to date from the Chao's fruit events
    /// the way a server would.
    struct Orchard {
        fruit: RefCell<Option<Fruit>>,
        touching: bool,
        player: Option<V3>,
    }

    impl Orchard {
        fn new(kind: u8, pos: V3) -> Self {
            Self {
                fruit: RefCell::new(Some(Fruit {
                    id: 7,
                    kind,
                    bites: 4,
                    pos,
                    held: false,
                    taken: false,
                })),
                touching: false,
                player: None,
            }
        }

        fn update(&self, chao: &mut Chao) {
            let mut fruit = self.fruit.borrow_mut();
            for e in chao.fruit_events.drain(..) {
                match e {
                    FruitEvent::Took { .. } => {
                        if let Some(f) = fruit.as_mut() {
                            f.held = false;
                        }
                    }
                    FruitEvent::Bite { left: 0, .. } => *fruit = None,
                    FruitEvent::Bite { left, .. } => fruit.as_mut().unwrap().bites = left,
                    FruitEvent::Dropped { .. } => {}
                }
            }
            if let (Some(f), Some(_)) = (fruit.as_mut(), chao.hands) {
                f.pos = chao.body.pos;
            }
        }
    }

    impl Senses for Orchard {
        fn nearest_fruit(&self) -> Option<Fruit> {
            *self.fruit.borrow()
        }
        fn fruit(&self, id: u32) -> Option<Fruit> {
            self.fruit.borrow().filter(|f| f.id == id)
        }
        fn touching_fruit(&self) -> Option<Fruit> {
            self.fruit.borrow().filter(|_| self.touching)
        }
        fn player_pos(&self) -> Option<V3> {
            self.player
        }
        fn player_character(&self) -> Option<u8> {
            Some(0)
        }
    }

    fn child(set: impl Fn(&mut Record)) -> Chao {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, TYPE_CHILD);
        rec.set_u16(off::MAX_LIFE, 3000);
        rec.set_u16(off::LIFE, 2000);
        set(&mut rec);
        Chao::new([0.0; 3], 0, rec)
    }

    /// Runs until the Chao is back to settling or `limit` frames; every frame the need
    /// latch is opened again (as the life tick would, far more often).
    fn live(c: &mut Chao, o: &Orchard, limit: u32, rng: &mut Rng) -> Vec<u32> {
        let garden = garden();
        let mut seen = Vec::new();
        for _ in 0..limit {
            c.rec.set_u16(off::NEED_CLOCK, 0);
            c.frame(&garden, 0, o, rng);
            o.update(c);
            let now = c.current().map(|b| b.address()).unwrap_or(0);
            if seen.last() != Some(&now) {
                seen.push(now);
            }
            if c.current() == Some(Behaviour::Settle) {
                break;
            }
        }
        seen
    }

    #[test]
    fn a_hungry_chao_walks_to_the_fruit_and_eats_it_up() {
        let mut rng = Rng::new(4);
        let o = Orchard::new(3, [0.0, 0.0, 20.0]);
        let mut c = child(|r| r.set_u16(off::HUNGER, 10_000));
        c.set(Behaviour::Game(GO_EAT), -1);
        let mut bites = 0;
        let garden = garden();
        let mut seen = Vec::new();
        for _ in 0..20_000 {
            // Keep it hungry so it eats the whole fruit.
            c.rec.set_u16(off::HUNGER, 10_000);
            c.rec.set_u16(off::NEED_CLOCK, 0);
            c.frame(&garden, 0, &o, &mut rng);
            bites += c
                .fruit_events
                .iter()
                .filter(|e| matches!(e, FruitEvent::Bite { .. }))
                .count();
            o.update(&mut c);
            let now = c.current().map(|b| b.address()).unwrap_or(0);
            if seen.last() != Some(&now) {
                seen.push(now);
            }
            if c.current() == Some(Behaviour::Settle) {
                break;
            }
        }
        assert_eq!(
            &seen[..5],
            &[0x55C3C0, reach::NOTICE, reach::WALK_TO_LINK, PICK_UP, EAT]
        );
        assert_eq!(bites, 4);
        assert!(o.fruit.borrow().is_none());
        // Kind 3: stamina +50, a quarter (12) a bite and the remainder (2) on the last.
        assert_eq!(c.rec.u8(0x20 + 4), 50);
        assert_eq!(c.rec.u16(off::LIFE), 2012);
        assert_eq!(c.rec.u16(off::MAX_LIFE), 3020);
        assert!((c.rec.f32(off::EVOLUTION) - 0.02).abs() < 1e-6);
        assert!(c.hands.is_none());
    }

    #[test]
    fn eating_stops_when_full_and_throws_the_rest_away() {
        let mut rng = Rng::new(9);
        let o = Orchard::new(3, [0.0; 3]);
        let mut c = child(|r| {
            r.set_u16(off::HUNGER, 4000);
            r.set_u16(off::TIRE, 3000);
        });
        let fruit = o.nearest_fruit().unwrap();
        grab(&mut c, fruit, false, &o);
        c.set(Behaviour::Game(EAT), -1);
        let mut thrown = None;
        let garden = garden();
        for _ in 0..5000 {
            c.rec.set_u16(off::NEED_CLOCK, 0);
            c.frame(&garden, 0, &o, &mut rng);
            for e in &c.fruit_events {
                if let FruitEvent::Dropped { vel, .. } = e {
                    thrown = *vel;
                }
            }
            o.update(&mut c);
            if c.current() != Some(Behaviour::Game(EAT)) {
                break;
            }
        }
        // Two need periods of −2400 empty HUNGER 4000; TIRE goes with it.
        assert_eq!(c.rec.u16(off::HUNGER), 0);
        assert_eq!(c.rec.u16(off::TIRE), 0);
        let v = thrown.expect("thrown away");
        assert!((v[1] - 0.7).abs() < 1e-6 && (v[0].hypot(v[2]) - 0.3).abs() < 1e-4);
        assert_eq!(c.motion, 0xBA);
    }

    #[test]
    fn a_disliked_fruit_is_thrown_away_unhappily() {
        let mut rng = Rng::new(2);
        let o = Orchard::new(7, [0.0; 3]);
        let mut c = child(|r| {
            r.set_u8(0x157, 4);
            r.set_u16(off::HUNGER, 9000);
        });
        let fruit = o.nearest_fruit().unwrap();
        grab(&mut c, fruit, false, &o);
        c.set(Behaviour::Game(EAT), -1);
        live(&mut c, &o, 2000, &mut rng);
        assert_eq!(c.rec.i16(off::HAPPINESS), -1);
        assert_eq!(c.rec.i8(off::BONDS), -1);
        assert!((c.rec.f32(off::ALIGNMENT) + 0.1).abs() < 1e-6);
        assert!(c.sounds.contains(&0x603A));
        // Never bitten: HUNGER as it was.
        assert_eq!(c.rec.u16(off::HUNGER), 9000);
    }

    #[test]
    fn a_fruit_taken_by_another_chao_cannot_be_eaten_twice() {
        let mut rng = Rng::new(1);
        let o = Orchard::new(3, [0.0; 3]);
        let mut c = child(|r| r.set_u16(off::HUNGER, 10_000));
        grab(&mut c, o.nearest_fruit().unwrap(), false, &o);
        c.set(Behaviour::Game(EAT), -1);
        c.mode = 6;
        c.hands.as_mut().unwrap().chew = 181;
        o.fruit.borrow_mut().as_mut().unwrap().taken = true;
        c.fruit_events.clear();
        c.frame(&garden(), 0, &o, &mut rng);
        assert!(c.hands.is_none());
        assert!(c.fruit_events.is_empty());
        assert_eq!(c.rec.u8(0x20 + 4), 0);
        c.frame(&garden(), 0, &o, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn a_fruit_taken_away_between_bites_angers() {
        let mut rng = Rng::new(1);
        let o = Orchard::new(3, [0.0; 3]);
        let mut c = child(|_| {});
        let fruit = o.nearest_fruit().unwrap();
        grab(&mut c, fruit, false, &o);
        c.set(Behaviour::Game(EAT), -1);
        // Mode 1 waits for 0xA9's end; a player takes the fruit meanwhile.
        o.fruit.borrow_mut().as_mut().unwrap().held = true;
        let garden = garden();
        c.frame(&garden, 0, &o, &mut rng);
        assert_eq!((c.hands, c.mail), (None, 6));
        c.frame(&garden, 0, &o, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Settle));
        assert_eq!((c.rec.u8(0x12D), c.rec.u8(0x12E)), (200, 200));
        assert_eq!(c.rec.i16(off::HAPPINESS), -1);
    }

    #[test]
    fn taking_a_whole_fruit_from_a_hand_pleases() {
        let mut rng = Rng::new(1);
        let mut o = Orchard::new(3, [0.0, 0.0, 2.0]);
        o.touching = true;
        o.fruit.borrow_mut().as_mut().unwrap().held = true;
        let mut c = child(|r| r.set_u8(0x14C, 0));
        c.body.target = [0.0, 0.0, 2.0];
        c.set(Behaviour::Game(TAKE_OFFERED), -1);
        c.frame(&garden(), 0, &o, &mut rng);
        assert_eq!(
            c.fruit_events[0],
            FruitEvent::Took {
                id: 7,
                player_drops: true
            }
        );
        assert_eq!(c.current(), Some(Behaviour::Game(EAT)));
        assert_eq!(c.rec.i16(off::HAPPINESS), 1);
        assert!((c.rec.f32(off::ALIGNMENT) - 0.025).abs() < 1e-6);
        assert_eq!((c.rec.i8(off::BONDS), c.rec.u16(off::BONDS + 4)), (1, 1));
        assert_eq!(c.bubble.0, 4);
    }

    #[test]
    fn nothing_to_take_ends_on_frame_61() {
        let mut rng = Rng::new(1);
        let o = Orchard {
            fruit: RefCell::new(None),
            touching: false,
            player: None,
        };
        let mut c = child(|_| {});
        c.set(Behaviour::Game(TAKE_OFFERED), -1);
        let garden = garden();
        for _ in 0..60 {
            c.frame(&garden, 0, &o, &mut rng);
            assert_eq!(c.current(), Some(Behaviour::Game(TAKE_OFFERED)));
        }
        c.frame(&garden, 0, &o, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Settle));
        assert!(c.sounds.contains(&0x6037));
    }

    #[test]
    fn begging_walks_up_to_the_player() {
        let mut rng = Rng::new(1);
        let mut o = Orchard::new(3, [0.0; 3]);
        o.player = Some([0.0, 0.0, 30.0]);
        let mut c = child(|_| {});
        c.set(Behaviour::Game(BEG), -1);
        c.push(Behaviour::Game(BEG_WAIT));
        let seen = live(&mut c, &o, 3000, &mut rng);
        assert_eq!(&seen[..3], &[BEG, BEG_WAIT, 0x565BE0]);
        assert!(c.body.pos[2] > 20.0);
        assert_eq!(c.motion, 0x18E);
    }

    #[test]
    fn begging_gives_up_after_1001_frames() {
        let mut rng = Rng::new(1);
        let mut o = Orchard::new(3, [0.0; 3]);
        o.player = Some([0.0, 80.0, 0.0]);
        let mut c = child(|_| {});
        c.set(Behaviour::Game(BEG), -1);
        let garden = garden();
        for _ in 0..1001 {
            c.frame(&garden, 0, &o, &mut rng);
        }
        assert_eq!(c.current(), Some(Behaviour::Game(BEG)));
        c.frame(&garden, 0, &o, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn going_to_eat_waits_while_another_chao_holds_it() {
        let mut rng = Rng::new(1);
        let o = Orchard::new(3, [0.0; 3]);
        o.fruit.borrow_mut().as_mut().unwrap().taken = true;
        let mut c = child(|_| {});
        c.set(Behaviour::Game(GO_EAT), -1);
        for _ in 0..10 {
            c.frame(&garden(), 0, &o, &mut rng);
        }
        assert_eq!(c.current(), Some(Behaviour::Game(GO_EAT)));
        *o.fruit.borrow_mut() = None;
        c.frame(&garden(), 0, &o, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn stats_level_up_past_99() {
        let mut rng = Rng::new(1);
        let mut rec = Record::default();
        rec.set_u8(0x20 + 4, 90);
        rec.set_u8(0x28 + 4, 2);
        // Kind 1 (stamina +99): 24 a bite, 27 on the last.
        fruit_bite(&mut rec, 1, true, &mut rng);
        assert_eq!((rec.u8(0x20 + 4), rec.u8(0x30 + 4)), (17, 1));
        let points = rec.u16(0x38 + 8);
        assert!((17..=21).contains(&points), "{points}");
        // Adults' maximum life doesn't grow.
        rec.set_u8(off::TYPE, 5);
        rec.set_u16(off::MAX_LIFE, 100);
        rec.set_u16(off::LIFE, 99);
        fruit_bite(&mut rec, 0, false, &mut rng);
        assert_eq!((rec.u16(off::LIFE), rec.u16(off::MAX_LIFE)), (100, 100));
        // Level 99 stops at 99.
        rec.set_u8(0x30 + 4, 99);
        rec.set_u8(0x20 + 4, 95);
        fruit_bite(&mut rec, 1, false, &mut rng);
        assert_eq!(rec.u8(0x20 + 4), 99);
    }

    #[test]
    fn kindness_eases_fear() {
        let mut c = child(|r| {
            r.set_u8(off::BONDS + 6 * 3 + 1, 100);
            r.set_u16(off::BONDS + 6 * 3 + 2, 301);
        });
        add_happiness(&mut c, 1, 0.1, Some(3));
        let b = off::BONDS + 18;
        assert_eq!(
            (c.rec.i8(b), c.rec.u8(b + 1), c.rec.u16(b + 2)),
            (1, 80, 200)
        );
        // Eggman: alignment the other way.
        assert!((c.rec.f32(off::ALIGNMENT) + 0.1).abs() < 1e-6);
    }
}

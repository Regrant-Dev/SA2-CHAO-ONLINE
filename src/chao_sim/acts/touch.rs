//! What the player and the things a Chao bumps into do to it: the interrupt checks before
//! every behaviour (`0x53DB10` steps 4..7) and what they install: coming to a whistle, chasing
//! the ball, taking a hat, seed or animal, the 0xA2 fright, being held, dropped or thrown,
//! petted and hit (re/chao-ai.md §1.4, §2B "Player interactions", §6.2, §3.3). Decoded from
//! the game's bytes. The stats balloon (`0x565B50`, `0x565B90`), motion speeds, the knocked
//! spin (`D+0x50`) and the collision bits (`D+0xA0 & 8`, `0x54A690`) are the clients' and
//! aren't modelled; the items themselves are the server's (`ItemEvent`).

use super::super::brain::{Behaviour, Chao, ItemEvent, Next, Rng};
use super::super::land::Garden;
use super::super::physics::WALL_PUSHED;
use super::super::picker::{Release, Senses};
use super::super::record::off;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    _garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    Some(match address {
        0x5634D0 => come_to_whistle(chao, senses, rng),
        0x563250 => walk_to_player(chao, senses, rng),
        0x563830 => chase_ball(chao, senses, rng),
        0x569240 => take_hat(chao, senses),
        0x569340 => look_at_hat(chao, senses, rng),
        0x569430 => wear_hat(chao, senses),
        0x569550 => toss_hat(chao, senses),
        0x567640 => take_seed(chao, senses),
        0x566C40 => take_animal(chao, senses),
        0x5647B0 => startled(chao, rng),
        0x566670 => held(chao, senses),
        0x5660A0 => held_glad(chao, senses, rng),
        0x5662B0 => held_calm(chao, senses),
        0x566300 => held_cross(chao, senses, rng),
        0x566580 => held_sleepy(chao, senses),
        0x53D720 => set_down(chao),
        0x564320 => landed(chao, senses, rng),
        0x5648A0 => petted(chao, senses, rng),
        0x563EB0 => knocked(chao, senses, rng),
        _ => return None,
    })
}

/// The held behaviours: the held check leaves them be (`0x566720`).
const HELD: [u32; 5] = [0x566670, 0x5660A0, 0x5662B0, 0x566300, 0x566580];

/// The interrupt checks from things around the Chao (`0x53DB10` steps 4..7, re/chao-ai.md
/// §1.4): whistle, ball, hat, seed, fruit, animal, 0xA2 (skipped in the water or asleep), then
/// held, else petted, then hit. Each sees what the ones before installed, and the last to
/// fire wins. True when one fired (the water check counts).
pub(super) fn checks(
    chao: &mut Chao,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
    water: bool,
    asleep: bool,
) -> bool {
    let _ = garden;
    let mut fired = water;
    if !water && !asleep {
        // `|=` runs every check, as the game does.
        fired |= whistle_check(chao, senses);
        fired |= ball_check(chao, senses);
        fired |= hat_check(chao, senses);
        fired |= seed_check(chao, senses);
        fired |= fruit_check(chao, senses);
        fired |= animal_check(chao, senses);
        fired |= fright_check(chao, senses);
    }
    if held_check(chao, senses) || pet_check(chao, senses) {
        fired = true;
    }
    let hit = hit_check(chao, senses, rng);
    if (fired || hit) && senses.carrying().is_some() {
        // Its ALW entry's flags and link are cleared and it lets go (`0x530690`). The
        // balloon `0x565B90` closes after, unless only the held check fired.
        chao.items.push(ItemEvent::LetGo);
    }
    fired || hit
}

fn current(chao: &Chao) -> u32 {
    chao.current().map_or(0, |b| b.address())
}

/// `0x5637B0`: player 1 whistling within 100 units (squared 3D distance < 10000) brings a
/// Chao that likes them (bond > 50) over; any whistle in range counts as fired.
fn whistle_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    if !senses.whistling() {
        return false;
    }
    let Some(p) = senses.player_pos() else {
        return false;
    };
    if distance2(p, chao.body.pos) >= 10_000.0 {
        return false;
    }
    if like(chao, senses) > 50 {
        chao.set(Behaviour::Game(0x5634D0), -1);
    }
    true
}

/// `0x563B70`: a rolling ball within 50 of a bored Chao (TEDIOUS > 4000): TEDIOUS −2000 and
/// chase. It doesn't spare a chase under way, so it restarts it every frame it holds.
fn ball_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    let Some(ball) = senses.ball().filter(|b| b.moving) else {
        return false;
    };
    if chao.rec.u16(off::TEDIOUS) <= 4000 || distance2(ball.pos, chao.body.pos).sqrt() >= 50.0 {
        return false;
    }
    chao.rec.add_need(off::TEDIOUS, -2000);
    chao.set(Behaviour::Game(0x563830), -1);
    true
}

/// The item touches (`0x5691B0` hat, `0x560640` fruit, `0x566BC0` animal): not while in one
/// of `not`; the item becomes the target.
fn item_check(chao: &mut Chao, senses: &dyn Senses, kind: u8, not: [u32; 3], next: u32) -> bool {
    if not.contains(&current(chao)) {
        return false;
    }
    let Some(pos) = senses.touch(kind) else {
        return false;
    };
    chao.body.target = pos;
    chao.set(Behaviour::Game(next), -1);
    true
}

/// `0x5691B0`: a hat it touches, not while eating, sleeping or taking one.
fn hat_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    item_check(chao, senses, 0x93, [0x5607C0, 0x54EF10, 0x569240], 0x569240)
}

/// `0x567560`: a seed it touches, not while sleeping or eating, with run points (×3 in the
/// gardens) of 150 or more and no mutual link (ALW `+0x40`).
fn seed_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    let cur = current(chao);
    if cur == 0x54EF10 || cur == 0x5607C0 {
        return false;
    }
    // 32-bit here, unlike the walks' 16-bit compare (`0x5675A4`).
    if u32::from(chao.rec.u16(off::RUN_POINTS)) * 3 < 150 {
        return false;
    }
    // Linked both ways with something: a fruit or another item in its hands, or the horse.
    if chao.hands.is_some() || senses.carrying().is_some() || chao.toy_link.is_some() {
        return false;
    }
    item_check(chao, senses, 0x85, [0x567640; 3], 0x567640)
}

/// `0x560640`: a fruit it touches that no Chao has, not while eating, sleeping or going for
/// one.
fn fruit_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    if [0x5607C0, 0x54EF10, 0x5606D0].contains(&current(chao)) {
        return false;
    }
    let Some(fruit) = senses.touching_fruit().filter(|f| !f.taken) else {
        return false;
    };
    chao.body.target = fruit.pos;
    chao.set(Behaviour::Game(0x5606D0), -1);
    true
}

/// `0x566BC0`: an animal or Chaos Drive it touches, not while taking or absorbing one.
fn animal_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    item_check(chao, senses, 0x87, [0x566DF0, 0x566C40, 0x566C40], 0x566C40)
}

/// `0x564760`: touching an 0xA2 collision startles it for 500 frames.
fn fright_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    if current(chao) == 0x5647B0 || senses.touch(0xA2).is_none() {
        return false;
    }
    chao.set(Behaviour::Game(0x5647B0), 500);
    true
}

/// `0x566720`: picked up. It turns with its holder: from its facing less 0xFA0 toward a
/// quarter turn either side, the one facing away from the holder when it faced them.
fn held_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    if HELD.contains(&current(chao)) || !chao.held {
        return false;
    }
    let holder = senses.player_rot_y().unwrap_or(0);
    chao.held_turn[1] = holder.wrapping_add(chao.body.rot_y).wrapping_sub(0xFA0);
    let to_player = angle_to_player(chao, senses);
    let off = (chao.body.rot_y.wrapping_sub(to_player) as i16).unsigned_abs();
    chao.held_turn[0] = if off < 0x4000 { 0xC000 } else { 0x4000 };
    chao.body.vel = [0.0; 3];
    chao.body.accel = [0.0; 3];
    chao.set(Behaviour::Game(0x566670), -1);
    true
}

/// `0x564C10`: a hand on it while it stands on the ground (Status `D+4 & 1`, set with the
/// physics' on-ground bit, `0x53DF90`), unless already petted, in deep sleep or sad.
fn pet_check(chao: &mut Chao, senses: &dyn Senses) -> bool {
    if [0x5648A0, 0x54FCF0, 0x59FCA0, 0x59F970, 0x59FA50].contains(&current(chao))
        || !chao.body.on_ground()
        || !senses.petted()
    {
        return false;
    }
    chao.set(Behaviour::Game(0x5648A0), -1);
    true
}

/// `0x563D30`: a player's attack knocks it back 0.8 away and 1.5 up, and a worn hat falls
/// off when the save has room for it. (The game normalises (dx, ?, dz) with its y slot never
/// written; taken as 0.)
fn hit_check(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> bool {
    if [0x563EB0, 0x568D30, 0x568F60].contains(&current(chao)) {
        return false;
    }
    let Some(at) = senses.hit_by() else {
        return false;
    };
    let (dx, dz) = (at[0] - chao.body.pos[0], at[2] - chao.body.pos[2]);
    let len = (dx * dx + dz * dz).sqrt();
    let (nx, nz) = if len > 0.0 {
        (dx / len, dz / len)
    } else {
        (dx, dz)
    };
    chao.body.vel = [-nx * 0.8, 1.5, -nz * 0.8];
    chao.set(Behaviour::Game(0x563EB0), -1);
    if chao.rec.u8(off::HAT) != 0 && senses.hat_room() {
        hat_off(chao, rng);
    }
    true
}

/// The hat falls off (`0x54A540`): a hat at the Chao, facing a random way (r·360·182.044).
fn hat_off(chao: &mut Chao, rng: &mut Rng) {
    let rot_y = (f64::from(rng.r()) * 360.0 * 182.044_448_852_539_06) as i32 as u16;
    chao.items.push(ItemEvent::HatOff {
        kind: chao.rec.u8(off::HAT),
        rot_y,
        vel: chao.body.vel,
    });
    chao.rec.set_u8(off::HAT, 0);
}

fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// The bond slot of player 1's character (`0x53E0C0`): characters 1..5 their own, else 0.
fn bond(senses: &dyn Senses) -> usize {
    match senses.player_character() {
        Some(c @ 1..=5) => usize::from(c),
        _ => 0,
    }
}

/// How much it likes player 1 (bond `+0`).
fn like(chao: &Chao, senses: &dyn Senses) -> i32 {
    chao.rec.p(off::BONDS + bond(senses) * 6)
}

/// `0x7969B0`: the bearing to player 1.
fn angle_to_player(chao: &Chao, senses: &dyn Senses) -> u16 {
    senses.player_pos().map_or(0, |p| {
        let a = (p[0] - chao.body.pos[0]).atan2(p[2] - chao.body.pos[2]);
        (f64::from(a) * 10_430.380_434_934_39) as i32 as u16
    })
}

/// `0x796AB0(player 0, rate)`: turn toward player 1; what's left to turn.
fn turn_to_player(chao: &mut Chao, senses: &dyn Senses, rate: u16) -> u16 {
    let Some(p) = senses.player_pos() else {
        return 0;
    };
    let target = chao.body.target;
    chao.body.target = p;
    let left = chao.body.turn_to_target(rate);
    chao.body.target = target;
    left
}

/// `0x446960`: `from` toward `to` by at most `rate`, the short way.
fn approach(from: u16, to: u16, rate: u16) -> u16 {
    let diff = i32::from(to.wrapping_sub(from) as i16);
    if diff > i32::from(rate) || diff < -i32::from(rate) {
        if diff < 0 {
            from.wrapping_sub(rate)
        } else {
            from.wrapping_add(rate)
        }
    } else {
        to
    }
}

/// The facing along the velocity (`0x859B6A`·10430.38, truncated).
fn face_flight(chao: &mut Chao) {
    let a = chao.body.vel[0].atan2(chao.body.vel[2]);
    chao.body.rot_y = (f64::from(a) * 10_430.380_434_934_39) as i32 as u16;
}

/// `AddHappiness` (`0x536320`, acts/food.rs) with player 1's character.
fn add_happiness(chao: &mut Chao, senses: &dyn Senses, delta: i32, align: f32) {
    super::food::add_happiness(chao, delta, align, senses.player_character());
}

/// `0x53E1F0`: fear (bond `+1`, 0..200) up by `n(300−p155)/300 + n(300−p14C)/300`.
fn scare(chao: &mut Chao, senses: &dyn Senses, n: i32) {
    let at = off::BONDS + bond(senses) * 6;
    let up = n * (300 - chao.rec.p(0x155)) / 300 + n * (300 - chao.rec.p(0x14C)) / 300;
    let fear = (i32::from(chao.rec.u8(at + 1)) + up).clamp(0, 200);
    chao.rec.set_u8(at + 1, fear as u8);
}

/// Hit or thrown: emotions `0x12D..=0x131` each up by trunc(50 + 100r).
fn upset(chao: &mut Chao, rng: &mut Rng) {
    for at in 0x12D..=0x131 {
        let by = (f64::from(rng.r()) * 100.0 + 50.0) as i32;
        chao.rec.add_emotion(at, by);
    }
}

/// The countdown most of these use: true when the old count was spent.
fn spent(chao: &mut Chao) -> bool {
    let old = chao.count_a;
    chao.count_a -= 1;
    old <= 0
}

/// `0x5634D0` (whistle): stand 20..59 frames, look up (0xEE or 3) 20..79 frames, then turn
/// to player 1; facing them, maybe wave (0x1EC), then walk over (`0x563250`).
fn come_to_whistle(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.count_a = 20 + rng.below(40);
            chao.play_blend(0, 10);
            chao.mode = 1;
        }
        1 => {
            if spent(chao) {
                chao.play_blend(if rng.r() < 0.5 { 0xEE } else { 3 }, 10);
                chao.emotion_ball(3, 180);
                chao.mode = 2;
                chao.count_a = 20 + rng.below(60);
            }
        }
        2 => {
            if spent(chao) {
                chao.play_blend(0x64, 10);
                chao.mode = 3;
            }
        }
        3 => {
            // Motion speed `D+0xB8 = 1.5`: the clients'.
            if turn_to_player(chao, senses, 0x300) < 0x100 {
                chao.emotion_ball(1, 60);
                chao.count_a = 20 + rng.below(20);
                if rng.r() < 0.7 {
                    chao.play_blend(0x1EC, 10);
                    chao.set_eyes(4, -1);
                    chao.mode = 4;
                    chao.sound(0x6019);
                } else {
                    chao.mode = 5;
                }
            }
        }
        4 => {
            if chao.motion_done() {
                chao.mode = 5;
            }
        }
        5 => {
            let done = spent(chao);
            if done {
                chao.set(Behaviour::Game(0x563250), -1);
            }
        }
        _ => {}
    }
    Next::Keep
}

/// `0x563250`: walk to player 1 until within 8 units (3D), tripping now and then.
fn walk_to_player(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        let r = rng.r();
        chao.play_blend(
            if r < 0.2 {
                0x76
            } else if r < 0.6 {
                0x77
            } else {
                0x6A
            },
            10,
        );
        chao.set_eyes(4, -1);
        chao.set_mouth(if rng.r() < 0.4 { 1 } else { 3 }, -1);
        chao.emotion_ball(4, 120);
        chao.mode = 1;
        chao.count_a = 0;
    }
    if chao.mode == 1
        && let Some(p) = senses.player_pos()
    {
        chao.body.target = p;
    }
    chao.count_a += 1;
    if chao.count_a % 100 == 0 && rng.r() < 0.1 {
        chao.set(Behaviour::Stumble, -1);
    }
    if distance2(chao.body.target, chao.body.pos) < 64.0 {
        return Next::Pop;
    }
    chao.body.turn_to_target(0x100);
    let run = f64::from(u32::from(chao.rec.u16(off::RUN_POINTS)) * 3);
    chao.body.drive((0.05 * (run * 0.0001 + 1.0)) as f32);
    Next::Keep
}

/// `0x563830`: chase the ball (aiming at its x and z ×1.02) until it's picked up or falls in
/// the water (a sulk), for up to 3000 frames, while bored (TEDIOUS ≥ 1000) and not too tired
/// (TIRE ≤ 7000). Once a need period: TIRE +300, TEDIOUS −100.
fn chase_ball(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        let r = rng.r();
        chao.play_blend(
            if r < 0.15 {
                0x76
            } else if r < 0.6 {
                0x77
            } else {
                0x6A
            },
            10,
        );
        // Both sides of this roll give eyes 4.
        let _ = rng.r();
        chao.set_eyes(4, -1);
        chao.set_mouth(if rng.r() < 0.4 { 1 } else { 3 }, -1);
        chao.emotion_ball(1, 120);
        chao.mode = 1;
        chao.count_a = 0;
    }
    let ball = senses.ball();
    if chao.mode == 1
        && let Some(b) = ball
    {
        chao.body.target = [b.pos[0] * 1.02, b.pos[1], b.pos[2] * 1.02];
        if b.held {
            return Next::Pop;
        }
    }
    chao.count_a += 1;
    if chao.count_a % 100 == 0 && rng.r() < 0.15 {
        chao.set(Behaviour::Stumble, -1);
    }
    if ball.is_some_and(|b| b.in_water) {
        chao.emotion_ball(5, 240);
        return Next::Pop;
    }
    chao.body.turn_to_target(0x180);
    let run = f32::from(chao.rec.scaled(off::RUN_POINTS));
    chao.body.drive(7e-6 * run + 0.028);
    if chao.rec.need_latch() {
        chao.rec.add_need(off::TIRE, 300);
        chao.rec.add_need(off::TEDIOUS, -100);
    }
    if chao.count_a > 3000 || chao.rec.u16(off::TEDIOUS) < 1000 || chao.rec.u16(off::TIRE) > 7000 {
        return Next::Pop;
    }
    Next::Keep
}

/// What the grab does this frame.
enum Grab {
    Turning,
    Took,
    Missed,
}

/// The grab (`0x569240`, `0x567640`, `0x566C40`): reach out (0xCA) and turn to the target
/// 0x600 a frame; once within `within`, take the item of `kind` if it's touching it. With
/// `window`, a miss counts down 60 frames; without, it gives up at once.
fn grab(chao: &mut Chao, senses: &dyn Senses, kind: u8, within: u16, window: bool) -> Grab {
    if chao.mode == 0 {
        chao.play_blend(0xCA, 15);
        chao.mode = 1;
        if window {
            chao.count_a = 60;
        }
    }
    if chao.body.turn_to_target(0x600) >= within {
        return Grab::Turning;
    }
    if senses.touch(kind).is_some() {
        chao.items.push(ItemEvent::Take(kind));
        return Grab::Took;
    }
    if window && !spent(chao) {
        Grab::Turning
    } else {
        Grab::Missed
    }
}

/// `0x569240`: take the hat, look at it (`0x569340`), then wear it (`0x569430`) if it has
/// absorbed animal 16 and wears none, else toss it (`0x569550`).
fn take_hat(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match grab(chao, senses, 0x93, 0xB6, true) {
        Grab::Turning => Next::Keep,
        Grab::Missed => Next::Pop,
        Grab::Took => {
            chao.set(Behaviour::Game(0x569340), -1);
            let wear = chao.rec.u32(0x118) & 0x10000 != 0 && chao.rec.u8(off::HAT) == 0;
            chao.push(Behaviour::Game(if wear { 0x569430 } else { 0x569550 }));
            Next::Keep
        }
    }
}

/// `0x567640`: take the seed, then carry it (`0x567710`).
fn take_seed(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match grab(chao, senses, 0x85, 0xB6, true) {
        Grab::Turning => Next::Keep,
        Grab::Missed => Next::Pop,
        Grab::Took => {
            chao.set(Behaviour::Game(0x567710), -1);
            Next::Keep
        }
    }
}

/// `0x566C40`: take the animal or drive (within 0x71C), then absorb it (`0x566DF0`).
fn take_animal(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match grab(chao, senses, 0x87, 0x71C, false) {
        Grab::Turning => Next::Keep,
        Grab::Missed => Next::Pop,
        Grab::Took => {
            chao.set(Behaviour::Game(0x566DF0), -1);
            Next::Keep
        }
    }
}

/// Still holding it (`0x56CFF0`): the hat behaviours end when it's gone.
fn holding(senses: &dyn Senses) -> Next {
    if senses.carrying().is_some() {
        Next::Keep
    } else {
        Next::Pop
    }
}

/// `0x569340`: look at the hat (0xBE) 120..180 frames.
fn look_at_hat(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play_blend(0xBE, 35);
        chao.mode = 1;
        chao.emotion_ball(3, 120);
        chao.sound(0x601C);
        chao.count_a = 120 + rng.below(61);
    }
    if spent(chao) {
        return Next::Pop;
    }
    holding(senses)
}

/// `0x569430`: put the hat on (0xC7); at the motion's hold it becomes `rec+0xD5` and the hat
/// item goes (0xC8). It ends when the hat is gone from its hands.
fn wear_hat(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0xC7, 35);
            chao.mode = 1;
            chao.emotion_ball(1, 120);
        }
        1 if chao.motion_held() => {
            if let Some(hat) = senses.carrying() {
                chao.rec.set_u8(off::HAT, hat.kind);
                chao.items.push(ItemEvent::Wear);
            }
            chao.play_blend(0xC8, 35);
        }
        // The game's mode 2 (end at the hold) is never reached.
        _ => {}
    }
    holding(senses)
}

/// `0x569550`: toss the hat (0xC9): at the motion's end it flies off 0.3 ahead-left and 0.7
/// up (facing + 0x71C7).
fn toss_hat(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0xC9, 35);
            chao.mode = 1;
        }
        1 if chao.motion_done() => {
            if senses.carrying().is_some() {
                let a = f32::from(chao.body.rot_y.wrapping_add(0x71C7)) * std::f32::consts::TAU
                    / 65536.0;
                chao.items
                    .push(ItemEvent::Throw([a.sin() * 0.3, 0.7, a.cos() * 0.3]));
            }
            return Next::Pop;
        }
        _ => {}
    }
    holding(senses)
}

/// `0x5647B0` (0xA2): a fright (0x238), slowing to a stop; it never ends by itself, the
/// 500-frame limit its interrupt set does.
fn startled(chao: &mut Chao, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(0x238);
        chao.set_eyes(1, -1);
        chao.set_mouth(8, -1);
        chao.emotion_ball(5, 300);
        chao.mode = 1;
        chao.count_a = 0;
        chao.sound(if rng.r() < 0.5 { 0x6029 } else { 0x606D });
    }
    if chao.body.on_ground() {
        for v in &mut chao.body.vel {
            *v *= 0.9;
        }
    }
    Next::Keep
}

/// `0x565DC0`, every frame of the held behaviours: emotion `0x132` full; while held, face
/// as the holder turns, stand still, and every 601 frames let hunger sour the mood (a
/// starving cross Chao in a garden wriggles free); once let go, how it was let go.
fn held_update(chao: &mut Chao, senses: &dyn Senses) {
    // `W+0x10 −= W+0x48` isn't modelled: the server places a held Chao.
    chao.rec.add_emotion(0x132, 200);
    if !chao.held {
        released(chao, senses);
        return;
    }
    let holder = senses.player_rot_y().unwrap_or(0);
    chao.body.rot_y = chao.held_turn[1].wrapping_sub(holder);
    chao.held_turn[1] = approach(chao.held_turn[1], chao.held_turn[0], 0x800);
    chao.body.vel = [0.0; 3];
    chao.body.accel = [0.0; 3];
    let old = chao.count_b;
    chao.count_b += 1;
    if old <= 600 {
        return;
    }
    let hunger = chao.rec.u16(off::HUNGER);
    if hunger >= 10_000 && current(chao) == 0x566300 && (1..=3).contains(&chao.area) {
        // `0x46E5E0`: player 1 drops it.
        chao.held = false;
        chao.set(Behaviour::Settle, -1);
    }
    if hunger > 8000 && current(chao) == 0x5662B0 {
        chao.set(Behaviour::Game(0x566300), -1);
    }
    if hunger > 5000 && current(chao) == 0x5660A0 {
        chao.set(Behaviour::Game(0x5662B0), -1);
    }
    chao.count_b = 0;
}

/// The release in `0x565DC0`.
fn released(chao: &mut Chao, senses: &dyn Senses) {
    match senses.release() {
        Release::NoControl => {
            let [x, _, z] = chao.body.vel;
            let len = (x * x + z * z).sqrt();
            let (nx, nz) = if len > 0.0 {
                (x / len, z / len)
            } else {
                (x, z)
            };
            chao.body.pos[1] += 2.0;
            chao.body.vel = [nx * 0.2, 0.87, nz * 0.2];
            chao.set(Behaviour::Game(0x53D720), -1);
        }
        Release::PutDown => {
            chao.body.vel = [0.0; 3];
            chao.body.accel = [0.0; 3];
            chao.held_turn[0] = 0;
            chao.set(Behaviour::Settle, -1);
        }
        Release::Dropped => {
            chao.held_turn[0] = 0;
            chao.set(Behaviour::Game(0x564320), -1);
        }
    }
}

/// `0x566670`: how it takes being held: sleepy (SLPDPTH > 500, `0x566580`), else by how it
/// likes the holder: > 50 glad (`0x5660A0`), < −50 cross (`0x566300`), else calm
/// (`0x5662B0`), with happiness +1 when `0x132` is under 10.
fn held(chao: &mut Chao, senses: &dyn Senses) -> Next {
    if chao.rec.u16(off::SLPDPTH) > 500 {
        chao.set(Behaviour::Game(0x566580), -1);
    } else {
        let like = like(chao, senses);
        let next = if like > 50 {
            0x5660A0
        } else if like < -50 {
            0x566300
        } else {
            0x5662B0
        };
        chao.set(Behaviour::Game(next), -1);
        if chao.rec.u8(0x132) < 10 {
            add_happiness(chao, senses, 1, 0.033_333_335);
        }
    }
    held_update(chao, senses);
    Next::Keep
}

/// `0x5660A0`: glad in the hands: a happy motion for 140..320 frames (a plain 0x101 for
/// 200..380), again and again.
fn held_glad(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.set_eyes(4, -1);
            chao.set_mouth(if rng.r() < 0.5 { 5 } else { 3 }, -1);
            let r = rng.r();
            chao.count_a = 140 + rng.below(181);
            if r < 0.25 {
                chao.play(0x101);
                chao.set_eyes(0, -1);
                chao.set_mouth(0, -1);
                chao.count_a = 200 + rng.below(181);
            } else {
                chao.play(if r < 0.4 {
                    0x102
                } else if r < 0.55 {
                    0x103
                } else if r < 0.7 {
                    0x105
                } else {
                    0x107
                });
            }
            chao.mode = 1;
        }
        1 => {
            // Again when the time's up.
            let done = spent(chao);
            if done {
                chao.mode = 0;
            }
        }
        _ => {}
    }
    held_update(chao, senses);
    Next::Keep
}

/// `0x5662B0`: calm in the hands (0x101).
fn held_calm(chao: &mut Chao, senses: &dyn Senses) -> Next {
    if chao.mode == 0 {
        chao.play(0x101);
        chao.mode = 1;
    }
    held_update(chao, senses);
    Next::Keep
}

/// `0x566300`: cross in the hands: squirms (0x109..0x10E, or a plain 0x101) for 100..240
/// frames, complaining 40% of the time, again and again.
fn held_cross(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.set_eyes(1, -1);
            chao.set_mouth(4, -1);
            let r = rng.r();
            if r < 0.3 {
                chao.play(0x101);
                chao.set_eyes(0, -1);
                chao.set_mouth(0, -1);
            } else {
                chao.play(if r < 0.5 {
                    0x109
                } else if r < 0.6 {
                    0x10A
                } else if r < 0.7 {
                    0x10B
                } else if r < 0.8 {
                    0x10C
                } else if r < 0.9 {
                    0x10D
                } else {
                    0x10E
                });
            }
            chao.count_a = 100 + rng.below(141);
            if rng.r() < 0.4 {
                let r = rng.r();
                chao.sound(if r < 0.33 {
                    0x6047
                } else if r < 0.66 {
                    0x6048
                } else {
                    0x6049
                });
            }
            chao.mode = 1;
        }
        1 => {
            // Again when the time's up.
            let done = spent(chao);
            if done {
                chao.mode = 0;
            }
        }
        _ => {}
    }
    held_update(chao, senses);
    Next::Keep
}

/// `0x566580`: dozing in the hands (0x101, eyes 9, the sleep ball every 402 frames),
/// SLPDPTH −2 a frame; awake at 0 (that frame without `0x565DC0`).
fn held_sleepy(chao: &mut Chao, senses: &dyn Senses) -> Next {
    if chao.mode == 0 {
        chao.play(0x101);
        chao.set_eyes(9, -1);
        chao.emotion_ball(5, 360);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let old = chao.count_a;
        chao.count_a += 1;
        if old > 400 {
            chao.emotion_ball(5, 360);
            chao.count_a = 0;
        }
        chao.rec.add_need(off::SLPDPTH, -2);
        if chao.rec.u16(off::SLPDPTH) == 0 {
            return Next::Pop;
        }
    }
    held_update(chao, senses);
    Next::Keep
}

/// `0x53D720`: let go with player control off: it stands (motion 0) until something else
/// comes along.
fn set_down(chao: &mut Chao) -> Next {
    if chao.mode == 0 {
        chao.play(0);
        chao.mode = 1;
    }
    Next::Keep
}

/// `0x564320`: dropped or thrown: it flies upset (0x10F; happiness −5, fear, emotions
/// up), loses its hat against a wall, and on landing sits stunned (0x9F) 180..300 frames.
fn landed(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            face_flight(chao);
            chao.set_eyes(1, -1);
            chao.emotion_ball(5, 150);
            chao.play_blend(0x10F, 15);
            chao.mode = 2;
            add_happiness(chao, senses, -5, -0.066_666_67);
            scare(chao, senses, 10);
            upset(chao, rng);
        }
        2 => {
            if chao.body.flags & WALL_PUSHED != 0 && chao.rec.u8(off::HAT) != 0 && senses.hat_room()
            {
                hat_off(chao, rng);
            }
            if chao.body.on_ground() {
                stunned(chao, rng);
            }
        }
        3 => {
            chao.count_a -= 1;
            if chao.count_a <= 0 {
                return Next::Pop;
            }
        }
        _ => {}
    }
    Next::Keep
}

/// Down on the ground (Status `D+4 & 1`): sit stunned (0x9F) 180..300 frames.
fn stunned(chao: &mut Chao, rng: &mut Rng) {
    chao.play_blend(0x9F, 15);
    chao.count_a = 180 + rng.below(121);
    chao.emotion_ball(5, 150);
    chao.mode += 1;
}

/// `0x563EB0`: hit: it flies (0x3E spinning, or 0x10F) upset (happiness −30, fear, emotions
/// up), then lies stunned once down. From the spin, landing passes through mode 2, which
/// lands again the next frame.
fn knocked(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            face_flight(chao);
            chao.set_eyes(1, -1);
            chao.emotion_ball(5, 150);
            if rng.r() < 0.5 {
                chao.play_blend(0x3E, 15);
                chao.mode = 1;
            } else {
                chao.play_blend(0x10F, 15);
                chao.mode = 2;
            }
            add_happiness(chao, senses, -30, -0.1);
            scare(chao, senses, 50);
            upset(chao, rng);
        }
        // Mode 1 spins (`D+0x50 += 0x400`) until down: the clients'.
        1 | 2 => {
            if chao.body.on_ground() {
                stunned(chao, rng);
            }
        }
        3 => {
            chao.count_a -= 1;
            if chao.count_a <= 0 {
                return Next::Pop;
            }
        }
        _ => {}
    }
    Next::Keep
}

/// `0x5648A0`: petted: settle into it (by posture), happiness +1 when `0x132` is under 10;
/// then a pleased motion, or deep sleep if sleepy enough. While the hand stays, `0x132` +1 a
/// frame (from the 4th); 10 motions on, it winds down (12 frames). Once the hand goes, `0x132`
/// −1 a frame (from the 16th) until under 10.
fn petted(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            let id = match chao.posture() {
                1 | 3 => 0x3E,
                2 => 0x2A,
                _ => 0,
            };
            chao.play_blend(id, 20);
            if chao.rec.u8(0x132) < 10 {
                add_happiness(chao, senses, 1, 0.05);
            }
            chao.mode = 1;
        }
        1 => {
            if !chao.motion_done() {
                return Next::Keep;
            }
            if chao.rec.u16(off::SLPDPTH) > 500 {
                chao.set(Behaviour::Game(0x54FCF0), -1);
                return Next::Keep;
            }
            let half = rng.r() < 0.5;
            let id = match (chao.posture(), half) {
                (1..=2, true) => 0x41,
                (1..=2, false) => 0x42,
                (_, true) => 0xFA,
                (_, false) => 0xFC,
            };
            chao.play_blend(id, 20);
            chao.mode = 2;
            chao.count_a = 10;
            if rng.r() < 0.3 {
                chao.sound(if rng.r() < 0.5 { 0x6074 } else { 0x6075 });
            }
        }
        2 => {
            if chao.motion_done() {
                chao.count_a -= 1;
                if chao.count_a <= 0 {
                    chao.mode = 3;
                }
            }
            if chao.rec.u8(0x132) > 100 {
                chao.set_eyes(3, -1);
                chao.set_mouth(3, -1);
            }
            let old = chao.count_b;
            chao.count_b += 1;
            if old >= 3 {
                chao.rec.add_emotion(0x132, 1);
            }
            if !senses.petted() {
                match chao.posture() {
                    0 => chao.play_blend(1, 20),
                    1 => chao.play_blend(0xC, 20),
                    _ => {}
                }
                chao.mode = 4;
                chao.count_a = 0;
                chao.count_b = 0;
            }
        }
        3 => {
            let old = chao.count_a;
            chao.count_a += 1;
            if old > 10 {
                return Next::Pop;
            }
        }
        4 => {
            let old = chao.count_b;
            chao.count_b += 1;
            if old >= 15 {
                chao.rec.add_emotion(0x132, -1);
            }
            if chao.rec.u8(0x132) < 10 {
                return Next::Pop;
            }
        }
        _ => {}
    }
    Next::Keep
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, Tri, V3};
    use super::super::super::physics::ON_GROUND;
    use super::super::super::picker::{Ball, Carried};
    use super::super::super::record::{Record, TYPE_CHILD};
    use super::*;

    /// A flat floor at y = 0, all land.
    fn lawn() -> Garden {
        let mut tris = Vec::new();
        for i in -5..5 {
            for j in -5..5 {
                let (x, z) = (i as f32 * 20.0, j as f32 * 20.0);
                tris.push(Tri {
                    v: [[x, 0.0, z], [x + 20.0, 0.0, z], [x, 0.0, z + 20.0]],
                    flags: 1,
                });
                tris.push(Tri {
                    v: [
                        [x + 20.0, 0.0, z],
                        [x + 20.0, 0.0, z + 20.0],
                        [x, 0.0, z + 20.0],
                    ],
                    flags: 1,
                });
            }
        }
        Garden {
            tris,
            grid: vec![1; GRID_CELLS],
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    /// Player 1 (Shadow, bond 1) and what's going on around the Chao.
    #[derive(Default)]
    struct World {
        player: Option<V3>,
        whistling: bool,
        ball: Option<Ball>,
        touching: Vec<u8>,
        petted: bool,
        hit_by: Option<V3>,
        carrying: Option<Carried>,
        release: Option<Release>,
        hat_room: bool,
    }

    impl Senses for World {
        fn player_pos(&self) -> Option<V3> {
            self.player
        }
        fn player_character(&self) -> Option<u8> {
            self.player.map(|_| 1)
        }
        fn player_rot_y(&self) -> Option<u16> {
            self.player.map(|_| 0x1000)
        }
        fn whistling(&self) -> bool {
            self.whistling
        }
        fn ball(&self) -> Option<Ball> {
            self.ball
        }
        fn touch(&self, kind: u8) -> Option<V3> {
            self.touching.contains(&kind).then_some([0.0, 0.0, 3.0])
        }
        fn petted(&self) -> bool {
            self.petted
        }
        fn hit_by(&self) -> Option<V3> {
            self.hit_by
        }
        fn carrying(&self) -> Option<Carried> {
            self.carrying
        }
        fn release(&self) -> Release {
            self.release.unwrap_or(Release::PutDown)
        }
        fn hat_room(&self) -> bool {
            self.hat_room
        }
    }

    const BOND: usize = off::BONDS + 6;

    fn chao() -> Chao {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, TYPE_CHILD);
        let mut chao = Chao::new([0.0; 3], 0, rec);
        chao.body.flags |= ON_GROUND;
        chao
    }

    fn step(chao: &mut Chao, world: &World, rng: &mut Rng) -> Option<u32> {
        chao.frame(&lawn(), 0, world, rng);
        chao.current().map(|b| b.address())
    }

    #[test]
    fn a_whistle_brings_a_fond_chao_over() {
        let mut rng = Rng::new(5);
        let mut chao = chao();
        chao.rec.set_u8(BOND, 60);
        let mut world = World {
            player: Some([0.0, 0.0, 40.0]),
            whistling: true,
            ..World::default()
        };
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x5634D0));
        world.whistling = false;
        let mut seen = vec![0x5634D0];
        for _ in 0..2000 {
            let now = step(&mut chao, &world, &mut rng).unwrap();
            if seen.last() != Some(&now) {
                seen.push(now);
            }
            if now == 0x565BE0 {
                break;
            }
        }
        // Whistle, walk over (maybe tripping), then stand about once within 8 units.
        assert_eq!(seen[1], 0x563250, "{seen:x?}");
        assert_eq!(seen.last(), Some(&0x565BE0), "{seen:x?}");
        let d = distance2(chao.body.pos, [0.0, 0.0, 40.0]);
        assert!(d < 100.0, "{:?}", chao.body.pos);
    }

    #[test]
    fn a_whistle_out_of_range_or_from_a_stranger_does_nothing() {
        let mut rng = Rng::new(5);
        let mut chao = chao();
        let world = World {
            player: Some([0.0, 0.0, 40.0]),
            whistling: true,
            ..World::default()
        };
        // Heard but not liked: counts as fired, installs nothing.
        assert!(checks(&mut chao, &lawn(), &world, &mut rng, false, false));
        assert_eq!(chao.current(), Some(Behaviour::Settle));
        chao.rec.set_u8(BOND, 60);
        let far = World {
            player: Some([0.0, 0.0, 100.0]),
            whistling: true,
            ..World::default()
        };
        assert!(!checks(&mut chao, &lawn(), &far, &mut rng, false, false));
        // Asleep, it doesn't hear.
        assert!(!checks(&mut chao, &lawn(), &world, &mut rng, false, true));
    }

    #[test]
    fn a_bored_chao_chases_the_ball_until_it_is_picked_up() {
        let mut rng = Rng::new(9);
        let mut chao = chao();
        chao.rec.set_u16(off::TEDIOUS, 7000);
        // No need tick due: the chase's latch stays shut.
        chao.rec.set_u16(off::NEED_CLOCK, 1);
        let mut world = World {
            ball: Some(Ball {
                pos: [0.0, 0.0, 30.0],
                moving: true,
                held: false,
                in_water: false,
            }),
            ..World::default()
        };
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x563830));
        assert_eq!(chao.rec.u16(off::TEDIOUS), 5000);
        // Still rolling: it restarts the chase every frame, −2000 each time, while over 4000.
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x563830));
        assert_eq!(chao.rec.u16(off::TEDIOUS), 3000);
        step(&mut chao, &world, &mut rng);
        assert_eq!(chao.rec.u16(off::TEDIOUS), 3000);
        world.ball.as_mut().unwrap().moving = false;
        for _ in 0..30 {
            step(&mut chao, &world, &mut rng);
        }
        assert_eq!(chao.body.target, [0.0, 0.0, 30.0 * 1.02]);
        world.ball.as_mut().unwrap().held = true;
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x565BE0));
    }

    #[test]
    fn the_chase_ends_in_a_sulk_when_the_ball_splashes() {
        let mut rng = Rng::new(9);
        let mut chao = chao();
        chao.rec.set_u16(off::TEDIOUS, 5000);
        chao.set(Behaviour::Game(0x563830), -1);
        let world = World {
            ball: Some(Ball {
                pos: [0.0, 0.0, 30.0],
                moving: false,
                held: false,
                in_water: true,
            }),
            ..World::default()
        };
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x565BE0));
        assert_eq!(chao.bubble.0, 5);
    }

    #[test]
    fn a_touched_hat_is_taken_then_tossed_or_worn() {
        for wears in [false, true] {
            let mut rng = Rng::new(2);
            let mut chao = chao();
            if wears {
                chao.rec.set_u32(0x118, 0x10000);
            }
            let mut world = World {
                touching: vec![0x93],
                ..World::default()
            };
            // Facing it already, it takes it the same frame.
            assert_eq!(step(&mut chao, &world, &mut rng), Some(0x569340));
            assert_eq!(chao.items, vec![ItemEvent::Take(0x93)]);
            let next = if wears { 0x569430 } else { 0x569550 };
            assert_eq!(chao.stack[1], Some(Behaviour::Game(next)));
            chao.items.clear();
            world.touching.clear();
            world.carrying = Some(Carried {
                touch: 0x93,
                kind: 7,
            });
            let mut frames = 0;
            while chao.current() == Some(Behaviour::Game(0x569340)) {
                step(&mut chao, &world, &mut rng);
                frames += 1;
            }
            assert!((121..=182).contains(&frames), "{frames}");
            assert_eq!(chao.current(), Some(Behaviour::Game(next)));
            for _ in 0..400 {
                step(&mut chao, &world, &mut rng);
                if !chao.items.is_empty() {
                    break;
                }
            }
            if wears {
                assert_eq!(chao.items, vec![ItemEvent::Wear]);
                assert_eq!(chao.rec.u8(off::HAT), 7);
            } else {
                assert!(matches!(chao.items[..], [ItemEvent::Throw(_)]));
                assert_eq!(chao.current(), Some(Behaviour::Settle));
            }
        }
    }

    #[test]
    fn a_missed_grab_gives_up() {
        let mut rng = Rng::new(2);
        let mut chao = chao();
        chao.set(Behaviour::Game(0x569240), -1);
        let world = World::default();
        let mut frames = 0;
        while chao.current() == Some(Behaviour::Game(0x569240)) {
            step(&mut chao, &world, &mut rng);
            frames += 1;
        }
        assert_eq!(frames, 61);
        // The animal grab gives up at once.
        chao.set(Behaviour::Game(0x566C40), -1);
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x565BE0));
    }

    #[test]
    fn seeds_need_run_points_and_empty_hands() {
        let mut rng = Rng::new(2);
        let mut chao = chao();
        let mut world = World {
            touching: vec![0x85],
            ..World::default()
        };
        assert!(!checks(&mut chao, &lawn(), &world, &mut rng, false, false));
        chao.rec.set_u16(off::RUN_POINTS, 50);
        world.carrying = Some(Carried {
            touch: 0x83,
            kind: 0,
        });
        assert!(!checks(&mut chao, &lawn(), &world, &mut rng, false, false));
        world.carrying = None;
        assert!(checks(&mut chao, &lawn(), &world, &mut rng, false, false));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x567640)));
    }

    #[test]
    fn an_a2_fright_lasts_500_frames() {
        let mut rng = Rng::new(2);
        let mut chao = chao();
        let mut world = World {
            touching: vec![0xA2],
            ..World::default()
        };
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x5647B0));
        assert_eq!((chao.eyes, chao.mouth), (1, 8));
        world.touching.clear();
        let mut frames = 1;
        while chao.current() == Some(Behaviour::Game(0x5647B0)) {
            step(&mut chao, &world, &mut rng);
            frames += 1;
        }
        assert_eq!(frames, 500);
    }

    #[test]
    fn being_held_by_a_friend_then_put_down() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        chao.rec.set_u8(BOND, 80);
        chao.held = true;
        let mut world = World {
            player: Some([0.0, 0.0, 5.0]),
            ..World::default()
        };
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x5660A0));
        // Happiness +1 (`0x132` was under 10), then `0x132` pinned at 200.
        assert_eq!(chao.rec.i16(off::HAPPINESS), 1);
        assert_eq!(chao.rec.u8(0x132), 200);
        assert_eq!(chao.rec.i8(BOND), 81);
        for _ in 0..100 {
            assert_eq!(step(&mut chao, &world, &mut rng), Some(0x5660A0));
            assert_eq!((chao.body.vel[0], chao.body.vel[2]), (0.0, 0.0));
        }
        // It faced the holder, so it turns to face away, a quarter turn off their facing.
        assert_eq!(chao.body.rot_y, 0xC000 - 0x1000);
        chao.held = false;
        world.release = Some(Release::PutDown);
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x565BE0));
    }

    #[test]
    fn hunger_sours_a_held_chao_until_it_wriggles_free() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        chao.area = 1;
        chao.held = true;
        chao.rec.set_u16(off::HUNGER, 10_000);
        let world = World::default();
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x5662B0));
        let mut seen = vec![0x5662B0];
        for _ in 0..1300 {
            let now = step(&mut chao, &world, &mut rng).unwrap();
            if seen.last() != Some(&now) {
                seen.push(now);
            }
        }
        assert_eq!(&seen[..3], &[0x5662B0, 0x566300, 0x565BE0], "{seen:x?}");
        assert!(!chao.held);
    }

    #[test]
    fn a_sleepy_chao_dozes_in_the_hands() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        chao.held = true;
        chao.rec.set_u16(off::SLPDPTH, 600);
        let world = World::default();
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x566580));
        assert_eq!(chao.rec.u16(off::SLPDPTH), 600);
        let mut frames = 1;
        while chao.current() == Some(Behaviour::Game(0x566580)) {
            step(&mut chao, &world, &mut rng);
            frames += 1;
        }
        // 300 frames of −2; then calm in the hands again.
        assert_eq!(frames, 301);
        assert_eq!(chao.current(), Some(Behaviour::Settle));
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x5662B0));
    }

    #[test]
    fn a_dropped_chao_is_upset_and_sits_stunned() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        chao.held = true;
        let mut world = World {
            player: Some([0.0, 0.0, 5.0]),
            ..World::default()
        };
        step(&mut chao, &world, &mut rng);
        chao.held = false;
        world.release = Some(Release::Dropped);
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x564320));
        chao.body.pos[1] = 5.0;
        chao.body.flags &= !ON_GROUND;
        step(&mut chao, &world, &mut rng);
        assert_eq!(chao.rec.i16(off::HAPPINESS), 1 - 5);
        // Fear up by 10·2: 20.
        assert_eq!(chao.rec.u8(BOND + 1), 20);
        for at in 0x12D..=0x131 {
            assert!((50..150).contains(&chao.rec.u8(at)));
        }
        let mut frames = 0;
        while chao.current() == Some(Behaviour::Game(0x564320)) {
            step(&mut chao, &world, &mut rng);
            frames += 1;
        }
        assert!((180..400).contains(&frames), "{frames}");
    }

    #[test]
    fn let_go_without_control_it_hops_down_and_stands() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        chao.held = true;
        let mut world = World::default();
        step(&mut chao, &world, &mut rng);
        chao.held = false;
        world.release = Some(Release::NoControl);
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x53D720));
        for _ in 0..100 {
            assert_eq!(step(&mut chao, &world, &mut rng), Some(0x53D720));
        }
    }

    #[test]
    fn petting_warms_it_until_the_hand_goes() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        let mut world = World {
            player: Some([0.0, 0.0, 5.0]),
            petted: true,
            ..World::default()
        };
        assert_eq!(step(&mut chao, &world, &mut rng), Some(0x5648A0));
        assert_eq!(chao.rec.i16(off::HAPPINESS), 1);
        while chao.mode != 2 {
            step(&mut chao, &world, &mut rng);
        }
        for _ in 0..40 {
            step(&mut chao, &world, &mut rng);
        }
        assert!(chao.rec.u8(0x132) > 30);
        assert_eq!((chao.eyes, chao.mouth), (0, 0));
        world.petted = false;
        step(&mut chao, &world, &mut rng);
        assert_eq!(chao.mode, 4);
        let warm = usize::from(chao.rec.u8(0x132));
        let mut frames = 0;
        while chao.current() == Some(Behaviour::Game(0x5648A0)) {
            step(&mut chao, &world, &mut rng);
            frames += 1;
        }
        // 15 frames before it cools, then −1 a frame to 9.
        assert_eq!(frames, 15 + warm - 9);
    }

    #[test]
    fn a_hit_knocks_it_away_and_off_comes_the_hat() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        chao.rec.set_u8(off::HAT, 3);
        let world = World {
            player: Some([0.0, 0.0, 5.0]),
            hit_by: Some([0.0, 0.0, 5.0]),
            hat_room: true,
            ..World::default()
        };
        assert!(checks(&mut chao, &lawn(), &world, &mut rng, false, false));
        assert_eq!(chao.body.vel, [0.0, 1.5, -0.8]);
        assert_eq!(chao.current(), Some(Behaviour::Game(0x563EB0)));
        assert_eq!(chao.rec.u8(off::HAT), 0);
        assert!(matches!(
            chao.items[..],
            [ItemEvent::HatOff { kind: 3, .. }]
        ));
        let calm = World::default();
        step(&mut chao, &world, &mut rng);
        // Facing along the flight (−z).
        assert_eq!(chao.body.rot_y, 0x8000);
        assert_eq!(chao.rec.i16(off::HAPPINESS), -30);
        assert_eq!(chao.rec.i8(BOND), -30);
        assert_eq!(chao.rec.u8(BOND + 1), 100);
        let mut frames = 0;
        while chao.current() == Some(Behaviour::Game(0x563EB0)) {
            step(&mut chao, &calm, &mut rng);
            frames += 1;
        }
        assert!((180..450).contains(&frames), "{frames}");
    }

    #[test]
    fn the_last_check_to_fire_wins() {
        let mut rng = Rng::new(4);
        let mut chao = chao();
        chao.rec.set_u8(BOND, 60);
        chao.held = true;
        let world = World {
            player: Some([0.0, 0.0, 5.0]),
            whistling: true,
            touching: vec![0x93],
            carrying: Some(Carried {
                touch: 0x83,
                kind: 0,
            }),
            ..World::default()
        };
        assert!(checks(&mut chao, &lawn(), &world, &mut rng, false, false));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x566670)));
        // Whatever it held, it lets go.
        assert_eq!(chao.items, vec![ItemEvent::LetGo]);
        let hit = World {
            hit_by: Some([0.0, 0.0, 5.0]),
            petted: true,
            ..World::default()
        };
        chao.held = false;
        chao.set(Behaviour::Settle, -1);
        assert!(checks(&mut chao, &lawn(), &hit, &mut rng, false, false));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x563EB0)));
        // In the water only the hands count.
        chao.set(Behaviour::Settle, -1);
        assert!(checks(&mut chao, &lawn(), &world, &mut rng, true, false));
        chao.held = false;
        chao.set(Behaviour::Settle, -1);
        assert!(checks(&mut chao, &lawn(), &world, &mut rng, true, false));
        assert_eq!(chao.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn kindness_eases_fear_and_ticks_the_bond() {
        let mut chao = chao();
        let world = World {
            player: Some([0.0; 3]),
            ..World::default()
        };
        chao.rec.set_u8(BOND + 1, 50);
        chao.rec.set_u16(BOND + 2, 300);
        add_happiness(&mut chao, &world, 1, 0.05);
        assert_eq!(chao.rec.u8(BOND + 1), 30);
        assert_eq!(chao.rec.u16(BOND + 2), 200);
        assert_eq!(chao.rec.u16(BOND + 4), 1);
        // Shadow takes alignment the other way.
        assert_eq!(chao.rec.f32(off::ALIGNMENT), -0.05);
    }

    #[test]
    fn approach_turns_the_short_way() {
        assert_eq!(approach(0, 0x4000, 0x800), 0x800);
        assert_eq!(approach(0, 0xC000, 0x800), 0xF800);
        assert_eq!(approach(0x3C00, 0x4000, 0x800), 0x4000);
    }
}

//! What a Chao does when an emotion byte runs high, when it's lonely for the player, and a
//! dark adult's mischief (re/chao-ai.md §2B "Moods and emotions", "Dark-adult mischief and
//! lonely", §3.3). Decoded from the game's bytes; the emotion bytes go by offset, as their
//! names aren't confirmed. Motion blend lengths, motion speeds and the tear effects are the
//! clients' business and aren't modelled.

use super::super::brain::{Behaviour, Chao, Next, Rng, random_cell_point};
use super::super::land::Garden;
use super::super::physics::FACING_WALL;
use super::super::picker::Senses;
use super::super::record::off;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    Some(match address {
        0x59F350 => scared(chao, rng),
        0x59F5F0 => flee(chao, garden, senses, rng),
        0x59F8E0 => sad(chao, rng),
        0x59F970 => sulk(chao, rng),
        0x59FA50 => sob(chao, rng),
        0x59FCA0 => cry(chao, rng),
        0x5A00A0 => angry(chao, rng),
        0x5A0BA0 => happy(chao, rng),
        0x5A03B0 => call_out(chao),
        0x5A0490 => bond_tick(chao, senses),
        0x5A0960 => run_to_player(chao, senses),
        0x5A0820 => near_player(chao, senses),
        0x5A1800 => dislike(chao),
        0x5A1460 => laugh(chao, rng),
        0x5A18A0 => pester(chao, senses, rng),
        0x5A1600 => taunt(chao, 0x1B9),
        0x5A1680 => taunt(chao, 0x1C3),
        0x5A1700 => taunt(chao, 0x1BB),
        0x5A1780 => taunt(chao, 0x1BD),
        _ => return None,
    })
}

/// The mouth `0x5A1460` and friends test (`D+0x69E == 1`): 9 then, else 4.
fn mischief_mouth(chao: &mut Chao) {
    let id = if chao.mouth == 1 { 9 } else { 4 };
    chao.set_mouth(id, -1);
}

/// Squared 3D distance to player 1 (the lonely and mischief tests); no player is far away.
fn player_distance2(chao: &Chao, senses: &dyn Senses) -> f32 {
    senses.player_pos().map_or(f32::INFINITY, |p| {
        let d = [
            p[0] - chao.body.pos[0],
            p[1] - chao.body.pos[1],
            p[2] - chao.body.pos[2],
        ];
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    })
}

/// `0x796AB0(player 0, rate)`: turn toward player 1, as `0x796910` does toward the target.
fn turn_to_player(chao: &mut Chao, senses: &dyn Senses, rate: u16) {
    if let Some(p) = senses.player_pos() {
        let target = chao.body.target;
        chao.body.target = p;
        chao.body.turn_to_target(rate);
        chao.body.target = target;
    }
}

/// `0x59F350` (emotion `0x12F`, picker `0x59F210`): cower 180..480 frames, whimpering.
fn scared(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(if rng.r() < 0.5 { 0x1F1 } else { 0x1F2 });
            chao.mode += 1;
            chao.set_eyes(1, -1);
            chao.set_mouth(8, -1);
            chao.count_a = 180 + rng.below(301);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.count_a % 150 == 0 && rng.r() < 0.5 {
        chao.sound(0x602D);
    }
    chao.count_a -= 1;
    if chao.count_a != 0 {
        Next::Keep
    } else {
        Next::Pop
    }
}

/// `0x59F5F0`: queued after `0x59F350` when the player is seen. Run 30 units from the player
/// (`0x59F490`) and again while still within 50 of them, for up to 1000 frames.
fn flee(chao: &mut Chao, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(if rng.r() < 0.5 { 0x76 } else { 0x6A });
        chao.set_eyes(1, -1);
        chao.set_mouth(8, -1);
        // `0x437590`, a positional sound.
        chao.sound(0x602E);
        flee_target(chao, garden, senses, rng);
        chao.mode += 1;
        chao.count_a = 0;
        chao.count_b = 0;
    }
    if chao.rec.need_latch() {
        chao.rec.add_need(off::TIRE, 900);
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old > 1000 {
        return Next::Pop;
    }
    if chao.count_a % 100 == 0 && rng.r() < 0.03 {
        // Trips; the stumble resumes the flight after (`0x53DA00`). This frame carries on.
        chao.set(Behaviour::Stumble, -1);
    }
    if chao.body.target_distance2() < 64.0 {
        if player_distance2(chao, senses) >= 2500.0 {
            return Next::Pop;
        }
        chao.mode = 0;
    }
    if chao.body.flags & FACING_WALL != 0 {
        chao.mode = 0;
    }
    chao.body.turn_to_target(0x120);
    chao.body.drive(0.08);
    Next::Keep
}

/// `0x59F490`: 30 units straight away from the player, or square to that if it would leave
/// the cell value (zone, `0x535010`) the Chao stands in, or else a random point of the zone
/// (`0x534F80` on its list). Outside any zone, a land point.
fn flee_target(chao: &mut Chao, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) {
    let zone_at = |x: f32, z: f32| garden.cell_at(x, z).unwrap_or(0);
    let pos = chao.body.pos;
    let zone = zone_at(pos[0], pos[2]);
    let mut target = chao.body.target;
    if zone == 0 {
        if let Some((x, z)) = random_cell_point(garden, 1, rng) {
            (target[0], target[2]) = (x, z);
        }
    } else {
        // The game always has player 1 here; without one, straight to the zone's list.
        let away = senses.player_pos().map(|p| {
            let (dx, dz) = (pos[0] - p[0], pos[2] - p[2]);
            let len = (dx * dx + dz * dz).sqrt();
            if len > 0.0 {
                (30.0 * dx / len, 30.0 * dz / len)
            } else {
                (0.0, 0.0)
            }
        });
        let mut found = false;
        if let Some((ax, az)) = away {
            target = [pos[0] + ax, pos[1], pos[2] + az];
            found = zone_at(target[0], target[2]) == zone;
            if !found {
                target = if rng.r() < 0.5 {
                    [pos[0] + az, pos[1], pos[2] - ax]
                } else {
                    [pos[0] - az, pos[1], pos[2] + ax]
                };
                found = zone_at(target[0], target[2]) == zone;
            }
        }
        if !found && let Some((x, z)) = random_cell_point(garden, zone, rng) {
            (target[0], target[2]) = (x, z);
        }
    }
    chao.body.target = target;
}

/// `0x59F8E0` (emotion `0x12E`, picker `0x59F860`): above 120 cry (`0x59FA50` or
/// `0x59FCA0`), else `0x59FA50` or sulk (`0x59F970`), half and half.
fn sad(chao: &mut Chao, rng: &mut Rng) -> Next {
    let next = if chao.rec.u8(0x12E) > 120 {
        if rng.r() < 0.5 { 0x59FA50 } else { 0x59FCA0 }
    } else if rng.r() < 0.5 {
        0x59FA50
    } else {
        0x59F970
    };
    chao.set(Behaviour::Game(next), -1);
    Next::Keep
}

/// `0x59F970`: sulk 180..300 frames.
fn sulk(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(if (1..=3).contains(&chao.posture()) {
                0x1F9
            } else {
                0x1F6
            });
            chao.set_eyes(3, -1);
            chao.set_mouth(4, -1);
            chao.count_a = 180 + rng.below(121);
            chao.mode += 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a -= 1;
    if old > 0 { Next::Keep } else { Next::Pop }
}

/// `0x59FA50`: sob 100..200 frames a time, `0x12E` −10 each, until it's below 10.
fn sob(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            let r = rng.r();
            let sitting = (1..=3).contains(&chao.posture());
            chao.play(match (sitting, r) {
                (true, r) if r < 0.3 => 0x225,
                (true, r) if r < 0.6 => 0x227,
                (true, r) if r < 0.8 => 0x228,
                (true, _) => 0x229,
                (false, r) if r < 0.3 => 0x220,
                (false, r) if r < 0.6 => 0x222,
                (false, r) if r < 0.8 => 0x223,
                (false, _) => 0x224,
            });
            chao.set_eyes(if rng.r() < 0.5 { 3 } else { 1 }, -1);
            chao.set_mouth(if rng.r() < 0.5 { 4 } else { 6 }, -1);
            chao.count_a = 100 + rng.below(101);
            chao.mode += 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a -= 1;
    if old > 0 {
        return Next::Keep;
    }
    chao.rec.add_emotion(0x12E, -10);
    if chao.rec.u8(0x12E) < 10 {
        return Next::Pop;
    }
    chao.mode = 0;
    Next::Keep
}

/// `0x59FCA0`: bawl a motion or 100..200 frames a time, `0x12E` −10 each, until it's below
/// 10; tears (`0x6ED270`, clients) and now and then a wail.
fn cry(chao: &mut Chao, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        let r = rng.r();
        chao.play(if (1..=3).contains(&chao.posture()) {
            match r {
                r if r < 0.4 => 0x210,
                r if r < 0.8 => 0x213,
                _ => 0x215,
            }
        } else if r < 0.5 {
            0x20B
        } else {
            0x20D
        });
        chao.set_eyes(if rng.r() < 0.5 { 3 } else { 1 }, -1);
        chao.set_mouth(if rng.r() < 0.4 { 4 } else { 6 }, -1);
        chao.count_a = 100 + rng.below(101);
        chao.mode += 1;
    }
    if chao.mode == 1 {
        let over = chao.motion_held() || {
            let old = chao.count_a;
            chao.count_a -= 1;
            old <= 0
        };
        if over {
            chao.rec.add_emotion(0x12E, -10);
            if chao.rec.u8(0x12E) < 10 {
                return Next::Pop;
            }
            chao.mode = 0;
        }
    }
    if chao.frame.is_multiple_of(180) && rng.r() < 0.3 {
        chao.sound(0x6001);
    }
    Next::Keep
}

/// `0x5A00A0` (emotion `0x131`, picker `0x5A0010`): a tantrum (over 100) costing 10, then
/// up to five stamps costing 20 each, until it's below 10.
fn angry(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                if chao.rec.u8(0x131) > 100 {
                    chao.play(0x230);
                    if rng.r() < 0.5 {
                        chao.set_eyes(6, -1);
                    }
                    chao.set_mouth(if rng.r() < 0.5 { 8 } else { 4 }, -1);
                    chao.count_a = 240 + rng.below(181);
                    chao.mode += 1;
                } else {
                    chao.mode = 2;
                }
            }
            let old = chao.count_a;
            chao.count_a -= 1;
            if old <= 0 {
                chao.rec.add_emotion(0x131, -10);
                chao.mode = 2;
            }
        }
        2 => {
            let r = rng.r();
            chao.play(match r {
                r if r < 0.33 => 0x232,
                r if r < 0.66 => 0x233,
                _ => 0x234,
            });
            chao.set_eyes(if rng.r() < 0.5 { 3 } else { 1 }, -1);
            chao.set_mouth(4, -1);
            chao.mode += 1;
            chao.count_a = 120 + rng.below(201);
        }
        3 => {
            let old = chao.count_a;
            chao.count_a -= 1;
            if old <= 0 {
                let old = chao.count_b;
                chao.count_b += 1;
                if old > 4 {
                    return Next::Pop;
                }
                chao.rec.add_emotion(0x131, -20);
                if chao.rec.u8(0x131) < 10 {
                    return Next::Pop;
                }
                chao.mode = 2;
            }
        }
        _ => {}
    }
    Next::Keep
}

/// `0x5A0BA0` (emotion `0x12C`, picker `0x5A0B10`): up to nine bursts of joy, each costing
/// 20, until it's below 10.
fn happy(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                let r = rng.r();
                let (id, mode) = match r {
                    r if r < 0.2 => (0x1D4, 1),
                    r if r < 0.6 => (0x1CF, 3),
                    _ => (0x1EC, 4),
                };
                chao.play(id);
                chao.mode = mode;
                chao.set_eyes(if rng.r() < 0.5 { 1 } else { 4 }, -1);
                let r = rng.r();
                chao.set_mouth(
                    match r {
                        r if r < 0.4 => 3,
                        r if r < 0.8 => 5,
                        _ => 2,
                    },
                    -1,
                );
                if rng.r() < 0.7 {
                    let r = rng.r();
                    chao.sound(match r {
                        r if r < 0.2 => 0x602F,
                        r if r < 0.4 => 0x6030,
                        r if r < 0.6 => 0x6031,
                        r if r < 0.8 => 0x6032,
                        _ => 0x6033,
                    });
                }
                chao.count_a = 120 + rng.below(61);
            }
            // Mode 0 runs on into mode 1's countdown, whatever mode it chose.
            chao.count_a -= 1;
            if chao.count_a <= 0 {
                chao.play(0x1D6);
                chao.mode = 2;
            }
        }
        2 => {
            if chao.motion_done() {
                chao.mode = 5;
            }
        }
        3 => {
            chao.count_a -= 1;
            if chao.count_a <= 0 {
                chao.mode = 5;
            }
        }
        4 => {
            chao.count_a -= 1;
            if chao.count_a <= 0 && chao.motion_done() {
                chao.mode = 5;
            }
        }
        5 => {
            let old = chao.count_b;
            chao.count_b += 1;
            if old > 8 {
                return Next::Pop;
            }
            chao.rec.add_emotion(0x12C, -20);
            if chao.rec.u8(0x12C) < 10 {
                return Next::Pop;
            }
            chao.mode = 0;
        }
        _ => {}
    }
    Next::Keep
}

/// `0x5A03B0` (lonely, picker `0x5A04A0`): call out to the player for 200 frames, cheerily
/// when its alignment is over 0.5.
fn call_out(chao: &mut Chao) -> Next {
    match chao.mode {
        0 => {
            if chao.rec.f32(off::ALIGNMENT) > 0.5 {
                chao.play(0x18C);
                chao.sound(0x6091);
            } else {
                chao.play(0x18D);
            }
            chao.set_eyes(4, -1);
            chao.set_mouth(3, -1);
            chao.mode += 1;
            chao.count_a = 200;
        }
        1 => {}
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a -= 1;
    if old > 0 { Next::Keep } else { Next::Pop }
}

/// `0x5A0490`, queued after `0x5A03B0` and `0x5A1800`: `0x53E280` on the current character's
/// bond (`0x53E0C0`, 0 with no player): `+4` counts up (to 0xFFFF), `+2` becomes 2·⌊v/3⌋.
fn bond_tick(chao: &mut Chao, senses: &dyn Senses) -> Next {
    // `0x53E0C0` reads the player's character whether the Chao sees them or not.
    let bond = senses
        .player_character()
        .filter(|c| (1..=5).contains(c))
        .unwrap_or(0);
    let at = off::BONDS + usize::from(bond) * 6;
    let count = chao.rec.u16(at + 4);
    if count < 0xFFFF {
        chao.rec.set_u16(at + 4, count + 1);
    }
    let v = chao.rec.u16(at + 2);
    chao.rec.set_u16(at + 2, v / 3 * 2);
    Next::Pop
}

/// `0x5A0960` (lonely, a liked player): run to player 1 for up to 1000 frames; within 5
/// units, `0x12C` +10 and stay (`0x5A0820`). Already glad enough (`0x12C` > 180): LONELY
/// −6000 and done.
fn run_to_player(chao: &mut Chao, senses: &dyn Senses) -> Next {
    if chao.mode == 0 {
        chao.play(0x75);
        chao.set_eyes(4, -1);
        chao.set_mouth(3, -1);
        chao.mode += 1;
        chao.count_a = 0;
        chao.count_b = 0;
        if chao.rec.u8(0x12C) > 180 {
            chao.rec.add_need(off::LONELY, -6000);
            return Next::Pop;
        }
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old > 1000 {
        return Next::Pop;
    }
    if player_distance2(chao, senses) < 25.0 {
        chao.rec.add_emotion(0x12C, 10);
        chao.set(Behaviour::Game(0x5A0820), -1);
        chao.push(Behaviour::Game(0x5A0960));
    }
    turn_to_player(chao, senses, 0x120);
    chao.body.drive(0.055);
    Next::Keep
}

/// `0x5A0820`: by the player. Each time its motion ends: still within 5 units, `0x12C` +10
/// and again (over 140: LONELY −6000 and done); else run back (`0x5A0960`).
fn near_player(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x190);
            chao.set_eyes(3, -1);
            chao.set_mouth(3, -1);
            chao.mode += 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    if !chao.motion_done() {
        return Next::Keep;
    }
    if player_distance2(chao, senses) < 25.0 {
        chao.rec.add_emotion(0x12C, 10);
        chao.set(Behaviour::Game(0x5A0820), -1);
        if chao.rec.u8(0x12C) > 140 {
            chao.rec.add_need(off::LONELY, -6000);
            return Next::Pop;
        }
    } else {
        chao.set(Behaviour::Game(0x5A0960), -1);
    }
    Next::Keep
}

/// `0x5A1800`: turn up its nose at a disliked player, for one motion.
fn dislike(chao: &mut Chao) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x1BF);
            chao.mode += 1;
            chao.set_eyes(1, -1);
            chao.set_mouth(4, -1);
            chao.sound(0x6069);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.motion_done() {
        Next::Pop
    } else {
        Next::Keep
    }
}

/// `0x5A1460` (dark adults, picker `0x5A1240`): an evil laugh for 180..480 frames.
fn laugh(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x203);
            chao.mode += 1;
            chao.set_eyes(10, -1);
            mischief_mouth(chao);
            chao.count_a = 180 + rng.below(301);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.count_a % 150 == 0 && rng.r() < 0.5 {
        let r = rng.r();
        chao.sound(match r {
            r if r < 0.33 => 0x6065,
            r if r < 0.66 => 0x6066,
            _ => 0x6068,
        });
    }
    chao.count_a -= 1;
    if chao.count_a != 0 {
        Next::Keep
    } else {
        Next::Pop
    }
}

/// `0x5A18A0`, queued after `0x5A1460` (and by the lonely picker): go after player 1 for up
/// to 1000 frames. Within 5 units, `0x12D` −10 and a taunt, then on again; further than 10,
/// laugh (`0x5A1460`) instead. Done once `0x12D` is below 10.
fn pester(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(0x7D);
        chao.set_eyes(10, -1);
        mischief_mouth(chao);
        chao.mode += 1;
        chao.count_a = 0;
        chao.count_b = 0;
        if chao.rec.u8(0x12D) < 10 {
            return Next::Pop;
        }
    }
    // The once-per-need-period latch, set with no change of its own.
    let _ = chao.rec.need_latch();
    let old = chao.count_a;
    chao.count_a += 1;
    if old > 1000 {
        return Next::Pop;
    }
    let d2 = player_distance2(chao, senses);
    if d2 < 25.0 {
        chao.rec.add_emotion(0x12D, -10);
        let r = rng.r();
        let taunt = match r {
            r if r < 0.35 => 0x5A1600,
            r if r < 0.7 => 0x5A1780,
            r if r < 0.9 => 0x5A1680,
            _ => 0x5A1700,
        };
        chao.set(Behaviour::Game(taunt), -1);
        chao.push(Behaviour::Game(0x5A18A0));
        if rng.r() < 0.6 {
            let r = rng.r();
            chao.sound(match r {
                r if r < 0.25 => 0x606F,
                r if r < 0.5 => 0x6070,
                r if r < 0.75 => 0x6071,
                _ => 0x6072,
            });
        }
    }
    if d2 > 100.0 {
        chao.set(Behaviour::Game(0x5A1460), -1);
    }
    turn_to_player(chao, senses, 0x120);
    chao.body.drive(0.055);
    Next::Keep
}

/// `0x5A1600` / `0x5A1680` / `0x5A1700` / `0x5A1780`: one taunting motion.
fn taunt(chao: &mut Chao, id: u16) -> Next {
    match chao.mode {
        0 => {
            chao.play(id);
            chao.mode += 1;
            chao.set_eyes(10, -1);
            mischief_mouth(chao);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.motion_done() {
        Next::Pop
    } else {
        Next::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, Tri, V3};
    use super::super::super::picker::Alone;
    use super::super::super::record::Record;
    use super::*;

    /// A flat floor at y = 0 in 20-unit tiles (the probe only reaches triangles near it),
    /// all land.
    fn lawn() -> Garden {
        let mut tris = Vec::new();
        for i in -10..10 {
            for j in -10..10 {
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

    /// Player 1 standing at a point, seen as bond 1.
    struct Player(V3);

    impl Senses for Player {
        fn player_seen(&self) -> Option<usize> {
            Some(1)
        }
        fn player_character(&self) -> Option<u8> {
            Some(1)
        }
        fn player_pos(&self) -> Option<V3> {
            Some(self.0)
        }
    }

    fn chao_doing(address: u32) -> Chao {
        let mut chao = Chao::new([0.0; 3], 0, Record::default());
        chao.set(Behaviour::Game(address), -1);
        chao
    }

    /// Frames until the Chao leaves the behaviours in `during`, or None.
    fn run_until_out(
        chao: &mut Chao,
        during: &[u32],
        senses: &dyn Senses,
        limit: usize,
        rng: &mut Rng,
    ) -> Option<usize> {
        let garden = lawn();
        (1..=limit).find(|_| {
            chao.frame(&garden, 0, senses, rng);
            !chao
                .current()
                .is_some_and(|b| during.contains(&b.address()))
        })
    }

    #[test]
    fn a_scared_chao_cowers_for_180_to_480_frames() {
        for seed in 0..20 {
            let mut rng = Rng::new(seed);
            let mut chao = chao_doing(0x59F350);
            chao.frame(&lawn(), 0, &Alone, &mut rng);
            assert_eq!((chao.eyes, chao.mouth), (1, 8));
            let frames = run_until_out(&mut chao, &[0x59F350], &Alone, 600, &mut rng).unwrap() + 1;
            assert!((181..=481).contains(&frames), "{frames}");
            assert_eq!(chao.current(), Some(Behaviour::Settle));
        }
    }

    #[test]
    fn fleeing_runs_30_units_from_the_player_and_tires() {
        let garden = lawn();
        let mut rng = Rng::new(3);
        let mut chao = chao_doing(0x59F5F0);
        let player = Player([0.0, 0.0, -20.0]);
        chao.frame(&garden, 0, &player, &mut rng);
        let t = chao.body.target;
        assert!(t[0].abs() < 1e-3 && (t[2] - 30.0).abs() < 1e-3, "{t:?}");
        assert_eq!(chao.rec.u16(off::TIRE), 900);
        assert_eq!(chao.sounds, [0x602E]);
        // Arrives 50 units from the player: done.
        let frames = run_until_out(&mut chao, &[0x59F5F0, 0x562D50], &player, 2000, &mut rng);
        assert!(frames.is_some());
        assert!(chao.body.pos[2] > 20.0, "{:?}", chao.body.pos);
    }

    #[test]
    fn the_flight_turns_aside_at_the_edge_of_its_zone() {
        let mut garden = lawn();
        // Only the Chao's own cell column (x 0..10) and the one to its left are zone 1.
        garden.grid = vec![0; GRID_CELLS];
        for row in 0..40 {
            garden.grid[row * 32 + 16] = 1;
            garden.grid[row * 32 + 15] = 1;
        }
        let mut rng = Rng::new(1);
        let mut chao = Chao::new([5.0, 0.0, 0.0], 0, Record::default());
        chao.set(Behaviour::Game(0x59F5F0), -1);
        // The player to its left: straight away (+x) leaves the zone, so it goes ±z.
        chao.frame(&garden, 0, &Player([-5.0, 0.0, 0.0]), &mut rng);
        let t = chao.body.target;
        assert!(
            (t[0] - 5.0).abs() < 1e-3 && (t[2].abs() - 30.0).abs() < 1e-3,
            "{t:?}"
        );
    }

    #[test]
    fn sadness_picks_by_how_sad() {
        let garden = lawn();
        for (value, choices) in [(150u8, [0x59FA50, 0x59FCA0]), (100, [0x59FA50, 0x59F970])] {
            let mut seen = Vec::new();
            for seed in 0..20 {
                let mut rng = Rng::new(seed * 104_729);
                let mut chao = chao_doing(0x59F8E0);
                chao.rec.set_u8(0x12E, value);
                chao.frame(&garden, 0, &Alone, &mut rng);
                let now = chao.current().unwrap().address();
                assert!(choices.contains(&now));
                seen.push(now);
            }
            assert!(choices.iter().all(|c| seen.contains(c)));
        }
    }

    #[test]
    fn sobbing_and_crying_wear_sadness_down_by_tens() {
        for address in [0x59FA50, 0x59FCA0] {
            let mut rng = Rng::new(5);
            let mut chao = chao_doing(address);
            chao.rec.set_u8(0x12E, 35);
            let frames = run_until_out(&mut chao, &[address], &Alone, 2000, &mut rng).unwrap();
            assert_eq!(chao.rec.u8(0x12E), 5);
            // Three cycles: of 101..201 frames (sobbing), or ended by the motion (crying).
            if address == 0x59FA50 {
                assert!((303..=603).contains(&frames), "{frames}");
            } else {
                assert!(frames <= 3 * 63, "{frames}");
            }
        }
    }

    #[test]
    fn sulking_lasts_180_to_300_frames() {
        let mut rng = Rng::new(9);
        let mut chao = chao_doing(0x59F970);
        chao.frame(&lawn(), 0, &Alone, &mut rng);
        assert_eq!((chao.eyes, chao.mouth), (3, 4));
        let frames = run_until_out(&mut chao, &[0x59F970], &Alone, 400, &mut rng).unwrap() + 1;
        assert!((181..=301).contains(&frames), "{frames}");
    }

    #[test]
    fn anger_burns_off_ten_then_twenty_five_times() {
        let mut rng = Rng::new(2);
        let mut chao = chao_doing(0x5A00A0);
        chao.rec.set_u8(0x131, 160);
        run_until_out(&mut chao, &[0x5A00A0], &Alone, 5000, &mut rng).unwrap();
        assert_eq!(chao.rec.u8(0x131), 160 - 10 - 5 * 20);
        // Not over 100: straight to the stamping, after the −10.
        let mut chao = chao_doing(0x5A00A0);
        chao.rec.set_u8(0x131, 25);
        run_until_out(&mut chao, &[0x5A00A0], &Alone, 5000, &mut rng).unwrap();
        assert_eq!(chao.rec.u8(0x131), 0);
    }

    #[test]
    fn joy_lasts_nine_bursts_at_most() {
        let mut rng = Rng::new(4);
        let mut chao = chao_doing(0x5A0BA0);
        chao.rec.set_u8(0x12C, 200);
        run_until_out(&mut chao, &[0x5A0BA0], &Alone, 10_000, &mut rng).unwrap();
        assert_eq!(chao.rec.u8(0x12C), 200 - 9 * 20);
        let mut chao = chao_doing(0x5A0BA0);
        chao.rec.set_u8(0x12C, 105);
        run_until_out(&mut chao, &[0x5A0BA0], &Alone, 10_000, &mut rng).unwrap();
        assert_eq!(chao.rec.u8(0x12C), 5);
    }

    #[test]
    fn calling_out_then_the_bond_tick() {
        let mut rng = Rng::new(1);
        let mut chao = chao_doing(0x5A03B0);
        chao.push(Behaviour::Game(0x5A0490));
        chao.rec.set_f32(off::ALIGNMENT, 1.0);
        let at = off::BONDS + 6;
        chao.rec.set_u16(at + 2, 301);
        chao.rec.set_u16(at + 4, 1);
        let player = Player([50.0, 0.0, 0.0]);
        let frames =
            run_until_out(&mut chao, &[0x5A03B0, 0x5A0490], &player, 400, &mut rng).unwrap();
        assert_eq!(frames, 202);
        assert_eq!(chao.motion, 0x18C);
        assert!(chao.sounds.contains(&0x6091));
        assert_eq!((chao.rec.u16(at + 2), chao.rec.u16(at + 4)), (200, 2));
    }

    #[test]
    fn a_lonely_chao_stays_by_the_player_until_glad() {
        let mut rng = Rng::new(1);
        let mut chao = chao_doing(0x5A0960);
        chao.rec.set_u16(off::LONELY, 8000);
        chao.rec.set_u8(0x12C, 100);
        let player = Player([0.0, 0.0, 2.0]);
        run_until_out(&mut chao, &[0x5A0960, 0x5A0820], &player, 2000, &mut rng).unwrap();
        // +10 on arrival, then +10 per motion until over 140.
        assert_eq!(chao.rec.u8(0x12C), 150);
        assert_eq!(chao.rec.u16(off::LONELY), 2000);
        // Glad already: no run at all.
        let mut chao = chao_doing(0x5A0960);
        chao.rec.set_u16(off::LONELY, 8000);
        chao.rec.set_u8(0x12C, 190);
        let frames = run_until_out(&mut chao, &[0x5A0960], &player, 10, &mut rng);
        assert_eq!((frames, chao.rec.u16(off::LONELY)), (Some(1), 2000));
    }

    #[test]
    fn running_to_a_far_player_closes_in() {
        let garden = lawn();
        let mut rng = Rng::new(1);
        let mut chao = chao_doing(0x5A0960);
        let player = Player([0.0, 0.0, -40.0]);
        for _ in 0..200 {
            chao.frame(&garden, 0, &player, &mut rng);
        }
        assert!(chao.body.pos[2] < -5.0, "{:?}", chao.body.pos);
    }

    #[test]
    fn pestering_taunts_up_close_and_laughs_from_afar() {
        let garden = lawn();
        let mut rng = Rng::new(6);
        let mut chao = chao_doing(0x5A18A0);
        chao.rec.set_u8(0x12D, 100);
        chao.frame(&garden, 0, &Player([1.0, 0.0, 0.0]), &mut rng);
        assert_eq!(chao.rec.u8(0x12D), 90);
        let taunt = chao.current().unwrap().address();
        assert!([0x5A1600, 0x5A1680, 0x5A1700, 0x5A1780].contains(&taunt));
        // The new behaviour starts with the default face (`0x53D760`), its own from its first
        // frame.
        assert_eq!(chao.eyes, 0);
        let frames = run_until_out(&mut chao, &[taunt], &Alone, 100, &mut rng);
        assert_eq!(frames, Some(62));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x5A18A0)));
        // From afar: laugh instead, then done.
        chao.frame(&garden, 0, &Player([30.0, 0.0, 0.0]), &mut rng);
        assert_eq!(chao.current(), Some(Behaviour::Game(0x5A1460)));
        let frames = run_until_out(&mut chao, &[0x5A1460], &Alone, 600, &mut rng).unwrap();
        assert!((181..=481).contains(&frames), "{frames}");
        // Calm enough: done at once.
        let mut chao = chao_doing(0x5A18A0);
        chao.rec.set_u8(0x12D, 5);
        assert_eq!(
            run_until_out(&mut chao, &[0x5A18A0], &Alone, 10, &mut rng),
            Some(1)
        );
    }

    #[test]
    fn dislike_is_one_motion() {
        let mut rng = Rng::new(1);
        let mut chao = chao_doing(0x5A1800);
        let frames = run_until_out(&mut chao, &[0x5A1800], &Alone, 100, &mut rng);
        // The 60-frame stand-in motion, its one-frame link and the pulse a frame late.
        assert_eq!(frames, Some(63));
        assert!(chao.sounds.contains(&0x6069));
        assert_eq!(chao.motion, 0x1BF);
    }
}

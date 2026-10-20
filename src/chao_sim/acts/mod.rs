//! The game's behaviours beyond walking about, a file per group, each named by the game
//! function it follows (re/chao-ai.md §2). `run` finds the group that has a behaviour.

use super::brain::{Chao, Next, Rng};
use super::land::Garden;
use super::picker::Senses;

mod food;
mod moods;
mod play;
mod reach;
mod seeds;
mod sleep;
mod swim;
mod tend;
mod touch;
mod toys;

pub(super) use toys::{join_toy, toy_skill};

/// Runs behaviour `address` for a frame, or None when no group has it yet.
pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    sleep::run(chao, address, garden, senses, rng)
        .or_else(|| swim::run(chao, address, garden, senses, rng))
        .or_else(|| tend::run(chao, address, garden, senses, rng))
        .or_else(|| moods::run(chao, address, garden, senses, rng))
        .or_else(|| touch::run(chao, address, garden, senses, rng))
        .or_else(|| food::run(chao, address, garden, senses, rng))
        .or_else(|| reach::run(chao, address, garden, senses, rng))
        .or_else(|| seeds::run(chao, address, garden, senses, rng))
        .or_else(|| toys::run(chao, address, garden, senses, rng))
        .or_else(|| play::run(chao, address, garden, senses, rng))
}

/// Cocoon and death: no interrupts at all.
const UNINTERRUPTED: [u32; 2] = [0x568D30, 0x568F60];
/// Sleeping: water still counts, the rest don't.
const ASLEEP: [u32; 2] = [0x54EF10, 0x54F7A0];

/// The interrupt checks before every behaviour frame (`0x53DB10`, re/chao-ai.md §1.4). The
/// ones from things around the Chao (whistle, ball, hat, fruit, animals, being held, petted
/// or hit) come in with `Senses`.
pub(super) fn interrupts(chao: &mut Chao, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) {
    let current = chao.current().map(|b| b.address());
    if current.is_some_and(|c| UNINTERRUPTED.contains(&c)) {
        return;
    }
    // In the water or asleep, the things nearby don't count; being held, petted or hit
    // still does (`0x53DB55`, `0x53DB5D`).
    let water = swim::water(chao, garden);
    let asleep = current.is_some_and(|c| ASLEEP.contains(&c));
    // Whatever fires unlinks the Chao (`0x530690` at `0x53DC1E`): a fruit in its hands drops.
    if touch::checks(chao, garden, senses, rng, water, asleep) {
        chao.toy_link = None;
        food::let_go(chao);
    }
}

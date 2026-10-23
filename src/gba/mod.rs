//! GBA link: reads creatures from a Gen 3 GBA save and their data from the player's GBA ROM.
//!
//! Nothing content-like ships with the mod; names, stats and sprites come from the ROM at runtime.

pub mod battle_anim;
pub mod ladder;
mod lz77;
pub mod rom;
pub mod save;
pub mod terrain;
pub mod text;
pub mod world;

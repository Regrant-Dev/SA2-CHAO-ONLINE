//! Chao battles: a turn-based battle engine with Gen 3 rules, singles and doubles, teams of up
//! to 6.
//!
//! No game or ROM access here: moves, the type chart and fighters come in as plain data (read
//! from the player's own ROM by the GBA link), and every call returns the `Event`s it caused for
//! the battle screen to show. The engine knows things only by id; names come from the ROM and
//! the fighters.
//!
//! - `data`: move records, targets, type ids and the type chart.
//! - `fighter`: a fighter's stats, moves, status and stat stages.
//! - `damage`: the damage formula.
//! - `effects`: what each move effect id does.
//! - `abilities`: where abilities step in.
//! - `engine`: the battle and its turns (using moves, status moves, the end of a turn); `ai`
//!   chooses for a computer-run side.
//! - `rng`: seeded randomness, so a seed replays a battle exactly.
//!
//! The battle screen in the game:
//!
//! - `team`: Chao as fighters (the stat mapping) and the ROM data a battle needs.
//! - `narrate`: a turn's events as beats (messages, what the Chao do, HUD changes).
//! - `session`: the picker and the turns, driven by the menu buttons.
//! - `arena`: the karate arena's menu, Chao, camera and motions.
//! - `game`: ties the session to the arena and the HUD each frame.

pub mod abilities;
pub mod ai;
mod arena;
pub mod damage;
pub mod data;
pub mod effects;
pub mod engine;
pub mod event;
pub mod field;
pub mod fighter;
pub mod game;
pub mod narrate;
pub mod rng;
pub mod session;
pub mod team;

pub use data::{Effectiveness, MoveData, Target, TypeChart};
pub use engine::{Action, Battle, BattleError, Choice, Format, Phase, Rules};
pub use event::{Blocked, Condition, Event, HurtBy, Outcome, Slot};
pub use fighter::{Ailment, Fighter, Stat, Stats};

#[cfg(test)]
mod tests;

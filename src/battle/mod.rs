//! Chao battles: a turn-based battle engine, singles and doubles, teams of up to 6.
//!
//! No game or catalog access here: moves, the type chart and fighters come in as plain data
//! (read from the player's own catalog by the creature link), and every call returns the
//! `Event`s it caused for the battle screen to show. The engine knows things only by id; names
//! come from the catalog and the fighters.
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
//! - `team`: Chao as fighters (the stat mapping) and the catalog data a battle needs.
//! - `narrate`: a turn's events as beats (messages, what the Chao do, HUD changes).
//! - `session`: the picker and the turns, driven by the menu buttons.
//! - `online`: live battles against a friend online, turns synced through the host; `mirror`
//!   shows the accepter's game the battle from their side.
//! - `levels`: battle levels and the XP battles give.
//! - `trainers` and `ladder`: the source game's trainers as opponents, their order and locks, and
//!   the progress and milestone moves won; `figures` shows their creatures in the arena.
//! - `visuals`: how the arena shows each cue (motions by kind of move, status signs, effect
//!   sprites, flash and shake), with `effects_gfx` reading the catalog's effect sprites and drawing
//!   them and `movement` moving each move's sprites the way its script does.
//! - `wild`: battling a wild visitor from a garden, a trip to the karate arena and back.
//! - `arena`: the karate arena's menu, Chao, camera and motions.
//! - `game`: ties the session to the arena and the HUD each frame.

pub mod abilities;
pub mod ai;
mod arena;
pub mod damage;
pub mod data;
pub mod effects;
pub mod effects_gfx;
pub mod engine;
pub mod event;
pub mod field;
pub mod fighter;
mod figures;
pub mod game;
pub mod ladder;
pub mod levels;
mod mirror;
pub mod movement;
pub mod narrate;
pub mod online;
pub mod rng;
pub mod session;
pub mod source_rules;
pub mod team;
pub mod trainers;
pub mod visuals;
pub mod wild;

pub use data::{Effectiveness, MoveData, Target, TypeChart};
pub use engine::{Action, Battle, BattleError, Choice, Format, Phase, Rules};
pub use event::{Blocked, Condition, Event, HurtBy, Outcome, Slot};
pub use fighter::{Ailment, Fighter, Stat, Stats};

#[cfg(test)]
mod tests;

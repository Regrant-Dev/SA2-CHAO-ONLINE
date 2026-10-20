//! The server's own garden Chao (re/chao-ai.md): the garden they live in and, built on it, the
//! brain that runs them. Plain Rust, no game: the dedicated server runs it, and the mod only
//! uses `land` to read the gardens out of the game and send them up.

pub mod land;

pub mod acts;
pub mod brain;
pub mod life;
pub mod motion;
pub mod physics;
pub mod picker;
pub mod record;
pub mod world;

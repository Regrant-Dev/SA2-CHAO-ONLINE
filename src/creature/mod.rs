//! Generic local creature data, private asset cache, and source-image import adapters.

pub mod anim_kinds;
pub mod battle_anim;
pub mod ladder;
pub mod locomotion;
pub mod placement;
pub mod rom;
#[cfg(any(target_arch = "x86", test))]
mod strips;
pub mod world;

#[cfg(feature = "game-import")]
pub mod archive;
#[cfg(feature = "game-import")]
pub mod asset_import;
pub mod assets;
pub mod catalog;
pub mod data;
pub mod import_error;
#[cfg(feature = "game-import")]
pub mod source;

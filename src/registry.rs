//! The Chao World object registry ("ALW"): Chao, eggs, items, trees and toys, one category each
//! (re/chao-garden-sync.md 1, re/chao-garden-items.md 7).
//!
//! Each category has a fixed run of 0x48-byte entries (0x1200 bytes per category) and a count.
//! An entry is in use while its object pointer (+0x3C) is set. The run isn't packed: removing
//! an object (0x5307F0) lowers the count and leaves a hole, and registering one (0x530750) takes
//! the first hole, so the first `count` entries (all the game's own walk 0x530470 looks at) can
//! miss the last ones. Walks here go over every entry up to the category's limit.

/// The entries (counts per category are just before, at 0x1DC0F80).
const WORLD: usize = 0x01DC_0FC0;
const ENTRY_SIZE: usize = 0x48;
const CATEGORY_SIZE: usize = 0x1200;
/// Entries per category the game allows (u32 per category: Chao 32, eggs 16, animals 10, fruit
/// 48, ...); never more than fit in a category's 0x1200 bytes.
const LIMITS: usize = 0x008A_B838;
const MAX_ENTRIES: usize = CATEGORY_SIZE / ENTRY_SIZE;
const CATEGORIES: usize = 12;
/// The Chao category.
pub const CHAO: usize = 0;
/// An entry's object (0 when it's free).
pub const ENTRY_OBJECT: usize = 0x3C;

/// The entries of `category` in use: (entry, object).
pub fn entries(category: usize) -> Vec<(usize, usize)> {
    if category >= CATEGORIES {
        return Vec::new();
    }
    let mut out = Vec::new();
    // SAFETY: the game's static registry and its limits table, read on the game thread; the
    // limit is clamped to the category's room.
    unsafe {
        let limit = ((LIMITS + category * 4) as *const u32).read() as usize;
        for i in 0..limit.min(MAX_ENTRIES) {
            let entry = WORLD + category * CATEGORY_SIZE + i * ENTRY_SIZE;
            let object = ((entry + ENTRY_OBJECT) as *const usize).read();
            if object != 0 {
                out.push((entry, object));
            }
        }
    }
    out
}

/// The category an entry belongs to.
pub fn category_of(entry: usize) -> usize {
    entry.wrapping_sub(WORLD) / CATEGORY_SIZE
}

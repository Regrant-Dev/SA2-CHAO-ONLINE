//! Visiting Chao and Chao files.
//!
//! - Export (F9): writes every non-empty garden slot to `chao\export\slotNN.sa2chao`.
//!   Read-only; never writes game memory.
//! - Bring visitors from files (F9, in a garden): spawns every `chao\import\*.sa2chao` as a
//!   visitor. Visitors live in our own buffers, outside the save's Chao slots, so the game's
//!   save never includes them.
//! - Online: other players in the same garden bring their Chao; online.rs passes them to
//!   `spawn_remote`, which uses the same visitor buffers.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::chao::{self, ChaoData, file};
use crate::game;
use crate::log;

/// Visitors per garden visit (Chao from files and other players' Chao together). The garden's own
/// Chao count too, so keep this modest until the garden's object limits are known.
pub const MAX_VISITORS: usize = 6;

/// Where a visitor came from, so the same Chao isn't spawned twice in one visit.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    File(PathBuf),
    Player { id: u8, index: u8 },
}

/// Visitor buffers. Leaked so their addresses never move: the game keeps a pointer
/// for the Chao object's lifetime. Buffers are reused after leaving the garden, when
/// the game has deleted the Chao objects.
struct Pool {
    buffers: Vec<&'static mut ChaoData>,
    active: usize,
    /// Visitors spawned during this garden visit, so they aren't duplicated.
    spawned: Vec<Source>,
}

static POOL: Mutex<Pool> = Mutex::new(Pool {
    buffers: Vec::new(),
    active: 0,
    spawned: Vec::new(),
});

/// The Chao a player carries to the next area (re/chao-garden-items.md 5): `ChaoData*` or 0.
const CARRIED_CHAO: usize = 0x01A5_CA5C;

/// Call every frame: frees the visitor slots once the player is out of the garden.
pub fn on_frame() {
    {
        // A visitor must never be carried out of the garden: the next area would recreate it
        // from our buffer, which is reused for other visitors.
        let pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: a static game pointer, only compared and cleared on the game thread.
        unsafe {
            let carried = (CARRIED_CHAO as *mut usize).read_volatile();
            if carried != 0
                && pool
                    .buffers
                    .iter()
                    .any(|b| std::ptr::from_ref::<ChaoData>(b) as usize == carried)
            {
                (CARRIED_CHAO as *mut usize).write_volatile(0);
                log!("visitors: a visitor was being carried out of the garden; left behind");
            }
        }
    }
    if !game::in_garden() {
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        if pool.active > 0 {
            log!(
                "visitors: left garden, {} visitor buffer(s) released",
                pool.active
            );
            pool.active = 0;
            pool.spawned.clear();
            crate::garden::clear();
        }
    }
}

fn chao_dir(mod_path: &str, sub: &str) -> PathBuf {
    PathBuf::from(mod_path).join("chao").join(sub)
}

pub fn export_all(mod_path: &str) {
    let dir = chao_dir(mod_path, "export");
    if let Err(e) = fs::create_dir_all(&dir) {
        log!("export: can't create {}: {e}", dir.display());
        return;
    }
    let mut count = 0;
    for slot in 0..game::GARDEN_SLOTS {
        let Some(chao) = game::read_chao_slot(slot) else {
            continue;
        };
        if chao.is_empty() {
            continue;
        }
        let valid = match chao.validate() {
            Ok(()) => "ok".to_string(),
            Err(e) => format!("INVALID ({e})"),
        };
        let path = dir.join(format!("slot{slot:02}.sa2chao"));
        match fs::write(&path, file::encode(&chao)) {
            Ok(()) => {
                count += 1;
                log!(
                    "export: slot {slot:02} -> {} [{valid}] {}",
                    path.display(),
                    chao.summary()
                );
            }
            Err(e) => log!("export: slot {slot:02} write failed: {e}"),
        }
    }
    log!(
        "export: {count} chao exported (level {}, chao area {})",
        game::current_level(),
        game::current_chao_area()
    );
}

pub fn import_visitors(mod_path: &str) {
    if !game::in_garden() {
        log!(
            "import: not in a garden (level {}, chao area {}); skipped",
            game::current_level(),
            game::current_chao_area()
        );
        return;
    }
    let Some((origin, angle)) = game::player_position() else {
        log!("import: no player object; skipped");
        return;
    };

    let dir = chao_dir(mod_path, "import");
    let _ = fs::create_dir_all(&dir);
    let mut files: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("sa2chao"))
            })
            .collect(),
        Err(e) => {
            log!("import: can't read {}: {e}", dir.display());
            return;
        }
    };
    files.sort();
    if files.is_empty() {
        log!("import: no .sa2chao files in {}", dir.display());
        return;
    }

    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    for path in files {
        if pool.spawned.contains(&Source::File(path.clone())) {
            log!("import: {} is already visiting", path.display());
            continue;
        }
        if pool.active >= MAX_VISITORS {
            log!("import: visitor limit ({MAX_VISITORS}) reached");
            break;
        }
        let chao = match fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|d| file::decode(&d).map_err(|e| e.to_string()))
        {
            Ok(chao) => chao,
            Err(e) => {
                log!("import: {} rejected: {e}", path.display());
                continue;
            }
        };
        if chao.chao_type() == chao::TYPE_EGG {
            log!("import: {} is an egg; eggs can't visit yet", path.display());
            continue;
        }

        let n = pool.active as f32;
        let position = game::Vector {
            x: origin.x + 10.0 + n * 8.0,
            y: origin.y + 5.0,
            z: origin.z,
        };
        spawn(&mut pool, Source::File(path), chao, position, angle);
    }
}

/// Spawns another player's Chao next to `near` (their puppet), once per garden visit.
/// Returns false when it was already spawned or the visitor limit is reached.
pub fn spawn_remote(player: u8, index: u8, chao: ChaoData, near: game::Vector) -> bool {
    if !game::in_garden() || chao.validate().is_err() || chao.chao_type() == chao::TYPE_EGG {
        return false;
    }
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let source = Source::Player { id: player, index };
    if pool.spawned.contains(&source) || pool.active >= MAX_VISITORS {
        return false;
    }
    let position = game::Vector {
        x: near.x + 6.0 + f32::from(index) * 6.0,
        y: near.y + 5.0,
        z: near.z + 6.0,
    };
    let object = spawn(&mut pool, source, chao, position, 0);
    crate::garden::spawned(player, index, object);
    true
}

/// Copies `chao` into a free visitor buffer and creates the Chao object (returned, or 0).
fn spawn(
    pool: &mut Pool,
    source: Source,
    chao: ChaoData,
    position: game::Vector,
    angle: i32,
) -> usize {
    // Buffers at index >= active are not in use by any Chao object.
    let index = pool.active;
    if index == pool.buffers.len() {
        pool.buffers.push(Box::leak(Box::new(chao)));
    } else {
        *pool.buffers[index] = chao;
    }
    let data: &mut ChaoData = pool.buffers[index];
    log!(
        "visitors: spawning {source:?} at ({:.1}, {:.1}, {:.1}): {}",
        position.x,
        position.y,
        position.z,
        data.summary()
    );
    // SAFETY: called from OnFrame (game thread) while in a garden; the buffer is leaked and
    // not reused until the player leaves the garden.
    let object = unsafe { game::create_chao(data, position, angle) };
    log!("visitors: CreateChao returned {object:p}");
    pool.active += 1;
    pool.spawned.push(source);
    object as usize
}

//! Visiting Chao and Chao files.
//!
//! - Export (F9): writes every non-empty garden slot to `chao\export\slotNN.sa2chao`, and
//!   what they have from creature link partners to `chao\export\partners.txt` (creature_link).
//!   Read-only; never writes game memory.
//! - Bring visitors from files (F9, in a garden): spawns every `chao\import\*.sa2chao` as a
//!   visitor. Visitors live in our own buffers, outside the save's Chao slots, so the game's
//!   save never includes them.
//! - Online: other players in the same garden bring their Chao; online.rs passes them to
//!   `spawn_remote`, which uses the same visitor buffers.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use crate::chao::{self, ChaoData, file};
use crate::game;
use crate::log;

/// Visitors per garden visit (Chao from files, other players' Chao and the ones the server
/// holds for players who aren't there, together). In a shared garden we show only the
/// `online::BRING_CHAO` (4) we bring (bring.rs), so with these that's the Chao registry's 32
/// entries (the server shows 32 in all, `chao_host::ROOM`).
pub const MAX_VISITORS: usize = 32 - crate::online::BRING_CHAO;

/// Where a visitor came from, so the same Chao isn't spawned twice in one visit.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    File(PathBuf),
    Player { id: u8, index: u8 },
}

/// Visitor buffers. Leaked so their addresses never move: the game keeps a pointer
/// for the Chao object's lifetime. All are free again after leaving the garden, when the game
/// has deleted the Chao objects; one whose visitor left earlier (`forget`) is reused only after
/// `REUSE_AFTER`, once the game is surely done deleting it.
struct Buffer {
    data: &'static mut ChaoData,
    /// The visitor in it (this garden visit).
    used_by: Option<Source>,
    /// Its visitor left mid-visit at this time.
    freed: Option<Instant>,
}

struct Pool {
    buffers: Vec<Buffer>,
}

const REUSE_AFTER: Duration = Duration::from_secs(3);

impl Pool {
    /// Visitors in the garden now.
    fn count(&self) -> usize {
        self.buffers.iter().filter(|b| b.used_by.is_some()).count()
    }

    fn has(&self, source: &Source) -> bool {
        self.buffers
            .iter()
            .any(|b| b.used_by.as_ref() == Some(source))
    }
}

static POOL: Mutex<Pool> = Mutex::new(Pool {
    buffers: Vec::new(),
});

/// The Chao a player carries to the next area (re/chao-garden-items.md 5): `ChaoData*` or 0.
const CARRIED_CHAO: usize = 0x01A5_CA5C;

/// A save slot's Chao: (slot, type, garden).
type SlotChao = (usize, u8, u8);
/// Frames counted, and the save's Chao as last logged.
static SAVE_SEEN: Mutex<(u32, Vec<SlotChao>)> = Mutex::new((0, Vec::new()));

/// Every few seconds: when the save's Chao change (Chao World loaded them, a Chao hatched or
/// moved), logs each one and whether online play can use it, and exports them to files.
fn log_save_chao() {
    let mut seen = SAVE_SEEN.lock().unwrap_or_else(|e| e.into_inner());
    seen.0 = seen.0.wrapping_add(1);
    if seen.0 % 300 != 1 {
        return;
    }
    let chao: Vec<(usize, ChaoData)> = (0..game::GARDEN_SLOTS)
        .filter_map(|slot| game::read_chao_slot(slot).map(|c| (slot, c)))
        .filter(|(_, c)| !c.is_empty())
        .collect();
    let now: Vec<SlotChao> = chao
        .iter()
        .map(|(slot, c)| (*slot, c.chao_type(), c.garden()))
        .collect();
    if now == seen.1 {
        return;
    }
    // A hatched egg (or a Chao that moved) may fill an Auto visitor row now.
    if !seen.1.is_empty() {
        crate::online::refresh_visitors();
    }
    seen.1 = now;
    log!("save: {} Chao", chao.len());
    for (slot, c) in &chao {
        let usable = match c.validate() {
            _ if c.chao_type() == chao::TYPE_EGG => "egg, stays home".to_string(),
            Ok(()) => "ok for online".to_string(),
            Err(e) => format!("NOT used online: {e}"),
        };
        log!("save: slot {:02} {} [{usable}]", slot + 1, c.summary());
    }
    if !chao.is_empty() {
        export_all(crate::online::mod_path());
    }
}

/// When we entered the current garden, and whether the visitors from files came yet.
static IN_GARDEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
/// The garden we're in (0 outside): the transporter goes straight from one garden to the next
/// (0x52B030), and the old one's Chao objects are gone then too.
static AREA: AtomicI32 = AtomicI32::new(0);
/// Visitors from files come this long after entering a garden (it has loaded by then).
const IMPORT_AFTER: Duration = Duration::from_secs(2);

/// Brings the visitors from `chao/import` once per garden visit, shortly after arriving.
fn auto_import(changed: bool) {
    let mut g = IN_GARDEN.lock().unwrap_or_else(|e| e.into_inner());
    if !game::in_garden() || changed {
        *g = None;
        return;
    }
    let (since, done) = g.get_or_insert((Instant::now(), false));
    if !*done && since.elapsed() >= IMPORT_AFTER && game::player_position().is_some() {
        *done = true;
        drop(g);
        import_visitors(crate::online::mod_path());
    }
}

/// Call every frame: frees the visitor slots once the player is out of the garden.
pub fn on_frame() {
    log_save_chao();
    let area = if game::in_garden() {
        game::current_chao_area()
    } else {
        0
    };
    let changed = AREA.swap(area, Ordering::Relaxed) != area;
    auto_import(changed);
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
                    .any(|b| std::ptr::from_ref::<ChaoData>(b.data) as usize == carried)
            {
                (CARRIED_CHAO as *mut usize).write_volatile(0);
                log!("visitors: a visitor was being carried out of the garden; left behind");
            }
        }
    }
    if changed {
        // Left the garden, or went straight to another one.
        crate::garden::clear();
        crate::eggs::clear();
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        if pool
            .buffers
            .iter()
            .any(|b| b.used_by.is_some() || b.freed.is_some())
        {
            log!(
                "visitors: left the garden, {} visitor buffer(s) released",
                pool.count()
            );
            for b in &mut pool.buffers {
                b.used_by = None;
                b.freed = None;
            }
            drop(pool);
            // The garden may have changed them (the game saves on leaving too).
            export_all(crate::online::mod_path());
        }
    }
}

fn chao_dir(mod_path: &str, sub: &str) -> PathBuf {
    PathBuf::from(mod_path).join("chao").join(sub)
}

fn export_all(mod_path: &str) {
    let dir = chao_dir(mod_path, "export");
    if let Err(e) = fs::create_dir_all(&dir) {
        log!("export: can't create {}: {e}", dir.display());
        return;
    }
    let mut count = 0;
    let mut identities = Vec::new();
    for slot in 0..game::GARDEN_SLOTS {
        let Some(chao) = game::read_chao_slot(slot) else {
            continue;
        };
        if chao.is_empty() {
            continue;
        }
        let path = dir.join(format!("slot{slot:02}.sa2chao"));
        match fs::write(&path, file::encode(&chao)) {
            Ok(()) => {
                count += 1;
                identities.push(chao.identity());
            }
            Err(e) => log!("export: slot {slot:02} write failed: {e}"),
        }
    }
    // What they have from creature link partners, their moves and battle levels, for battles against
    // copies of them.
    crate::creature_link::export_partners(&dir, &identities);
    log!("export: {count} Chao written to {}", dir.display());
}

fn import_visitors(mod_path: &str) {
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
        return;
    }

    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    for path in files {
        if pool.has(&Source::File(path.clone())) {
            log!("import: {} is already visiting", path.display());
            continue;
        }
        if pool.count() >= MAX_VISITORS {
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

        let n = pool.count() as f32;
        let position = game::Vector {
            x: origin.x + 10.0 + n * 8.0,
            y: origin.y + 5.0,
            z: origin.z,
        };
        spawn(&mut pool, Source::File(path), chao, position, angle);
    }
}

/// Spawns another player's Chao at its last reported position, or next to `near` when unknown.
/// Returns false when it was already spawned or the visitor limit is reached.
pub fn spawn_remote(
    player: u8,
    index: u8,
    chao: ChaoData,
    near: game::Vector,
    initial: Option<crate::net::protocol::GardenChao>,
) -> bool {
    // Not while an area is loaded or torn down: a copy made then outlives the garden.
    if !game::in_garden()
        || game::between_areas()
        || crate::garden::tearing_down()
        || chao.validate().is_err()
    {
        return false;
    }
    let egg = chao.chao_type() == chao::TYPE_EGG;
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let source = Source::Player { id: player, index };
    if pool.has(&source) || pool.count() >= MAX_VISITORS {
        return false;
    }
    let position = initial.map_or(
        game::Vector {
            x: near.x + 6.0 + f32::from(index) * 6.0,
            y: near.y + 5.0,
            z: near.z + 6.0,
        },
        |s| {
            let [x, y, z] = s.position;
            game::Vector { x, y, z }
        },
    );
    let angle = initial.map_or(0, |s| s.rotation_y);
    let object = spawn(&mut pool, source, chao, position, angle);
    // An egg is an egg object, not a Chao: eggs.rs looks after it.
    if egg {
        crate::eggs::spawned(player, index, object);
    } else {
        crate::garden::spawned(player, index, object, initial);
    }
    true
}

/// Player `id`'s Chao `index` left the garden (its copy is being deleted): it may visit again,
/// and its buffer is reused later.
pub fn forget(id: u8, index: u8) {
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let source = Source::Player { id, index };
    for b in &mut pool.buffers {
        if b.used_by.as_ref() == Some(&source) {
            b.used_by = None;
            b.freed = Some(Instant::now());
        }
    }
}

/// Copies `chao` into a free visitor buffer and creates the Chao object (returned, or 0).
fn spawn(
    pool: &mut Pool,
    source: Source,
    chao: ChaoData,
    position: game::Vector,
    angle: i32,
) -> usize {
    let free = pool
        .buffers
        .iter()
        .position(|b| b.used_by.is_none() && b.freed.is_none_or(|at| at.elapsed() >= REUSE_AFTER));
    let index = match free {
        Some(index) => {
            *pool.buffers[index].data = chao;
            index
        }
        None => {
            pool.buffers.push(Buffer {
                data: Box::leak(Box::new(chao)),
                used_by: None,
                freed: None,
            });
            pool.buffers.len() - 1
        }
    };
    let buffer = &mut pool.buffers[index];
    buffer.used_by = Some(source.clone());
    buffer.freed = None;
    let data: &mut ChaoData = buffer.data;
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
    if object.is_null() {
        // Nothing uses the buffer: free it so this Chao can come again.
        pool.buffers[index].used_by = None;
    }
    object as usize
}

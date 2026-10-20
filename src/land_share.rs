//! Sends the host the garden we walk into, its ground, cell grid and climbable edges as our game
//! has them (chao_sim::land), and the Chao motion table (chao_sim::motion), so a server running
//! gardens' Chao has them without any game files.

use std::sync::Mutex;

use crate::chao_sim::land::{Garden, Mem};
use crate::net::protocol::{Body, LAND_PART};

/// The garden whose ground we last sent (0: none yet, or we left).
static SENT: Mutex<u8> = Mutex::new(0);
/// Frames we've waited for the garden's toys to be made.
static WAITED: Mutex<u32> = Mutex::new(0);
/// Past this many frames the garden goes without toys.
const TOYS_WAIT: u32 = 120;

/// `GardenLand` "garden" number of the motion table.
pub const MOTION_TABLE: u8 = 3;

/// Connected (again): the garden we're in goes to the host anew.
pub fn reset() {
    *SENT.lock().unwrap_or_else(|e| e.into_inner()) = 0;
}

/// Call every frame while online with the garden we're in (1..=3, or 0). Once a garden we
/// walked into is loaded, returns its ground (and the motion table) in `GardenLand` parts to
/// send reliably.
pub fn on_frame(area: u8) -> Vec<Body> {
    let mut sent = SENT.lock().unwrap_or_else(|e| e.into_inner());
    if !(1..=3).contains(&area) {
        *sent = 0;
        return Vec::new();
    }
    if *sent == area {
        return Vec::new();
    }
    let index = usize::from(area - 1);
    // Until its object layout is in, the garden isn't loaded yet: try again next frame.
    let Some(mut garden) = read(index) else {
        return Vec::new();
    };
    garden.toys = read_toys();
    let mut waited = WAITED.lock().unwrap_or_else(|e| e.into_inner());
    if garden.toys.is_empty() && *waited < TOYS_WAIT {
        *waited += 1;
        return Vec::new();
    }
    *waited = 0;
    *sent = area;
    let mut out = parts(index as u8, &garden.to_bytes(), garden.fingerprint());
    crate::log!(
        "land: sending garden {area} ({} triangles, {} ledges, {} blockers, {} toys, {:08x})",
        garden.tris.len(),
        garden.ledges.len(),
        garden.blockers.len(),
        garden.toys.len(),
        garden.fingerprint()
    );
    if let Some(table) = read_motions() {
        out.extend(parts(MOTION_TABLE, &table.to_bytes(), table.fingerprint()));
    }
    out
}

fn parts(garden: u8, bytes: &[u8], fingerprint: u32) -> Vec<Body> {
    let parts = bytes.chunks(LAND_PART).count() as u8;
    bytes
        .chunks(LAND_PART)
        .enumerate()
        .map(|(part, chunk)| Body::GardenLand {
            garden,
            fingerprint,
            part: part as u8,
            parts,
            bytes: chunk.to_vec(),
        })
        .collect()
}

/// The game's data DLL (the Mod Loader loads it as `Data_DLL_orig.dll`).
#[cfg(target_arch = "x86")]
fn data_dll() -> Option<*mut std::ffi::c_void> {
    [c"Data_DLL_orig.dll", c"Data_DLL.dll"]
        .into_iter()
        // SAFETY: plain module lookups by name.
        .map(|name| unsafe { GetModuleHandleA(name.as_ptr()) })
        .find(|m| !m.is_null())
}

#[cfg(target_arch = "x86")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleA(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn GetProcAddress(
        module: *mut std::ffi::c_void,
        name: *const std::ffi::c_char,
    ) -> *mut std::ffi::c_void;
}

/// An export of the data DLL.
#[cfg(target_arch = "x86")]
fn export(name: &str) -> Option<u32> {
    let dll = data_dll()?;
    let name = std::ffi::CString::new(name).ok()?;
    // SAFETY: an export lookup in a loaded module.
    let at = unsafe { GetProcAddress(dll, name.as_ptr()) } as u32;
    (at != 0).then_some(at)
}

/// Garden `index`'s ground from Data_DLL's LandTable export and the exe's grid, and its
/// ledges from the garden's loaded object layout (re/garden-collision.md §8.3); None until
/// that layout is this garden's.
#[cfg(target_arch = "x86")]
fn read(index: usize) -> Option<Garden> {
    use crate::chao_sim::land::{CLIMB_TYPE, LAND_EXPORT, SET_LISTS, read_blockers, read_ledges};
    let land = export(LAND_EXPORT[index])?;
    let mem = Images::of(&[data_dll()? as u32, 0x0040_0000])?;
    // SAFETY: the exe's pointers to the loaded SET data: object list, header, entries.
    let (list, header, entries) = unsafe {
        (
            (0x01DD_E268 as *const u32).read(),
            (0x01DD_E274 as *const u32).read(),
            (0x01DD_E278 as *const u32).read(),
        )
    };
    if list != SET_LISTS[index] || header == 0 || entries == 0 {
        return None;
    }
    // SAFETY: the loaded header's entry count.
    let count = unsafe { (header as *const u32).read() }.min(4096);
    let set = Images(vec![(entries, count * 0x20)]);
    let mut garden = Garden::read(&mem, land, &mem, index)?;
    garden.ledges = read_ledges(&set, entries, count, CLIMB_TYPE[index])?;
    garden.blockers = read_blockers(&set, entries, count, index)?;
    Some(garden)
}

/// The Chao motion table (Data_DLL's `al_motion_table`).
#[cfg(target_arch = "x86")]
fn read_motions() -> Option<crate::chao_sim::motion::MotionTable> {
    let table = export(crate::chao_sim::motion::TABLE_EXPORT)?;
    let mem = Images::of(&[data_dll()? as u32])?;
    crate::chao_sim::motion::MotionTable::read(&mem, table)
}

/// The garden's toys (garden_toys.rs finds them by their update).
#[cfg(target_arch = "x86")]
fn read_toys() -> Vec<crate::chao_sim::land::GardenToy> {
    crate::garden_toys::placed()
}

#[cfg(not(target_arch = "x86"))]
fn read_toys() -> Vec<crate::chao_sim::land::GardenToy> {
    Vec::new()
}

#[cfg(not(target_arch = "x86"))]
fn read_motions() -> Option<crate::chao_sim::motion::MotionTable> {
    None
}

#[cfg(not(target_arch = "x86"))]
fn read(_index: usize) -> Option<Garden> {
    None
}

/// The game's own memory, limited to the loaded images of some modules (their data is static,
/// and a bad pointer outside them is refused instead of read).
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
struct Images(Vec<(u32, u32)>);

#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
impl Images {
    /// The images at these bases, sized from their PE headers.
    fn of(bases: &[u32]) -> Option<Self> {
        let mut ranges = Vec::new();
        for &base in bases {
            // SAFETY: a loaded module's headers: e_lfanew at +0x3C, SizeOfImage at nt+0x50.
            let size = unsafe {
                let nt = ((base + 0x3C) as *const u32).read_unaligned();
                ((base + nt + 0x50) as *const u32).read_unaligned()
            };
            ranges.push((base, size));
        }
        Some(Self(ranges))
    }
}

impl Mem for Images {
    fn bytes(&self, addr: u32, len: usize) -> Option<&[u8]> {
        let end = addr.checked_add(len as u32)?;
        self.0
            .iter()
            .any(|&(base, size)| addr >= base && end <= base + size)
            .then_some(())?;
        // SAFETY: inside a loaded module's image (checked above).
        Some(unsafe { std::slice::from_raw_parts(addr as *const u8, len) })
    }
}

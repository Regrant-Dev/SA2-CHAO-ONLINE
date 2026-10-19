//! Opt-in native Black Market jewel eggs. Native evidence: re/jewel-eggs.md.
//! The original shop charges rings, delivers eggs, initializes genes and handles hatching.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::{hud, log, modloader::HelperFunctions, patch, prs};

const EGG_TABLE: usize = 0x008A_7B68;
const EGG_CATEGORY: usize = 0x008A_8730;
const ATTRIBUTE_SIZE: usize = 16;
const STOCK_FUNCTION: usize = 0x0058_9A00;
const STOCK_CALL: usize = 0x0058_B8EC;
const STOCK_COUNT: usize = 0x01DB_ED90;
const STOCK_ITEMS: usize = 0x01DB_EDC0;
const EMBLEMS: usize = 0x01DE_C60E;
const CAPACITY: usize = 32;
const FIRST_MESSAGE: usize = 99;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static FAILED: AtomicBool = AtomicBool::new(false);
static RNG: AtomicU32 = AtomicU32::new(0x83AD_61C7);
// Retain the path strings for the entire process, including helpers that retain pointers.
static REPLACEMENTS: OnceLock<Vec<(CString, CString)>> = OnceLock::new();

#[derive(Clone, Copy)]
struct Egg {
    name: &'static str,
    id: u8,
    emblems: u8,
    rings: u32,
}

const EGGS: [Egg; 12] = [
    Egg {
        name: "Gold",
        id: 54,
        emblems: 120,
        rings: 10000,
    },
    Egg {
        name: "Silver",
        id: 55,
        emblems: 125,
        rings: 12000,
    },
    Egg {
        name: "Ruby",
        id: 56,
        emblems: 130,
        rings: 15000,
    },
    Egg {
        name: "Sapphire",
        id: 57,
        emblems: 135,
        rings: 18000,
    },
    Egg {
        name: "Emerald",
        id: 58,
        emblems: 140,
        rings: 20000,
    },
    Egg {
        name: "Amethyst",
        id: 59,
        emblems: 145,
        rings: 22000,
    },
    Egg {
        name: "Aquamarine",
        id: 60,
        emblems: 150,
        rings: 24000,
    },
    Egg {
        name: "Garnet",
        id: 61,
        emblems: 155,
        rings: 26000,
    },
    Egg {
        name: "Onyx",
        id: 62,
        emblems: 160,
        rings: 28000,
    },
    Egg {
        name: "Peridot",
        id: 63,
        emblems: 165,
        rings: 30000,
    },
    Egg {
        name: "Topaz",
        id: 64,
        emblems: 170,
        rings: 35000,
    },
    Egg {
        name: "Pearl",
        id: 65,
        emblems: 180,
        rings: 40000,
    },
];

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}
pub fn unavailable() -> bool {
    FAILED.load(Ordering::Relaxed)
}

fn pick_offer(stock: &[u16], emblems: u8, roll: u32) -> Option<Egg> {
    if stock.len() >= CAPACITY {
        return None;
    }
    let eligible: Vec<_> = EGGS
        .iter()
        .copied()
        .filter(|egg| emblems >= egg.emblems && !stock.contains(&item(*egg)))
        .collect();
    (!eligible.is_empty()).then(|| eligible[roll as usize % eligible.len()])
}

fn item(egg: Egg) -> u16 {
    1 | u16::from(egg.id) << 8
}

fn native_attributes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for egg in EGGS {
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(999i16.to_le_bytes());
        bytes.extend(1i16.to_le_bytes());
        bytes.extend(
            (if egg.id <= 55 {
                64i16
            } else if egg.id <= 64 {
                65
            } else {
                66
            })
            .to_le_bytes(),
        );
        bytes.extend(0u16.to_le_bytes());
    }
    bytes
}

fn attributes() -> Vec<u8> {
    let mut bytes = native_attributes();
    for (i, egg) in EGGS.iter().enumerate() {
        let base = i * ATTRIBUTE_SIZE;
        bytes[base..base + 4].copy_from_slice(&egg.rings.to_le_bytes());
        bytes[base + 8..base + 10].copy_from_slice(&i16::from(egg.emblems).to_le_bytes());
        bytes[base + 10..base + 12].copy_from_slice(&((FIRST_MESSAGE + i) as i16).to_le_bytes());
        bytes[base + 12..base + 14]
            .copy_from_slice(&((FIRST_MESSAGE + EGGS.len() + i) as i16).to_le_bytes());
    }
    bytes
}

/// Append to a big-endian offset table ending in -1. Keep every original payload byte and
/// message ID, including control codes, padding and strings shared by multiple offsets.
fn append_messages(bytes: &[u8], extra: &[String]) -> Result<Vec<u8>, String> {
    if bytes.len() > 1 << 20 {
        return Err("item message file is too large".into());
    }
    let mut offsets = Vec::new();
    let mut position = 0;
    loop {
        let word = bytes
            .get(position..position + 4)
            .ok_or("truncated item message table")?;
        position += 4;
        let offset = u32::from_be_bytes(word.try_into().unwrap());
        if offset == u32::MAX {
            break;
        }
        offsets.push(offset);
        if offsets.len() > 4096 {
            return Err("item message table has no terminator".into());
        }
    }
    if offsets.len() != FIRST_MESSAGE {
        return Err(format!(
            "expected {FIRST_MESSAGE} item messages, found {}",
            offsets.len()
        ));
    }
    if offsets
        .iter()
        .any(|&off| (off as usize) < position || off as usize >= bytes.len())
    {
        return Err("item message offset is outside its payload".into());
    }
    let delta = extra.len() * 4;
    let mut output = Vec::new();
    for offset in offsets {
        output.extend((offset + delta as u32).to_be_bytes());
    }
    let mut tail = Vec::new();
    for message in extra {
        if message.as_bytes().contains(&0) {
            return Err("invalid added item message".into());
        }
        output.extend(((bytes.len() + delta + tail.len()) as u32).to_be_bytes());
        tail.extend(message.as_bytes());
        tail.push(0);
    }
    output.extend(u32::MAX.to_be_bytes());
    output.extend(&bytes[position..]);
    output.extend(tail);
    Ok(output)
}

fn message_strings() -> Vec<String> {
    EGGS.iter()
        .map(|egg| format!("{} jewel egg", egg.name))
        .chain(EGGS.iter().map(|egg| {
            format!(
                "A {} jewel Chao egg. Unlocked at {} emblems.",
                egg.name, egg.emblems
            )
        }))
        .collect()
}

fn prepare_messages(
    mod_path: &str,
    helpers: &HelperFunctions,
) -> Result<Vec<(CString, CString)>, String> {
    let mut replacements = Vec::new();
    let strings = message_strings();
    for folder in ["Message", "MessageK"] {
        for language in ["e", "j", "f", "g", "s", "i"] {
            let name = format!("MsgAlItem_{language}.prs");
            let source = CString::new(format!("resource\\gd_PC\\{folder}\\{name}")).unwrap();
            // SAFETY: Mod Loader's documented C ABI; the source is NUL-terminated.
            let path = unsafe { (helpers.get_replaceable_path)(source.as_ptr()) };
            if path.is_null() {
                continue;
            }
            // SAFETY: the helper returns its valid NUL-terminated replacement path.
            let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
            let bytes = match std::fs::read(Path::new(path.as_ref())) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("can't read {name}: {e}")),
            };
            let decoded = prs::decode(&bytes)?;
            let extended = append_messages(&decoded, &strings)?;
            let output = PathBuf::from(mod_path)
                .join("cache/jewel-eggs")
                .join(folder)
                .join(&name);
            std::fs::create_dir_all(output.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&output, prs::encode(&extended)).map_err(|e| e.to_string())?;
            let destination =
                CString::new(output.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
            replacements.push((source, destination));
        }
    }
    if replacements.is_empty() {
        return Err("no native Black Market item message files found".into());
    }
    Ok(replacements)
}

fn verify_native() -> Result<(), String> {
    if !cfg!(all(windows, target_arch = "x86")) {
        return Err("jewel eggs require the 32-bit game".into());
    }
    // SAFETY: fixed native tables/code, checked before any write or hook.
    unsafe {
        if (EGG_CATEGORY as *const usize).read() != EGG_TABLE
            || ((EGG_CATEGORY + 4) as *const u32).read() != 69
        {
            return Err("another shop mod changed the egg catalog".into());
        }
        if std::slice::from_raw_parts(STOCK_FUNCTION as *const u8, 8)
            != [0x83, 0xEC, 0x4C, 0xA1, 0, 0x73, 0x91, 0]
        {
            return Err("the native stock generator was changed".into());
        }
        if std::slice::from_raw_parts(STOCK_CALL as *const u8, 5) != call_bytes(STOCK_FUNCTION) {
            return Err("the native shop restock call was changed".into());
        }
        let expected = native_attributes();
        if std::slice::from_raw_parts(
            (EGG_TABLE + 54 * ATTRIBUTE_SIZE) as *const u8,
            expected.len(),
        ) != expected
        {
            return Err("the native jewel egg attributes were changed".into());
        }
    }
    Ok(())
}

fn call_bytes(target: usize) -> [u8; 5] {
    let mut bytes = [0xE8, 0, 0, 0, 0];
    bytes[1..].copy_from_slice(&(target.wrapping_sub(STOCK_CALL + 5) as u32).to_le_bytes());
    bytes
}

pub fn init(mod_path: &str, helpers: &HelperFunctions) {
    if active() {
        return;
    }
    if !crate::menu::load(mod_path).jewel_eggs {
        log!("jewel eggs: off (opt in through F9 or Mod Manager)");
        return;
    }
    let result = (|| -> Result<(), String> {
        verify_native()?;
        let replacements = prepare_messages(mod_path, helpers)?;
        let expected = native_attributes();
        let updated = attributes();
        let address = EGG_TABLE + 54 * ATTRIBUTE_SIZE;
        if !patch::replace_code(address, &expected, &updated) {
            return Err("couldn't set jewel egg prices/names".into());
        }
        // Register generated copies of the player's messages. The native loader still owns
        // and frees all in-memory message buffers; no Rust heap pointer enters that lifetime.
        for (source, destination) in &replacements {
            unsafe { (helpers.replace_file)(source.as_ptr(), destination.as_ptr()) };
        }
        let _ = REPLACEMENTS.set(replacements);
        let hook = restock as extern "C" fn() as usize;
        if !patch::replace_code(STOCK_CALL, &call_bytes(STOCK_FUNCTION), &call_bytes(hook)) {
            let _ = patch::replace_code(address, &updated, &expected);
            return Err("couldn't install the jewel restock hook".into());
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u32);
        RNG.store(seed.max(1), Ordering::Relaxed);
        ACTIVE.store(true, Ordering::Relaxed);
        Ok(())
    })();
    match result {
        Ok(()) => log!(
            "jewel eggs: enabled; 120-180 emblems, 10000-40000 rings; one extra offer per restock"
        ),
        Err(error) => {
            FAILED.store(true, Ordering::Relaxed);
            log!("jewel eggs: unavailable: {error}");
            hud::toast("Jewel eggs couldn't be enabled; see the mod log");
        }
    }
}

extern "C" fn restock() {
    // SAFETY: the original no-argument stock generator; its caller ignores EAX.
    #[cfg(target_arch = "x86")]
    unsafe {
        std::mem::transmute::<usize, extern "C" fn()>(STOCK_FUNCTION)()
    };
    if std::panic::catch_unwind(add_offer).is_err() {
        log!("jewel eggs: couldn't add an offer after native restocking");
    }
}

fn add_offer() {
    if !active() {
        return;
    }
    // SAFETY: native static shop data on the game thread immediately after stock generation.
    // Check the catalog hasn't been redirected by another mod before touching stock.
    unsafe {
        if (EGG_CATEGORY as *const usize).read() != EGG_TABLE
            || ((EGG_CATEGORY + 4) as *const u32).read() != 69
        {
            return;
        }
        let count = (STOCK_COUNT as *const u32).read() as usize;
        if count >= CAPACITY {
            return;
        }
        let stock = std::slice::from_raw_parts(STOCK_ITEMS as *const u16, count);
        let emblems = (EMBLEMS as *const u8).read();
        let mut roll = RNG.load(Ordering::Relaxed);
        roll ^= roll << 13;
        roll ^= roll >> 17;
        roll ^= roll << 5;
        RNG.store(roll, Ordering::Relaxed);
        if let Some(egg) = pick_offer(stock, emblems, roll) {
            ((STOCK_ITEMS + count * 2) as *mut u16).write(item(egg));
            (STOCK_COUNT as *mut u32).write((count + 1) as u32);
            log!(
                "jewel eggs: stocked {} ({} emblems, {} rings)",
                egg.name,
                emblems,
                egg.rings
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offers_follow_emblem_thresholds_and_do_not_overwrite_vanilla_stock() {
        let vanilla = vec![0x0201, 0x0003, 0x0107];
        let before = vanilla.clone();
        assert!(pick_offer(&vanilla, 119, 0).is_none());
        assert_eq!(pick_offer(&vanilla, 120, 999).unwrap().id, 54);
        assert_eq!(pick_offer(&vanilla, 125, 1).unwrap().id, 55);
        assert_eq!(pick_offer(&vanilla, 180, 11).unwrap().id, 65);
        assert_eq!(vanilla, before);
        assert!(pick_offer(&[0; CAPACITY], 180, 0).is_none());
        assert!(pick_offer(&[item(EGGS[0])], 120, 0).is_none());
    }
    #[test]
    fn catalog_uses_supported_native_jewel_ids_and_valid_message_indices() {
        let bytes = attributes();
        assert_eq!(bytes.len(), EGGS.len() * ATTRIBUTE_SIZE);
        for (i, egg) in EGGS.iter().enumerate() {
            assert_eq!(egg.id - 53, i as u8 + 1); // native gene conversion: egg 54..65 -> jewel 1..12
            assert!(egg.rings > 0 && egg.emblems <= 180);
            assert_eq!(
                u16::from_le_bytes(bytes[i * 16 + 10..i * 16 + 12].try_into().unwrap()) as usize,
                FIRST_MESSAGE + i
            );
        }
    }
    fn original_messages() -> Vec<u8> {
        let header = (FIRST_MESSAGE + 1) * 4;
        let mut bytes = Vec::new();
        for _ in 0..FIRST_MESSAGE {
            bytes.extend((header as u32).to_be_bytes());
        }
        bytes.extend(u32::MAX.to_be_bytes());
        bytes.extend([0x81, 0x41, 0, 0xFE, 0x7F]);
        bytes
    }
    #[test]
    fn new_names_preserve_every_original_message_id_and_payload_byte() {
        let original = original_messages();
        let extra = message_strings();
        let bytes = append_messages(&original, &extra).unwrap();
        let old_header = (FIRST_MESSAGE + 1) * 4;
        let new_header = old_header + extra.len() * 4;
        assert_eq!(
            &bytes[new_header..new_header + original.len() - old_header],
            &original[old_header..]
        );
        assert_eq!(
            u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize,
            new_header
        );
        let name = u32::from_be_bytes(
            bytes[FIRST_MESSAGE * 4..FIRST_MESSAGE * 4 + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        assert!(bytes[name..].starts_with(b"Gold jewel egg\0"));
        assert_eq!(prs::decode(&prs::encode(&bytes)).unwrap(), bytes);
    }
    #[test]
    fn incompatible_or_truncated_message_tables_are_rejected() {
        assert!(append_messages(&[], &message_strings()).is_err());
        let mut bytes = original_messages();
        bytes[0..4].copy_from_slice(&1u32.to_be_bytes());
        assert!(append_messages(&bytes, &message_strings()).is_err());
    }
}

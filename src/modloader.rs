//! SA2 Mod Loader ABI, translated from `SA2ModInfo.h` (ModLoaderVer 16).
//! Only the parts we use are defined; layouts must match the C++ headers exactly.

use std::ffi::c_void;
use std::ptr::null;

pub const MOD_LOADER_VER: i32 = 16;

/// `struct ModInfo`, exported as `SA2ModInfo`.
#[repr(C)]
pub struct ModInfo {
    pub version: i32,
    /// Deprecated init pointer; we export `Init` instead.
    pub init: *const c_void,
    pub patches: *const c_void,
    pub patch_count: i32,
    pub jumps: *const c_void,
    pub jump_count: i32,
    pub calls: *const c_void,
    pub call_count: i32,
    pub pointers: *const c_void,
    pub pointer_count: i32,
}

// SAFETY: all pointers are null and never written.
unsafe impl Sync for ModInfo {}

impl ModInfo {
    pub const fn new(version: i32) -> Self {
        Self {
            version,
            init: null(),
            patches: null(),
            patch_count: 0,
            jumps: null(),
            jump_count: 0,
            calls: null(),
            call_count: 0,
            pointers: null(),
            pointer_count: 0,
        }
    }
}

type Fn0 = *const c_void;

/// `struct HelperFunctions`, in header order up to the debug-text functions (version >= 8).
/// Only the fields we call are typed; the rest are placeholders that keep the offsets right.
#[repr(C)]
pub struct HelperFunctions {
    pub version: i32,
    _register_start_position: Fn0,
    _clear_start_position_list: Fn0,
    _register_2p_intro_position: Fn0,
    _clear_2p_intro_position_list: Fn0,
    _get_main_save_path: Fn0,
    _get_chao_save_path: Fn0,
    _register_end_position: Fn0,
    _clear_end_position_list: Fn0,
    _register_mission23_end_position: Fn0,
    _clear_mission23_end_position_list: Fn0,
    _hook_export: Fn0,
    _get_replaceable_path: Fn0,
    _replace_file: Fn0,
    _set_window_title: Fn0,
    /// Font size in pixels (default 12).
    pub set_debug_font_size: unsafe extern "C" fn(f32),
    /// ARGB color (default 0xFFBFBFBF).
    pub set_debug_font_color: unsafe extern "C" fn(i32),
    /// Queues one line for this frame at `NJM_LOCATION(x, y)` (cells of the font size).
    pub display_debug_string: unsafe extern "C" fn(i32, *const u8),
}

/// The first version with the debug-text functions.
pub const DEBUG_TEXT_VERSION: i32 = 8;

// Layout check for the real 32-bit target (host builds are only for type-checking).
#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<ModInfo>() == 40);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(std::mem::offset_of!(HelperFunctions, display_debug_string) == 0x44);

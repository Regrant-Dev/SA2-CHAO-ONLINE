//! Patching the game's code: redirecting its `call rel32` instructions to our functions, and
//! swapping a few code bytes (guarded by what they must hold first).

/// Points the `call` at `site` to `target` and returns the function it called before, or `None`
/// if `site` isn't a `call rel32` (or the page can't be made writable).
#[cfg(windows)]
pub fn redirect_call(site: usize, target: usize) -> Option<usize> {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualProtect(address: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    }
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;

    // SAFETY: patches the game's own code at a fixed address; checked to be a `call rel32`.
    unsafe {
        let code = site as *mut u8;
        if code.read() != 0xE8 {
            return None;
        }
        let next = site + 5;
        let original = next.wrapping_add((code.add(1) as *const i32).read_unaligned() as usize);
        let mut old = 0;
        if VirtualProtect(code.cast(), 5, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return None;
        }
        (code.add(1) as *mut i32).write_unaligned(target.wrapping_sub(next) as i32);
        VirtualProtect(code.cast(), 5, old, &mut old);
        Some(original)
    }
}

#[cfg(not(windows))]
pub fn redirect_call(_site: usize, _target: usize) -> Option<usize> {
    None
}

/// Writes `bytes` over the game's code at `address` if it currently holds `expected`.
/// Returns whether the code now holds `bytes`.
#[cfg(windows)]
pub fn replace_code(address: usize, expected: &[u8], bytes: &[u8]) -> bool {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualProtect(address: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    }
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    debug_assert_eq!(expected.len(), bytes.len());

    // SAFETY: the game's own code at a fixed address; checked to hold `expected` first.
    unsafe {
        let code = std::slice::from_raw_parts(address as *const u8, bytes.len());
        if code == bytes {
            return true;
        }
        if code != expected {
            return false;
        }
        let mut old = 0;
        if VirtualProtect(
            address as *mut c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        ) == 0
        {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
        VirtualProtect(address as *mut c_void, bytes.len(), old, &mut old);
        true
    }
}

#[cfg(not(windows))]
pub fn replace_code(_address: usize, _expected: &[u8], _bytes: &[u8]) -> bool {
    false
}

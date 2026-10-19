//! Keyboard input for the F9 menu, read with GetAsyncKeyState and only while the game window
//! is focused.

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn GetAsyncKeyState(key: i32) -> i16;
        pub fn GetForegroundWindow() -> *mut c_void;
        pub fn GetWindowThreadProcessId(window: *mut c_void, process_id: *mut u32) -> u32;
        pub fn EnumThreadWindows(
            thread: u32,
            callback: extern "system" fn(*mut c_void, isize) -> i32,
            param: isize,
        ) -> i32;
        pub fn IsWindowVisible(window: *mut c_void) -> i32;
        pub fn IsIconic(window: *mut c_void) -> i32;
        pub fn GetClientRect(window: *mut c_void, rect: *mut [i32; 4]) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetCurrentProcessId() -> u32;
        pub fn GetCurrentThreadId() -> u32;
    }
}

pub const VK_F8: i32 = 0x77;
pub const VK_F9: i32 = 0x78;

/// Edge-triggered key: `pressed()` is true once per key press.
pub struct Hotkey {
    key: i32,
    down: bool,
    /// Frames it has been held, as of the last `pressed()`.
    held: u32,
}

/// Held this many frames, a key repeats (`repeating`) every `REPEAT_EVERY` frames.
const REPEAT_AFTER: u32 = 20;
const REPEAT_EVERY: u32 = 3;

impl Hotkey {
    pub const fn new(key: i32) -> Self {
        Self {
            key,
            down: false,
            held: 0,
        }
    }

    pub fn key(&self) -> i32 {
        self.key
    }

    pub fn pressed(&mut self) -> bool {
        let now = key_down(self.key);
        let pressed = now && !self.down;
        self.down = now;
        self.held = if now { self.held.saturating_add(1) } else { 0 };
        pressed
    }

    /// Held long enough to repeat, and repeating this frame (as of the last `pressed()`).
    pub fn repeating(&self) -> bool {
        self.held > REPEAT_AFTER && (self.held - REPEAT_AFTER).is_multiple_of(REPEAT_EVERY)
    }
}

/// The game's window is the foreground window.
#[cfg(windows)]
pub fn game_focused() -> bool {
    // SAFETY: plain Win32 calls with valid arguments.
    unsafe {
        let mut pid = 0;
        win::GetWindowThreadProcessId(win::GetForegroundWindow(), &mut pid);
        pid == win::GetCurrentProcessId()
    }
}

#[cfg(not(windows))]
pub fn game_focused() -> bool {
    false
}

/// The game's window can be drawn to: it isn't minimized and its client area isn't empty (a
/// minimized window gives a zero-size swapchain, and presenting to it can block). Call on the
/// game thread (it owns the window).
pub fn game_window_drawable() -> bool {
    let (iconic, size) = game_window_state();
    !iconic && size.0 > 0 && size.1 > 0
}

/// The game thread's window: (minimized, client width and height). None found reads as
/// minimized. Call on the game thread.
#[cfg(windows)]
pub fn game_window_state() -> (bool, (i32, i32)) {
    extern "system" fn first_visible(window: *mut std::ffi::c_void, out: isize) -> i32 {
        // SAFETY: plain Win32 calls; `out` is the address of the caller's local.
        unsafe {
            if win::IsWindowVisible(window) != 0 {
                *(out as *mut usize) = window as usize;
                return 0;
            }
        }
        1
    }
    let mut window = 0usize;
    // SAFETY: plain Win32 calls with valid arguments; the callback writes only `window`.
    unsafe {
        win::EnumThreadWindows(
            win::GetCurrentThreadId(),
            first_visible,
            &raw mut window as isize,
        );
        if window == 0 {
            return (true, (0, 0));
        }
        let window = window as *mut std::ffi::c_void;
        let mut rect = [0i32; 4];
        if win::GetClientRect(window, &mut rect) == 0 {
            rect = [0; 4];
        }
        (
            win::IsIconic(window) != 0,
            (rect[2] - rect[0], rect[3] - rect[1]),
        )
    }
}

#[cfg(not(windows))]
pub fn game_window_state() -> (bool, (i32, i32)) {
    (true, (0, 0))
}

#[cfg(windows)]
pub fn key_down(key: i32) -> bool {
    // SAFETY: plain Win32 call with a valid key code.
    game_focused() && unsafe { win::GetAsyncKeyState(key) as u16 & 0x8000 != 0 }
}

#[cfg(not(windows))]
pub fn key_down(_key: i32) -> bool {
    false
}

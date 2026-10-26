//! On-screen text: toasts (connect/join/leave messages) and name tags above other players (a
//! Chao's with a padlock when it's locked against us).
//!
//! Drawn with the Mod Loader's debug text (`DisplayDebugString`): one queued line per call,
//! drawn at the end of the frame, on a grid of font-size cells with square glyphs.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::modloader::{DEBUG_TEXT_VERSION, HelperFunctions};

/// Real window/back-buffer size in pixels (floats; the Mod Loader keeps them current and
/// positions debug text in these pixels). 0x2670328/C is the game's 640x480 virtual size.
const SCREEN_WIDTH: usize = 0x0174_BA64;
const SCREEN_HEIGHT: usize = 0x0174_BA68;

const TOAST_TIME: Duration = Duration::from_secs(5);
const MAX_TOASTS: usize = 4;
/// Mod Loader default font size, restored after we draw.
const DEFAULT_FONT: f32 = 12.0;
const TOAST_COLOR: i32 = 0xFFFF_FFFFu32 as i32;
/// Routine notices (`note`) are drawn this much smaller than toasts.
const NOTE_SCALE: f32 = 0.6;
const TAG_COLOR: i32 = 0xFFE0_E8FFu32 as i32;

struct Hud {
    helpers: usize,
    /// Text, when it came, and whether it's a small routine notice (`note`).
    toasts: VecDeque<(String, Instant, bool)>,
}

static HUD: Mutex<Hud> = Mutex::new(Hud {
    helpers: 0,
    toasts: VecDeque::new(),
});

/// Remembers the Mod Loader helpers (lives for the whole process).
pub fn init(helpers: &HelperFunctions) {
    if helpers.version >= DEBUG_TEXT_VERSION {
        HUD.lock().unwrap_or_else(|e| e.into_inner()).helpers = helpers as *const _ as usize;
        rects::install();
    }
}

/// Shows `text` for a few seconds (newest at the bottom).
pub fn toast(text: impl Into<String>) {
    push(text.into(), false);
}

/// A routine notice (an autosave): like `toast`, smaller.
pub fn note(text: impl Into<String>) {
    push(text.into(), true);
}

fn push(text: String, small: bool) {
    let mut hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    hud.toasts.push_back((text, Instant::now(), small));
    while hud.toasts.len() > MAX_TOASTS {
        hud.toasts.pop_front();
    }
}

/// Only printable ASCII: the debug font has no other glyphs, and the string goes to C.
fn printable(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = text
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c as u8
            } else {
                b'?'
            }
        })
        .collect();
    out.push(0);
    out
}

fn screen_height() -> f32 {
    // SAFETY: static float in the game's data section.
    let h = unsafe { (SCREEN_HEIGHT as *const f32).read_volatile() };
    if h.is_finite() && h >= 240.0 {
        h
    } else {
        480.0
    }
}

pub fn screen_size() -> (f32, f32) {
    // SAFETY: static float in the game's data section.
    let w = unsafe { (SCREEN_WIDTH as *const f32).read_volatile() };
    let w = if w.is_finite() && w >= 320.0 {
        w
    } else {
        640.0
    };
    (w, screen_height())
}

/// Queues `text` with its top-left corner at pixel (x, y) in `size`-pixel cells.
fn draw(hud: &Hud, x: f32, y: f32, size: f32, color: i32, text: &str) {
    draw_raw(hud, x, y, size, color, &printable(text));
}

/// Like `draw`, for a NUL-terminated byte string (any glyph of the 16x16 debug font).
fn draw_raw(hud: &Hud, x: f32, y: f32, size: f32, color: i32, text: &[u8]) {
    if hud.helpers == 0 {
        return;
    }
    // SAFETY: `helpers` points at the Mod Loader's HelperFunctions (version >= 8 checked).
    let helpers = unsafe { &*(hud.helpers as *const HelperFunctions) };
    let col = (x / size).round().clamp(0.0, 4000.0) as i32;
    let row = (y / size).round().clamp(0.0, 4000.0) as i32;
    // SAFETY: Mod Loader API; the string is NUL-terminated and copied during the call.
    unsafe {
        (helpers.set_debug_font_size)(size);
        (helpers.set_debug_font_color)(color);
        (helpers.display_debug_string)((col << 16) | row, text.as_ptr());
        (helpers.set_debug_font_size)(DEFAULT_FONT);
        (helpers.set_debug_font_color)(0xFFBF_BFBFu32 as i32);
    }
}

/// Call every frame: draws the current toasts in the top-left corner.
pub fn on_frame() {
    let mut hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    while hud
        .toasts
        .front()
        .is_some_and(|(_, t, _)| now.duration_since(*t) > TOAST_TIME)
    {
        hud.toasts.pop_front();
    }
    let size = (screen_height() / 45.0).round().max(10.0);
    let mut y = size * 1.5;
    for (text, _, small) in &hud.toasts {
        let size = if *small {
            (size * NOTE_SCALE).round().max(10.0)
        } else {
            size
        };
        draw(&hud, size, y, size, TOAST_COLOR, text);
        y += size * 1.2;
    }
}

/// Draws one line of text with its top-left at pixel (x, y), for menus.
pub fn text(x: f32, y: f32, size: f32, color: i32, line: &str) {
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    draw(&hud, x, y, size, color, line);
}

const PANEL_COLOR: u32 = 0xFF10_1420;
const PANEL_BORDER: u32 = 0xFF50_6080;

/// An opaque dark panel of `cols` x `rows` text cells at pixel (x, y), with a thin border.
/// Filled just before the debug text is drawn this frame, so it sits under all text.
pub fn panel(x: f32, y: f32, size: f32, cols: usize, rows: usize) {
    let x2 = x + cols as f32 * size;
    let y2 = y + rows as f32 * size;
    let b = (size / 8.0).round().max(1.0);
    let rect = |x1: f32, y1: f32, x2: f32, y2: f32| [x1, y1, x2, y2].map(|v| v.round() as i32);
    rects::push(rect(x - b, y - b, x2 + b, y2 + b), PANEL_BORDER);
    rects::push(rect(x, y, x2, y2), PANEL_COLOR);
}

/// A filled rectangle (x1, y1)..(x2, y2) in pixels, under this frame's text (menu bars and
/// highlights).
pub fn rect(x1: f32, y1: f32, x2: f32, y2: f32, color: u32) {
    rects::push([x1, y1, x2, y2].map(|v| v.round() as i32), color);
}

/// A gauge (`fraction` 0..1, clamped) with its fill in `color` (HP bars).
pub fn bar(x: f32, y: f32, width: f32, height: f32, fraction: f32, color: u32) {
    let rect = |right: f32| [x, y, right, y + height].map(|v| v.round() as i32);
    rects::push(rect(x + width), 0xFF30_3A48);
    if fraction.is_finite() && fraction > 0.0 {
        rects::push(rect(x + width * fraction.min(1.0)), color);
    }
}

/// The MULTIPLAYER label: (fill, border, text), lit up while the cursor is on it like the
/// game's own buttons, dimmer otherwise.
const LABEL_SELECTED: (u32, u32, u32) = (0xFFF0_C030, 0xFFFF_FFFF, 0xFF0A_1430);
const LABEL_IDLE: (u32, u32, u32) = (0xFF0A_1430, 0xFF8A_6A20, 0xFFC8_A848);

/// The game's 640x480 screen space (menus, its projection) on the real screen: the largest 4:3
/// area that fits, centred. Returns (scale, x offset, y offset).
pub fn virtual_screen() -> (f32, f32, f32) {
    let (sw, sh) = screen_size();
    let scale = (sw / 640.0).min(sh / 480.0);
    (
        scale,
        (sw - 640.0 * scale) / 2.0,
        (sh - 480.0 * scale) / 2.0,
    )
}

/// A point of the game's 640x480 space on the real screen.
pub fn from_virtual(x: f32, y: f32) -> (f32, f32) {
    let (scale, ox, oy) = virtual_screen();
    (ox + x * scale, oy + y * scale)
}

/// A menu button of our own: gold when `selected` (the cursor is on it), dark otherwise, centred on
/// (cx, cy) and `w` x `h` big in the game's 640x480 space (e.g. over the main menu's 1P entry).
pub fn menu_label(cx: f32, cy: f32, w: f32, h: f32, text: &str, selected: bool) {
    let (fill, border, color) = if selected { LABEL_SELECTED } else { LABEL_IDLE };
    let (scale, _, _) = virtual_screen();
    let (x1, y1) = from_virtual(cx - w / 2.0, cy - h / 2.0);
    let (x2, y2) = (x1 + w * scale, y1 + h * scale);
    let b = (3.0 * scale).round().max(2.0);
    let r = |a: f32, b: f32, c: f32, d: f32| [a, b, c, d].map(|v| v.round() as i32);
    rects::push(r(x1 - b, y1 - b, x2 + b, y2 + b), border);
    rects::push(r(x1, y1, x2, y2), fill);
    let fit = (x2 - x1) * 0.9 / text.chars().count().max(1) as f32;
    let size = (h * scale * 0.55).min(fit).round().max(10.0);
    let width = size * text.chars().count() as f32;
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    draw(
        &hud,
        (x1 + x2 - width) / 2.0,
        (y1 + y2 - size) / 2.0,
        size,
        color as i32,
        text,
    );
}

/// A comfortable text size for menus at the current resolution.
pub fn menu_font() -> f32 {
    (screen_height() / 40.0).round().max(10.0)
}

/// Large text centred a little above the middle of the screen (start-together messages).
pub fn center(line: &str) {
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    let (w, h) = screen_size();
    // As big as fits the screen's width, at most the usual size.
    let fit = w * 0.95 / line.chars().count().max(1) as f32;
    let size = (h / 45.0).min(fit).round().max(8.0);
    let width = size * line.chars().count() as f32;
    let (x, y) = ((w - width) / 2.0, h * 0.3);
    backdrop(x, y, x + width, y + size, size);
    draw(&hud, x, y, size, TOAST_COLOR, line);
}

/// The menu's dark panel behind text spanning (x1, y1)..(x2, y2), with a margin of half a
/// character (`size`).
fn backdrop(x1: f32, y1: f32, x2: f32, y2: f32, size: f32) {
    let m = size * 0.5;
    let b = (size / 8.0).round().max(1.0);
    let rect = |x1: f32, y1: f32, x2: f32, y2: f32| [x1, y1, x2, y2].map(|v| v.round() as i32);
    rects::push(
        rect(x1 - m - b, y1 - m - b, x2 + m + b, y2 + m + b),
        PANEL_BORDER,
    );
    rects::push(rect(x1 - m, y1 - m, x2 + m, y2 + m), PANEL_COLOR);
}

/// Two lines of large text, centred like `center` (the second a little smaller).
pub fn center2(first: &str, second: &str) {
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    let (w, h) = screen_size();
    let top = h * 0.3;
    let mut y = top;
    let mut lines = Vec::new();
    for (line, big) in [(first, 45.0), (second, 55.0)] {
        let fit = w * 0.95 / line.chars().count().max(1) as f32;
        let size = (h / big).min(fit).round().max(8.0);
        let width = size * line.chars().count() as f32;
        lines.push(((w - width) / 2.0, y, size, line, width));
        y += size * 1.6;
    }
    // One panel behind both lines.
    let left = lines.iter().map(|l| l.0).fold(f32::MAX, f32::min);
    let right = lines.iter().map(|l| l.0 + l.4).fold(f32::MIN, f32::max);
    let (last_y, last_size) = lines.last().map_or((top, 0.0), |l| (l.1, l.2));
    let size = lines.first().map_or(10.0, |l| l.2);
    backdrop(left, top, right, last_y + last_size, size);
    for (x, y, size, line, _) in lines {
        draw(&hud, x, y, size, TOAST_COLOR, line);
    }
}

/// Two small lines centred near the top (a garden's wild area as it loads): out of the way of
/// the game, the second a little smaller.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub fn top_title(first: &str, second: &str) {
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    let (w, h) = screen_size();
    let top = h * 0.06;
    let mut y = top;
    let mut lines = Vec::new();
    for (line, small) in [(first, 60.0), (second, 80.0)] {
        let fit = w * 0.6 / line.chars().count().max(1) as f32;
        let size = (h / small).min(fit).round().max(8.0);
        let width = size * line.chars().count() as f32;
        lines.push(((w - width) / 2.0, y, size, line, width));
        y += size * 1.4;
    }
    let left = lines.iter().map(|l| l.0).fold(f32::MAX, f32::min);
    let right = lines.iter().map(|l| l.0 + l.4).fold(f32::MIN, f32::max);
    let (last_y, last_size) = lines.last().map_or((top, 0.0), |l| (l.1, l.2));
    let size = lines.last().map_or(8.0, |l| l.2);
    backdrop(left, top, right, last_y + last_size, size);
    for (x, y, size, line, _) in lines {
        draw(&hud, x, y, size, TOAST_COLOR, line);
    }
}

/// A line at the top right (race position).
pub fn corner(line: &str) {
    let (w, h) = screen_size();
    let size = (h / 72.0)
        .min(w * 0.6 / line.chars().count().max(1) as f32)
        .round()
        .max(8.0);
    let width = size * line.chars().count() as f32;
    let (x, y) = (w - width - size * 2.0, size * 2.0);
    backdrop(x, y, x + width, y + size, size);
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    draw(&hud, x, y, size, TOAST_COLOR, line);
}

/// The chat line being typed, bottom left on the menu panel.
pub fn chat_line(line: &str) {
    let (w, h) = screen_size();
    let size = (h / 45.0)
        .min(w * 0.9 / line.chars().count().max(1) as f32)
        .round()
        .max(8.0);
    let (x, y) = (size * 2.0, h * 0.82);
    let width = size * line.chars().count() as f32;
    backdrop(x, y, x + width, y + size, size);
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    draw(&hud, x, y, size, TOAST_COLOR, line);
}

const BAR_FILL: u32 = 0xFFF0_C030;

/// A line and a progress bar (`fraction` 0..1) centred near the bottom of the screen
/// (shared skins coming down, on the menus).
pub fn progress(line: &str, fraction: f32) {
    let (w, h) = screen_size();
    let size = (h / 50.0).round().max(10.0);
    let cols = line.chars().count().max(28) + 2;
    let width = cols as f32 * size;
    let (x, y) = ((w - width) / 2.0, h * 0.84);
    panel(x, y, size, cols, 3);
    let rect = |x1: f32, y1: f32, x2: f32, y2: f32| [x1, y1, x2, y2].map(|v| v.round() as i32);
    let (bx1, bx2) = (x + size, x + width - size);
    let (by1, by2) = (y + size * 1.7, y + size * 2.4);
    rects::push(rect(bx1, by1, bx2, by2), PANEL_BORDER);
    let filled = bx1 + (bx2 - bx1) * fraction.clamp(0.0, 1.0);
    if filled > bx1 {
        rects::push(rect(bx1, by1, filled, by2), BAR_FILL);
    }
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    draw(&hud, x + size, y + size * 0.4, size, TOAST_COLOR, line);
}

/// A line of text centred near the bottom of the screen (hints on Stage Select).
pub fn banner(line: &str) {
    let (w, h) = screen_size();
    let size = (h / 50.0).round().max(10.0);
    let width = size * line.chars().count() as f32;
    let (x, y) = ((w - width) / 2.0, h * 0.9);
    panel(x - size, y - size * 0.5, size, line.chars().count() + 2, 2);
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    draw(&hud, x, y, size, TOAST_COLOR, line);
}

/// Draws `name` centred above screen point (x, y), `scale` times the normal size (far players
/// get smaller tags), with a padlock after it when `locked` (a Chao locked against us). Called
/// from the puppet display.
pub fn name_tag(x: f32, y: f32, scale: f32, name: &str, locked: bool) {
    let hud = HUD.lock().unwrap_or_else(|e| e.into_inner());
    let size = (screen_height() / 60.0 * scale).round().max(8.0);
    let chars = name.chars().count();
    // The lock takes a cell of its own, after a space.
    let cells = chars + if locked { 2 } else { 0 };
    let width = size * cells as f32;
    // The debug text sits on a grid of `size` cells: the lock goes where its cell will be.
    let (left, top) = (x - width / 2.0, y - size);
    let (col, row) = ((left / size).round(), (top / size).round());
    draw(&hud, left, top, size, TAG_COLOR, name);
    if locked {
        for (rect, color) in padlock((col + chars as f32 + 1.0) * size, row * size, size) {
            rects::push(rect, color);
        }
    }
}

/// The padlock's body and shackle, and its outline and keyhole.
const LOCK_COLOR: u32 = 0xFFF0_C030;
const LOCK_DARK: u32 = 0xFF10_1420;

/// A padlock filling the `size`-pixel text cell at pixel (x, y), as rectangles in drawing order
/// (`rects::push`): a dark outline, then the shackle (an arch over the body's middle) and the
/// body, then the keyhole. Like a glyph, it stands on the cell's baseline.
fn padlock(x: f32, y: f32, size: f32) -> Vec<([i32; 4], u32)> {
    let px = |f: f32| f * size;
    // The shackle's bars, at least a pixel; the outline round everything.
    let bar = (size * 0.12).round().max(1.0);
    let edge = (size / 14.0).round().max(1.0);
    let (bx1, bx2, by1, by2) = (x + px(0.18), x + px(0.82), y + px(0.46), y + px(0.92));
    let (sx1, sx2, sy1) = (x + px(0.3), x + px(0.7), y + px(0.1));
    let shackle = [
        [sx1, sy1, sx1 + bar, by1],
        [sx2 - bar, sy1, sx2, by1],
        [sx1, sy1, sx2, sy1 + bar],
    ];
    let body = [bx1, by1, bx2, by2];
    let round = |r: [f32; 4]| r.map(|v| v.round() as i32);
    let grow = |r: [f32; 4]| [r[0] - edge, r[1] - edge, r[2] + edge, r[3] + edge];
    let (kw, kx) = ((size * 0.12).round().max(1.0), x + px(0.5));
    let keyhole = [kx - kw / 2.0, y + px(0.6), kx + kw / 2.0, y + px(0.8)];
    let mut out: Vec<([i32; 4], u32)> = shackle
        .iter()
        .chain([&body])
        .map(|&r| (round(grow(r)), LOCK_DARK))
        .collect();
    out.extend(
        shackle
            .iter()
            .chain([&body])
            .map(|&r| (round(r), LOCK_COLOR)),
    );
    out.push((round(keyhole), LOCK_DARK));
    out
}

/// Solid rectangles filled with Direct3D 9 `Clear` right before the Mod Loader draws its debug
/// text (we take over the game's call at 0x433FF7, which the loader points at its text drawing,
/// and call through to it afterwards). Queued during the frame, drawn and dropped once.
mod rects {
    use std::sync::Mutex;

    static QUEUE: Mutex<Vec<([i32; 4], u32)>> = Mutex::new(Vec::new());

    /// Queues a rectangle `[x1, y1, x2, y2]` in back-buffer pixels.
    pub fn push(rect: [i32; 4], color: u32) {
        let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() < 256 {
            queue.push((rect, color));
        }
    }

    #[cfg(target_arch = "x86")]
    pub use hook::install;

    #[cfg(not(target_arch = "x86"))]
    pub fn install() {
        QUEUE.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    #[cfg(target_arch = "x86")]
    mod hook {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use super::QUEUE;
        use crate::log;

        /// `call` to the end-of-scene 2D work (the Mod Loader's debug text hook).
        const CALL_SITE: usize = 0x0043_3FF7;
        /// `RenderDevice_DX9*`; its `IDirect3DDevice9*` is at +0x38.
        const RENDER_DEVICE: usize = 0x01A5_57C0;
        const DEVICE_OFFSET: usize = 0x38;
        // IDirect3DDevice9 vtable slots.
        const CLEAR: usize = 43;
        const SET_VIEWPORT: usize = 47;
        const GET_VIEWPORT: usize = 48;
        const SET_RENDER_STATE: usize = 57;
        const GET_RENDER_STATE: usize = 58;
        const D3DRS_SCISSORTESTENABLE: u32 = 174;
        const D3DCLEAR_TARGET: u32 = 1;

        static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

        #[repr(C)]
        #[derive(Default)]
        struct Viewport {
            x: u32,
            y: u32,
            width: u32,
            height: u32,
            min_z: f32,
            max_z: f32,
        }

        pub fn install() {
            let target = pre_end_scene as extern "C" fn() -> f64 as usize;
            match crate::patch::redirect_call(CALL_SITE, target) {
                Some(original) => {
                    ORIGINAL.store(original, Ordering::SeqCst);
                    log!("hud: menu background hook installed");
                }
                None => log!("hud: can't hook {CALL_SITE:#x}; no menu background"),
            }
        }

        /// Returns what the original returns (the game pops a float after this call).
        extern "C" fn pre_end_scene() -> f64 {
            let queue = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()));
            if !queue.is_empty() {
                // SAFETY: called by the game inside BeginScene/EndScene with a live device.
                unsafe { fill(&queue) };
            }
            let original = ORIGINAL.load(Ordering::SeqCst);
            // SAFETY: the function the game called here before we were installed.
            let result =
                unsafe { std::mem::transmute::<usize, extern "C" fn() -> f64>(original)() };
            // The game loop gets here every frame, paused or not (0x433EE0); the Mod Loader's
            // OnFrame doesn't come while the game is paused.
            crate::loop_frame();
            result
        }

        unsafe fn fill(queue: &[([i32; 4], u32)]) {
            unsafe {
                let render = (RENDER_DEVICE as *const usize).read();
                if render == 0 {
                    return;
                }
                let device = ((render + DEVICE_OFFSET) as *const usize).read();
                if device == 0 {
                    return;
                }
                let vtable = (device as *const *const usize).read();
                let slot = |i: usize| vtable.add(i).read();
                type GetViewport = extern "system" fn(usize, *mut Viewport) -> i32;
                type SetViewport = extern "system" fn(usize, *const Viewport) -> i32;
                type GetRenderState = extern "system" fn(usize, u32, *mut u32) -> i32;
                type SetRenderState = extern "system" fn(usize, u32, u32) -> i32;
                type Clear =
                    extern "system" fn(usize, u32, *const [i32; 4], u32, u32, f32, u32) -> i32;
                let get_viewport: GetViewport = std::mem::transmute(slot(GET_VIEWPORT));
                let set_viewport: SetViewport = std::mem::transmute(slot(SET_VIEWPORT));
                let get_state: GetRenderState = std::mem::transmute(slot(GET_RENDER_STATE));
                let set_state: SetRenderState = std::mem::transmute(slot(SET_RENDER_STATE));
                let clear: Clear = std::mem::transmute(slot(CLEAR));

                let mut saved = Viewport::default();
                if get_viewport(device, &mut saved) < 0 {
                    return;
                }
                let mut scissor = 0;
                get_state(device, D3DRS_SCISSORTESTENABLE, &mut scissor);
                // Clear only reaches inside the viewport: use the whole back buffer.
                let (w, h) = super::super::screen_size();
                let full = Viewport {
                    width: w as u32,
                    height: h as u32,
                    max_z: 1.0,
                    ..Viewport::default()
                };
                set_viewport(device, &full);
                set_state(device, D3DRS_SCISSORTESTENABLE, 0);
                for (rect, color) in queue {
                    let r = [
                        rect[0].clamp(0, w as i32),
                        rect[1].clamp(0, h as i32),
                        rect[2].clamp(0, w as i32),
                        rect[3].clamp(0, h as i32),
                    ];
                    if r[2] > r[0] && r[3] > r[1] {
                        clear(device, 1, &r, D3DCLEAR_TARGET, *color, 1.0, 0);
                    }
                }
                set_state(device, D3DRS_SCISSORTESTENABLE, scissor);
                set_viewport(device, &saved);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LOCK_COLOR, LOCK_DARK, padlock, printable};

    #[test]
    fn a_padlock_fits_its_cell_with_the_shackle_over_the_body() {
        for size in [8.0, 18.0, 36.0] {
            let rects = padlock(100.0, 50.0, size);
            let cell = [100, 50, 100 + size as i32, 50 + size as i32];
            // Outline (shackle bars and body), the same filled in, the keyhole.
            assert_eq!(rects.len(), 9);
            assert!(rects[..4].iter().all(|r| r.1 == LOCK_DARK));
            assert!(rects[4..8].iter().all(|r| r.1 == LOCK_COLOR));
            for ([x1, y1, x2, y2], _) in &rects {
                assert!(x1 < x2 && y1 < y2, "{size}: {rects:?}");
                assert!(*x1 >= cell[0] - 1 && *x2 <= cell[2] + 1, "{size}");
                assert!(*y1 >= cell[1] - 1 && *y2 <= cell[3] + 1, "{size}");
            }
            let (left, right, top, body) = (rects[4].0, rects[5].0, rects[6].0, rects[7].0);
            // The arch stands on the body, inside its width, above it.
            assert!(left[0] > body[0] && right[2] < body[2], "{size}");
            assert_eq!((left[3], right[3]), (body[1], body[1]));
            assert!(top[1] < body[1] && top[0] == left[0] && top[2] == right[2]);
            // The keyhole is inside the body.
            let key = rects[8].0;
            assert!(key[0] > body[0] && key[2] < body[2] && key[1] > body[1] && key[3] < body[3]);
        }
    }

    #[test]
    fn printable_replaces_non_ascii_and_terminates() {
        assert_eq!(printable("Hi é!"), b"Hi ?!\0");
        assert_eq!(printable(""), b"\0");
        assert_eq!(printable("a\nb"), b"a?b\0");
    }
}

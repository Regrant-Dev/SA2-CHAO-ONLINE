//! Text chat: T opens a line at the bottom of the screen, Enter sends it to everyone in the
//! lobby, Esc cancels. Messages show as toasts ("Mason: gg"). While typing, the game's controls
//! are off (letters are also game buttons) but the world keeps running.

use std::sync::Mutex;

use crate::game;
use crate::hud;
use crate::input::{self, Hotkey};

/// Longest message (the toast font is large; this fits one line on most screens).
pub const MAX_CHAT: usize = crate::net::protocol::MAX_CHAT;

const VK_T: i32 = 0x54;
const VK_BACK: i32 = 0x08;
const VK_RETURN: i32 = 0x0D;
const VK_SHIFT: i32 = 0x10;
const VK_ESCAPE: i32 = 0x1B;
const VK_SPACE: i32 = 0x20;
const VK_OEM_1: i32 = 0xBA; // ;:
const VK_OEM_COMMA: i32 = 0xBC; // ,<
const VK_OEM_MINUS: i32 = 0xBD; // -_
const VK_OEM_PERIOD: i32 = 0xBE; // .>
const VK_OEM_2: i32 = 0xBF; // /?
const VK_OEM_7: i32 = 0xDE; // '"

struct Chat {
    open: bool,
    text: String,
    keys: Vec<Hotkey>,
    outgoing: Vec<String>,
}

static CHAT: Mutex<Chat> = Mutex::new(Chat {
    open: false,
    text: String::new(),
    keys: Vec::new(),
    outgoing: Vec::new(),
});

impl Chat {
    fn pressed(&mut self, key: i32) -> bool {
        match self.keys.iter_mut().find(|k| k.key() == key) {
            Some(k) => k.pressed(),
            None => {
                let mut k = Hotkey::new(key);
                let p = k.pressed();
                self.keys.push(k);
                p
            }
        }
    }
}

/// Every key that types something, plus the editing keys.
fn keys() -> impl Iterator<Item = i32> {
    (0x30..=0x39).chain(0x41..=0x5A).chain([
        VK_SPACE,
        VK_OEM_1,
        VK_OEM_COMMA,
        VK_OEM_MINUS,
        VK_OEM_PERIOD,
        VK_OEM_2,
        VK_OEM_7,
        VK_BACK,
        VK_RETURN,
        VK_ESCAPE,
    ])
}

/// The character a key types (US layout).
fn typed(key: i32, shift: bool) -> Option<char> {
    Some(match (key, shift) {
        (0x31, true) => '!',
        (0x30..=0x39, false) => (b'0' + (key - 0x30) as u8) as char,
        (0x41..=0x5A, _) => {
            let c = (b'a' + (key - 0x41) as u8) as char;
            if shift { c.to_ascii_uppercase() } else { c }
        }
        (VK_SPACE, _) => ' ',
        (VK_OEM_1, false) => ';',
        (VK_OEM_1, true) => ':',
        (VK_OEM_COMMA, false) => ',',
        (VK_OEM_MINUS, false) => '-',
        (VK_OEM_MINUS, true) => '_',
        (VK_OEM_PERIOD, false) => '.',
        (VK_OEM_2, false) => '/',
        (VK_OEM_2, true) => '?',
        (VK_OEM_7, false) => '\'',
        (VK_OEM_7, true) => '"',
        _ => return None,
    })
}

/// Turns the game's controls off while typing; back on after, unless a room start holds us.
fn block_controls(block: bool) {
    if block {
        game::set_controls(false);
    } else if !crate::together::is_holding() {
        game::set_controls(true);
    }
}

/// Call every frame (after the F9 menu). `connected`: in a lobby.
pub fn on_frame(connected: bool, menu_open: bool) {
    let mut c = CHAT.lock().unwrap_or_else(|e| e.into_inner());
    if !c.open {
        // Keep the key states current so T doesn't count as pressed later.
        let t = c.pressed(VK_T);
        if t && connected && !menu_open && input::game_focused() {
            c.open = true;
            c.text.clear();
            // Swallow keys already down (the T itself).
            for key in keys() {
                let _ = c.pressed(key);
            }
            block_controls(true);
        }
        return;
    }
    if !connected || menu_open {
        c.open = false;
        block_controls(false);
        return;
    }
    if c.pressed(VK_ESCAPE) {
        c.open = false;
        block_controls(false);
        return;
    }
    if c.pressed(VK_RETURN) {
        let text = c.text.trim().to_string();
        if !text.is_empty() {
            c.outgoing.push(text.clone());
            hud::toast(format!("You: {text}"));
        }
        c.open = false;
        block_controls(false);
        return;
    }
    if c.pressed(VK_BACK) {
        c.text.pop();
    }
    let shift = input::key_down(VK_SHIFT);
    for key in keys() {
        if matches!(key, VK_BACK | VK_RETURN | VK_ESCAPE) {
            continue;
        }
        if c.pressed(key)
            && let Some(ch) = typed(key, shift)
            && c.text.len() < MAX_CHAT
        {
            c.text.push(ch);
        }
    }
    // Keep the controls off (a room start ending mustn't turn them back on mid-sentence).
    game::set_controls(false);
    hud::chat_line(&format!("Say: {}_", c.text));
}

/// The chat line is open (keys type into it).
pub fn typing() -> bool {
    CHAT.lock().unwrap_or_else(|e| e.into_inner()).open
}

/// Messages to send.
pub fn take_outgoing() -> Vec<String> {
    std::mem::take(&mut CHAT.lock().unwrap_or_else(|e| e.into_inner()).outgoing)
}

/// Network: `name` said `text`.
pub fn received(name: &str, text: &str) {
    hud::toast(format!("{name}: {text}"));
}

#[cfg(test)]
mod tests {
    use super::typed;

    #[test]
    fn typing() {
        assert_eq!(typed(0x41, false), Some('a'));
        assert_eq!(typed(0x41, true), Some('A'));
        assert_eq!(typed(0x31, true), Some('!'));
        assert_eq!(typed(0xBF, true), Some('?'));
        assert_eq!(typed(0x35, false), Some('5'));
        assert_eq!(typed(0x35, true), None);
    }
}

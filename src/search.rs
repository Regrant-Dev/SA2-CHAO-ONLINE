//! The long lists' keyboard search and quick scrolling (the link machine's pages and the battle
//! menus): letters typed while one is open filter its rows by name (case-insensitive, anywhere
//! in the name), shown as a "Search: abc" line; Backspace deletes a letter and Esc clears the
//! search (it goes back only when the search is empty). Left/Right on the D-pad and PgUp/PgDn
//! jump `JUMP` rows. Rows without a name ("Done", rules, messages) always show.
//!
//! While a list takes the keyboard (`take_keyboard`), the game's own key bindings (letters are
//! game buttons too) and the chat's T and the status balloon's C leave the letters to it: a list
//! ignores the menu buttons while a typing key (or Backspace, Enter or Esc) is down, and Enter
//! and Esc stand for A and B there.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::input::{self, Hotkey};

/// Rows a jump moves.
pub const JUMP: usize = 10;
/// Longest search.
const MAX_SEARCH: usize = 20;

/// The text being searched for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    text: String,
}

/// What a keyboard press did to a search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Nothing.
    None,
    /// The search changed.
    Changed,
    /// Esc (or a fresh Backspace) with nothing to clear or delete: back.
    Back,
}

impl Search {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// A row with `name` shows: one without a name always does.
    pub fn matches(&self, name: Option<&str>) -> bool {
        match name {
            None => true,
            Some(name) => {
                self.text.is_empty()
                    || name
                        .to_ascii_lowercase()
                        .contains(&self.text.to_ascii_lowercase())
            }
        }
    }

    /// The indices of the rows (by their names) that show.
    pub fn shown<'a>(&self, names: impl IntoIterator<Item = Option<&'a str>>) -> Vec<usize> {
        names
            .into_iter()
            .enumerate()
            .filter(|&(_, n)| self.matches(n))
            .map(|(i, _)| i)
            .collect()
    }

    /// After editing a filter, retain a matching named row or select the first match. Utility
    /// rows such as "Done" remain available, but mustn't steal focus from the search results.
    /// `rows` includes only rows that can be selected, with their original indices.
    pub fn focus<'a>(
        &self,
        cursor: usize,
        rows: impl IntoIterator<Item = (usize, Option<&'a str>)>,
    ) -> usize {
        let shown: Vec<_> = rows
            .into_iter()
            .filter(|&(_, name)| self.matches(name))
            .collect();
        if !self.is_empty() {
            let mut named = shown.iter().filter(|(_, name)| name.is_some());
            if let Some(&(first, _)) = named.next() {
                return if first == cursor || named.any(|&(i, _)| i == cursor) {
                    cursor
                } else {
                    first
                };
            }
        }
        settle(cursor, &shown.iter().map(|&(i, _)| i).collect::<Vec<_>>())
    }

    /// Applies this frame's keys: typed letters join, Backspace deletes one, Esc clears it.
    /// `back_on_erase`: a fresh Backspace with nothing to delete goes back (the link machine's).
    pub fn edit(&mut self, typed: &Typed, back_on_erase: bool) -> Edit {
        if typed.escape {
            if self.text.is_empty() {
                return Edit::Back;
            }
            self.text.clear();
            return Edit::Changed;
        }
        let mut edit = Edit::None;
        if typed.erase {
            if self.text.pop().is_some() {
                edit = Edit::Changed;
            } else if typed.erase_fresh && back_on_erase {
                return Edit::Back;
            }
        }
        for c in typed.text.chars() {
            // No leading spaces.
            if self.text.len() < MAX_SEARCH && !(c == ' ' && self.text.is_empty()) {
                self.text.push(c);
                edit = Edit::Changed;
            }
        }
        edit
    }
}

/// The row of `shown` (indices of the rows that show) nearest `cursor`, earlier ones first on a
/// tie; `cursor` when it shows or nothing does.
pub fn settle(cursor: usize, shown: &[usize]) -> usize {
    shown
        .iter()
        .copied()
        .min_by_key(|&i| (i.abs_diff(cursor), i > cursor))
        .unwrap_or(cursor)
}

/// Up (`forward` false) or down a row among `shown`, wrapping.
pub fn step(cursor: usize, shown: &[usize], forward: bool) -> usize {
    let n = shown.len();
    if n == 0 {
        return cursor;
    }
    match shown.iter().position(|&i| i == cursor) {
        Some(at) if forward => shown[(at + 1) % n],
        Some(at) => shown[(at + n - 1) % n],
        None => settle(cursor, shown),
    }
}

/// `by` rows up (`forward` false) or down among `shown`, stopping at the ends.
pub fn jump(cursor: usize, shown: &[usize], forward: bool, by: usize) -> usize {
    let n = shown.len();
    if n == 0 {
        return cursor;
    }
    let at = shown
        .iter()
        .position(|&i| i == cursor)
        .unwrap_or_else(|| shown.partition_point(|&i| i < cursor).min(n - 1));
    let to = if forward {
        (at + by).min(n - 1)
    } else {
        at.saturating_sub(by)
    };
    shown[to]
}

/// This frame's keyboard presses on a list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Typed {
    /// Letters, digits and the like typed (lower case).
    pub text: String,
    /// Backspace pressed or repeating, and pressed fresh this frame.
    pub erase: bool,
    pub erase_fresh: bool,
    pub escape: bool,
    pub enter: bool,
    /// Arrow-key navigation, including held-key repeat.
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// PgUp, PgDn (pressed or repeating).
    pub page_up: bool,
    pub page_down: bool,
    /// A typing key, Backspace, Enter or Esc is down: the menu buttons (which the game's own
    /// key bindings may press too) don't count this frame.
    pub held: bool,
}

const VK_BACK: i32 = 0x08;
const VK_RETURN: i32 = 0x0D;
const VK_ESCAPE: i32 = 0x1B;
const VK_SPACE: i32 = 0x20;
const VK_PRIOR: i32 = 0x21;
const VK_NEXT: i32 = 0x22;
const VK_LEFT: i32 = 0x25;
const VK_UP: i32 = 0x26;
const VK_RIGHT: i32 = 0x27;
const VK_DOWN: i32 = 0x28;
const VK_OEM_MINUS: i32 = 0xBD;
const VK_OEM_PERIOD: i32 = 0xBE;
const VK_OEM_7: i32 = 0xDE;

/// Every key that types into a search.
fn typing_keys() -> impl Iterator<Item = i32> {
    (0x30..=0x39)
        .chain(0x41..=0x5A)
        .chain([VK_SPACE, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_7])
}

/// The character a typing key types (US layout, lower case).
fn typed_char(key: i32) -> Option<char> {
    Some(match key {
        0x30..=0x39 => (b'0' + (key - 0x30) as u8) as char,
        0x41..=0x5A => (b'a' + (key - 0x41) as u8) as char,
        VK_SPACE => ' ',
        VK_OEM_MINUS => '-',
        VK_OEM_PERIOD => '.',
        VK_OEM_7 => '\'',
        _ => return None,
    })
}

/// Reads the keyboard for a list. Call it every frame (also while its list doesn't show), so
/// keys held from before don't count as pressed.
#[derive(Default)]
pub struct Keyboard {
    keys: Vec<Hotkey>,
}

impl Keyboard {
    fn key(&mut self, key: i32) -> &mut Hotkey {
        match self.keys.iter().position(|k| k.key() == key) {
            Some(i) => &mut self.keys[i],
            None => {
                self.keys.push(Hotkey::new(key));
                self.keys.last_mut().expect("just pushed")
            }
        }
    }

    fn pressed(&mut self, key: i32) -> bool {
        self.key(key).pressed()
    }

    /// Pressed, or held long enough to repeat.
    fn held(&mut self, key: i32) -> bool {
        let k = self.key(key);
        k.pressed() || k.repeating()
    }

    /// This frame's presses.
    pub fn read(&mut self) -> Typed {
        let mut t = Typed::default();
        for key in typing_keys() {
            if self.pressed(key)
                && let Some(c) = typed_char(key)
            {
                t.text.push(c);
            }
            t.held |= input::key_down(key);
        }
        t.held |= [VK_BACK, VK_RETURN, VK_ESCAPE]
            .into_iter()
            .any(input::key_down);
        let erase_fresh = self.pressed(VK_BACK);
        t.erase = erase_fresh || self.key(VK_BACK).repeating();
        t.erase_fresh = erase_fresh;
        t.escape = self.pressed(VK_ESCAPE);
        t.enter = self.pressed(VK_RETURN);
        t.up = self.held(VK_UP);
        t.down = self.held(VK_DOWN);
        t.left = self.held(VK_LEFT);
        t.right = self.held(VK_RIGHT);
        t.page_up = self.held(VK_PRIOR);
        t.page_down = self.held(VK_NEXT);
        t
    }
}

/// Who has a list open that takes the keyboard (a bit each).
static TAKEN: AtomicU8 = AtomicU8::new(0);

/// The lists that take the keyboard.
#[derive(Clone, Copy)]
// The link machine is only in the game build.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub enum Owner {
    LinkMachine = 1,
    Battle = 2,
    Menu = 4,
}

/// `owner`'s list is open (letters type into it) or not.
pub fn take_keyboard(owner: Owner, on: bool) {
    let bit = owner as u8;
    if on {
        TAKEN.fetch_or(bit, Ordering::Relaxed);
    } else {
        TAKEN.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// A list has the keyboard: letters type into its search, not into the chat or hotkeys.
pub fn keyboard_taken() -> bool {
    TAKEN.load(Ordering::Relaxed) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Typed {
        Typed {
            text: text.into(),
            ..Typed::default()
        }
    }

    #[test]
    fn a_search_matches_names_anywhere_in_any_case() {
        let mut s = Search::default();
        assert!(s.matches(Some("Grundel")) && s.matches(None));
        assert_eq!(s.edit(&typed("UND"), false), Edit::Changed);
        assert_eq!(s.text(), "UND");
        assert!(s.matches(Some("grundel")) && s.matches(Some("Grundelfin")));
        assert!(!s.matches(Some("Pebblet")));
        // Rows without a name always show.
        assert!(s.matches(None));
        let names = [Some("Pebblet"), Some("Grundel"), None, Some("Moss")];
        assert_eq!(s.shown(names), [1, 2]);
        // Backspace deletes one; Esc clears, then goes back.
        let erase = Typed {
            erase: true,
            erase_fresh: true,
            ..Typed::default()
        };
        assert_eq!(s.edit(&erase, true), Edit::Changed);
        assert_eq!(s.text(), "UN");
        let escape = Typed {
            escape: true,
            ..Typed::default()
        };
        assert_eq!(s.edit(&escape, false), Edit::Changed);
        assert!(s.is_empty());
        assert_eq!(s.edit(&escape, false), Edit::Back);
        // A fresh Backspace on an empty search goes back only where asked; a repeat never.
        assert_eq!(s.edit(&erase, false), Edit::None);
        assert_eq!(s.edit(&erase, true), Edit::Back);
        let repeat = Typed {
            erase: true,
            ..Typed::default()
        };
        assert_eq!(s.edit(&repeat, true), Edit::None);
        // No leading space, and a length limit.
        assert_eq!(s.edit(&typed(" a b"), false), Edit::Changed);
        assert_eq!(s.text(), "a b");
        s.edit(&typed(&"x".repeat(40)), false);
        assert_eq!(s.text().len(), MAX_SEARCH);
        assert_eq!(s.edit(&Typed::default(), true), Edit::None);
    }

    #[test]
    fn the_cursor_moves_and_jumps_among_the_rows_shown() {
        let all: Vec<usize> = (0..25).collect();
        assert_eq!(jump(0, &all, true, JUMP), 10);
        assert_eq!(jump(10, &all, true, JUMP), 20);
        assert_eq!(jump(20, &all, true, JUMP), 24);
        assert_eq!(jump(24, &all, false, JUMP), 14);
        assert_eq!(jump(3, &all, false, JUMP), 0);
        assert_eq!(step(24, &all, true), 0);
        assert_eq!(step(0, &all, false), 24);
        // Among some rows: by their place in the list shown.
        let shown = [2, 5, 9, 14, 20];
        assert_eq!(jump(2, &shown, true, 3), 14);
        assert_eq!(jump(14, &shown, true, 3), 20);
        assert_eq!(step(20, &shown, true), 2);
        assert_eq!(step(5, &shown, false), 2);
        // A cursor on a row that doesn't show goes to the nearest, earlier first.
        assert_eq!(settle(7, &shown), 5);
        assert_eq!(settle(8, &shown), 9);
        assert_eq!(settle(9, &shown), 9);
        assert_eq!(settle(0, &shown), 2);
        assert_eq!(step(7, &shown, true), 5);
        assert_eq!(jump(7, &shown, true, 1), 14);
        assert_eq!(jump(30, &shown, false, 1), 14);
        // Nothing shows: it stays.
        assert_eq!(
            (settle(4, &[]), step(4, &[], true), jump(4, &[], true, 10)),
            (4, 4, 4)
        );
    }

    #[test]
    fn typing_from_done_focuses_a_result_and_keeps_a_matching_selection() {
        let rows = [(0, Some("Plumtop")), (1, Some("Plover")), (2, None)];
        let mut s = Search::default();
        s.edit(&typed("pl"), false);
        assert_eq!(s.focus(2, rows), 0);
        assert_eq!(s.focus(1, rows), 1);
        s.edit(&typed("um"), false);
        assert_eq!(s.focus(1, rows), 0);
        // The utility row is still reachable when there are no results.
        s.edit(&typed("z"), false);
        assert_eq!(s.focus(0, rows), 2);
        s.clear();
        assert_eq!(s.focus(2, rows), 2);
        assert_eq!(s.focus(7, []), 7);
    }

    #[test]
    fn typing_keys_type_lower_case() {
        assert_eq!(typed_char(0x41), Some('a'));
        assert_eq!(typed_char(0x5A), Some('z'));
        assert_eq!(typed_char(0x37), Some('7'));
        assert_eq!(typed_char(VK_OEM_MINUS), Some('-'));
        assert_eq!(typed_char(VK_RETURN), None);
        assert!(typing_keys().all(|k| typed_char(k).is_some()));
    }
}

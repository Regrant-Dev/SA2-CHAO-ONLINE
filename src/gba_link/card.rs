//! The info card of a creature in the garden (a wild visitor or a partner brought in): its name,
//! level, types, ability, stats and the moves a Chao that absorbs it learns, all from the ROM.
//! It shows for the one the player holds, else the nearest within `RANGE`. A wild visitor's
//! card also has the button that battles it (`wild_battle`).

// Off the game (Linux builds) only the tests use the text side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use crate::gba::rom::Rom;

use super::ascii;
use super::store::Partner;
use crate::net::protocol::WildPlace;

/// A creature's card shows when the player is this close to it (across).
pub const RANGE: f32 = 7.0;
/// Stats are shown with this IV in every stat and a neutral nature (no EVs).
const SHOWN_IV: u8 = 15;

/// The card's lines.
pub fn lines(rom: &Rom, partner: &Partner, shiny: bool) -> Vec<String> {
    let name = rom
        .species_name(partner.species)
        .map_or_else(|| format!("#{}", partner.species), |n| ascii(&n));
    let mut out = vec![format!(
        "{name}  Lv{}{}",
        partner.level,
        if shiny { "  (shiny)" } else { "" }
    )];
    let mut types = vec![partner.types[0]];
    if partner.types[1] != partner.types[0] {
        types.push(partner.types[1]);
    }
    let types: Vec<String> = types
        .iter()
        .filter_map(|&t| rom.type_name(t))
        .map(|t| ascii(&t))
        .collect();
    out.push(format!("Type: {}", types.join(" / ")));
    let ability = rom
        .ability_name(partner.ability)
        .or_else(|| {
            let first = rom.species(partner.species)?.abilities[0];
            rom.ability_name(first)
        })
        .map_or_else(|| "none".into(), |a| ascii(&a));
    out.push(format!("Ability: {ability}"));
    if let Some(species) = rom.species(partner.species) {
        let s = crate::battle::trainers::stats(species.base, SHOWN_IV, partner.level, 0);
        out.push(format!("HP {}  Atk {}  Def {}", s.hp, s.attack, s.defense));
        out.push(format!(
            "SpA {}  SpD {}  Spe {}",
            s.sp_attack, s.sp_defense, s.speed
        ));
    }
    out.push("Teaches:".into());
    let mut any = false;
    for id in partner.moves.into_iter().filter(|&id| id != 0) {
        let Some(name) = rom.move_name(id) else {
            continue;
        };
        let detail = rom.move_info(id).map_or_else(String::new, |m| {
            let kind = rom
                .type_name(m.type_id)
                .map_or_else(String::new, |t| ascii(&t));
            if m.power > 1 {
                format!("  {kind} {}", m.power)
            } else {
                format!("  {kind}")
            }
        });
        out.push(format!(" {}{detail}", ascii(&name)));
        any = true;
    }
    if !any {
        out.push(" (no moves)".into());
    }
    out
}

/// The card's line about who has a shared garden's visitor (`place`), when someone other than
/// the player (`me`) holds it, or someone battles it (`name` gives a player's name).
pub fn note(place: WildPlace, me: Option<u8>, name: impl Fn(u8) -> String) -> Option<String> {
    match place {
        WildPlace::Held { by } if Some(by) != me => Some(format!("Held by {}", name(by))),
        WildPlace::Battle { by } if Some(by) == me => Some("Battling you".into()),
        WildPlace::Battle { by } => Some(format!("Battling {}", name(by))),
        _ => None,
    }
}

/// The creature whose card shows: the one the player holds (`held`), else the nearest of
/// `creatures` (object, position) within `RANGE` of `player` (across, and close in height).
pub fn pick(held: usize, player: [f32; 3], creatures: &[(usize, [f32; 3])]) -> Option<usize> {
    if held != 0 && creatures.iter().any(|&(o, _)| o == held) {
        return Some(held);
    }
    creatures
        .iter()
        .map(|&(o, at)| {
            let (dx, dz) = (at[0] - player[0], at[2] - player[2]);
            (o, dx * dx + dz * dz, (at[1] - player[1]).abs())
        })
        .filter(|&(_, d, up)| d < RANGE * RANGE && up < RANGE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(o, _, _)| o)
}

#[cfg(target_arch = "x86")]
pub use game::on_frame;

#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}

#[cfg(target_arch = "x86")]
mod game {
    use super::super::store::Partner;
    use super::super::{loaded_rom, scene, shared, wild, with_store};
    use super::{lines, note, pick};
    use crate::game as sa2;
    use crate::hud;

    const COLOR_TITLE: i32 = 0xFFF0_C030u32 as i32;
    const COLOR_TEXT: i32 = 0xFFE0_E8F0u32 as i32;

    pub fn on_frame() {
        if !sa2::in_garden() || sa2::between_areas() || crate::menu::is_open() {
            return;
        }
        let Some(rom) = loaded_rom() else {
            return;
        };
        let Some((p, _)) = sa2::player_position() else {
            return;
        };
        // (object, position, what it teaches, shiny)
        let mut found: Vec<(usize, [f32; 3], Partner, bool)> = wild::creatures();
        let garden = sa2::current_chao_area() as u8;
        // A shared garden's: from its list entry, with who holds or battles it.
        let mut notes: Vec<(usize, String)> = Vec::new();
        let copies = shared::creatures();
        if !copies.is_empty() {
            let me = shared::me();
            let others = crate::online::others();
            let name = |id: u8| {
                others
                    .iter()
                    .find(|(o, _, _)| *o == id)
                    .map_or_else(|| format!("player {id}"), |(_, n, _)| n.clone())
            };
            for (object, at, entry) in copies {
                let Some(partner) = shared::partner_of(&rom, &entry.creature, false) else {
                    continue;
                };
                if let Some(n) = note(entry.place, me, name) {
                    notes.push((object, n));
                }
                found.push((object, at, partner, entry.creature.shiny));
            }
        }
        for (object, at, key) in scene::creatures() {
            let partner = with_store(|s| {
                s.waiting_in(garden)
                    .find(|w| w.key == key)
                    .map(|w| w.partner)
            });
            if let Some(partner) = partner {
                found.push((object, at, partner, false));
            }
        }
        let spots: Vec<(usize, [f32; 3])> = found.iter().map(|c| (c.0, c.1)).collect();
        let Some(object) = pick(crate::garden::player_held(), [p.x, p.y, p.z], &spots) else {
            return;
        };
        let Some((_, _, partner, shiny)) = found.iter().find(|c| c.0 == object) else {
            return;
        };
        let mut text = lines(&rom, partner, *shiny);
        if let Some((_, n)) = notes.iter().find(|(o, _)| *o == object) {
            text.push(n.clone());
        }
        if let Some(battle) = super::super::wild_battle::card(object, partner, *shiny) {
            text.push(battle.into());
        }
        let size = hud::menu_font();
        let (w, _) = hud::screen_size();
        let cols = text.iter().map(|l| l.chars().count()).max().unwrap_or(0) + 2;
        let x = (w - (cols as f32 + 1.0) * size).max(0.0);
        let y = 3.0 * size;
        hud::panel(x, y, size, cols, text.len() + 2);
        for (i, line) in text.iter().enumerate() {
            let color = if i == 0 { COLOR_TITLE } else { COLOR_TEXT };
            hud::text(x + size, y + (i + 1) as f32 * size, size, color, line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_one_first_else_the_nearest_in_range() {
        let c = [
            (1, [0.0, 0.0, 5.0]),
            (2, [3.0, 0.0, 0.0]),
            (3, [20.0, 0.0, 0.0]),
        ];
        assert_eq!(pick(0, [0.0; 3], &c), Some(2));
        assert_eq!(pick(3, [0.0; 3], &c), Some(3));
        // Holding something else: the nearest.
        assert_eq!(pick(9, [0.0; 3], &c), Some(2));
        assert_eq!(pick(0, [100.0, 0.0, 0.0], &c), None);
        // Far above or below doesn't count.
        assert_eq!(pick(0, [3.0, 30.0, 0.0], &c[1..2]), None);
    }

    #[test]
    fn a_shared_visitor_says_who_has_it() {
        let name = |id: u8| format!("P{id}");
        assert_eq!(note(WildPlace::Roam, Some(1), name), None);
        assert_eq!(note(WildPlace::Held { by: 1 }, Some(1), name), None);
        assert_eq!(
            note(WildPlace::Held { by: 3 }, Some(1), name).as_deref(),
            Some("Held by P3")
        );
        assert_eq!(
            note(WildPlace::Battle { by: 3 }, Some(1), name).as_deref(),
            Some("Battling P3")
        );
        assert_eq!(
            note(WildPlace::Battle { by: 1 }, Some(1), name).as_deref(),
            Some("Battling you")
        );
    }

    #[test]
    fn a_card_from_the_rom() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        let Ok(rom) = Rom::open(&path) else {
            return;
        };
        let species = rom.starters().unwrap()[0];
        let info = rom.species(species).unwrap();
        let partner = Partner {
            species,
            types: info.types,
            moves: rom.moves_at_level(species, 12),
            level: 12,
            ability: 0,
            from_save: false,
        };
        let text = lines(&rom, &partner, true);
        assert!(text[0].ends_with("Lv12  (shiny)"));
        assert!(text[1].starts_with("Type: "));
        assert!(text[2].starts_with("Ability: ") && !text[2].ends_with("none"));
        assert!(text[3].starts_with("HP ") && text[4].starts_with("SpA "));
        assert_eq!(text[5], "Teaches:");
        let taught = partner.moves.iter().filter(|&&m| m != 0).count();
        assert_eq!(text.len(), 6 + taught);
        assert!(text.iter().all(|l| l.is_ascii()));
    }
}

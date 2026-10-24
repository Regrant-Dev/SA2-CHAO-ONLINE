//! The visitor print (`Body::WildHello`): what a ROM's wild visitors are made of. Two games with
//! the same print build the same visitor, card, partner and battle foe from the same ids, so
//! only they share a garden's visitors (re/shared-visitors.md §1).

use std::fmt::Write;

use crate::battle::online::dex_print;
use crate::battle::team::Dex;
use crate::gba::rom::{Rom, SPECIES_COUNT};

/// FNV-1a over formatted text.
struct Fnv(u32);

impl Write for Fnv {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for b in s.bytes() {
            self.0 = (self.0 ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
        Ok(())
    }
}

/// The print of `rom`: its battle data (`dex_print`), every species' record and level-up
/// learnset, and every map's wild tables with its map section. Never 0; `None` when the ROM's
/// battle data or wild tables can't be read.
pub fn visitor_print(rom: &Rom) -> Option<u32> {
    let dex = Dex::from_rom(rom).ok()?;
    let mut h = Fnv(0x811C_9DC5);
    let _ = write!(h, "{}", dex_print(&dex));
    for species in 1..SPECIES_COUNT {
        let _ = write!(
            h,
            "|{species}:{:?}:{:?}",
            rom.species(species),
            rom.learnset(species)
        );
    }
    for map in rom.wild_maps()? {
        let _ = write!(
            h,
            "|{}.{}:{:?}:{:?}",
            map.group,
            map.num,
            rom.map_section(map.group, map.num),
            map.tables
        );
    }
    Some(h.0.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_rom_has_one_print_and_a_changed_one_another() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = Path::new(&home).join("Games/gba/emerald.gba");
        let Ok(bytes) = std::fs::read(&path) else {
            println!("skipped: no GBA ROM at {}", path.display());
            return;
        };
        let rom = Rom::from_bytes(bytes.clone()).unwrap();
        let print = visitor_print(&rom).unwrap();
        assert_ne!(print, 0);
        assert_eq!(
            visitor_print(&Rom::from_bytes(bytes.clone()).unwrap()),
            Some(print)
        );
        // One learnset entry's level changed: another print.
        let learnset = rom.learnset(1);
        let (level, id) = learnset[1];
        let mut changed = bytes;
        let find = |data: &[u8]| {
            let a = (u16::from(level) << 9) | id;
            let b = learnset[0].1 | (u16::from(learnset[0].0) << 9);
            data.windows(4)
                .position(|w| w == [b as u8, (b >> 8) as u8, a as u8, (a >> 8) as u8])
        };
        let at = find(&changed).expect("the first species' learnset") + 2;
        let a = ((u16::from(level) + 1) << 9) | id;
        changed[at..at + 2].copy_from_slice(&a.to_le_bytes());
        let other = Rom::from_bytes(changed).unwrap();
        assert_ne!(other.learnset(1), learnset);
        assert_ne!(visitor_print(&other), Some(print));
    }
}

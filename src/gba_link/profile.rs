//! A Chao's side in a battle: its types, ability and four moves. A Chao that absorbed a partner
//! has the partner's types and ability and the moves its partners taught it (`store::Learned`);
//! one without is Normal type with no ability and basic moves read from the ROM. Every id is the
//! ROM's own.

use crate::gba::rom::{ABILITY_COUNT, MOVE_COUNT, Rom, SPECIES_COUNT};

use super::store::Learned;

/// The Normal type's id.
pub const NORMAL: u8 = 0;
/// Basic moves are the ones Normal-type species learn by this level.
const BASIC_LEVEL: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The same twice for one type.
    pub types: [u8; 2],
    /// 0 for none.
    pub ability: u8,
    /// Move ids, the known ones first; 0 for an empty slot.
    pub moves: [u16; 4],
    /// The species of the partner it has these from.
    pub partner: Option<u16>,
}

/// The profile of a Chao that has `learned` (`None` without a partner). Moves the ROM doesn't
/// have are dropped and empty slots are filled with basic moves it doesn't know yet. Without
/// `save_partners` (a live battle's rule), moves a partner from a GBA save taught are left out,
/// and a partner from a GBA save gives no types or ability (Normal, none).
pub fn profile(rom: &Rom, learned: Option<&Learned>, save_partners: bool) -> Profile {
    let mut moves = [0u16; 4];
    let mut n = 0;
    let known = learned.map_or([0; 4], |l| {
        let mut known = l.moves;
        if !save_partners {
            for (i, id) in known.iter_mut().enumerate() {
                if l.save_moves & 1 << i != 0 {
                    *id = 0;
                }
            }
        }
        known
    });
    let learned = learned.filter(|l| save_partners || !l.partner.from_save);
    for id in known.into_iter().chain(basic_moves(rom)) {
        if n < moves.len()
            && id != 0
            && id < MOVE_COUNT
            && rom.move_info(id).is_some()
            && !moves[..n].contains(&id)
        {
            moves[n] = id;
            n += 1;
        }
    }
    let Some(l) = learned else {
        return Profile {
            types: [NORMAL; 2],
            ability: 0,
            moves,
            partner: None,
        };
    };
    let p = l.partner;
    let ability = if (1..ABILITY_COUNT).contains(&p.ability) {
        p.ability
    } else {
        rom.species(p.species).map_or(0, |s| s.abilities[0])
    };
    Profile {
        types: p.types,
        ability,
        moves,
        partner: Some(p.species),
    }
}

/// A Chao's basic moves: of the Normal-type moves pure Normal-type species learn by
/// `BASIC_LEVEL`, the two plain damaging and the two other moves most of them learn (lower ids
/// first on a tie), damaging ones first. When the ROM gives fewer, the others fill in, and with
/// no damaging one at all the lowest-id plain damaging Normal move comes first.
pub fn basic_moves(rom: &Rom) -> [u16; 4] {
    let mut count = vec![0u32; usize::from(MOVE_COUNT)];
    for species in 1..SPECIES_COUNT {
        if rom.species(species).is_none_or(|s| s.types != [NORMAL; 2]) {
            continue;
        }
        let mut seen = Vec::new();
        for (level, id) in rom.learnset(species) {
            if level <= BASIC_LEVEL && !seen.contains(&id) {
                seen.push(id);
                count[usize::from(id)] += 1;
            }
        }
    }
    let plain_damage = |id: u16| {
        rom.move_info(id)
            .is_some_and(|m| m.type_id == NORMAL && m.effect == 0 && m.power > 1)
    };
    let mut ranked: Vec<u16> = (1..MOVE_COUNT)
        .filter(|&id| count[usize::from(id)] > 0)
        .filter(|&id| rom.move_info(id).is_some_and(|m| m.type_id == NORMAL))
        .collect();
    ranked.sort_by_key(|&id| std::cmp::Reverse(count[usize::from(id)]));
    let (mut damaging, mut other): (Vec<u16>, Vec<u16>) =
        ranked.into_iter().partition(|&id| plain_damage(id));
    if damaging.is_empty()
        && let Some(id) = (1..MOVE_COUNT).find(|&id| plain_damage(id))
    {
        damaging.push(id);
    }
    let mut picked: Vec<u16> = damaging.iter().take(2).copied().collect();
    picked.extend(other.iter().take(2));
    // Fewer than two of a kind: the rest of the other kind fills in.
    damaging.drain(..damaging.len().min(2));
    other.drain(..other.len().min(2));
    picked.extend(damaging.into_iter().chain(other));
    let mut out = [0u16; 4];
    for (slot, id) in out.iter_mut().zip(picked) {
        *slot = id;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba_link::store::Partner;
    use std::path::Path;

    fn rom() -> Option<Rom> {
        let home = std::env::var_os("HOME")?;
        let path = Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return None;
        }
        Some(Rom::open(&path).expect("ROM opens"))
    }

    #[test]
    fn basic_moves_are_normal_and_lead_with_damage() {
        let Some(rom) = rom() else { return };
        let moves = basic_moves(&rom);
        assert_eq!(moves, basic_moves(&rom));
        assert!(moves.iter().all(|&id| id != 0), "{moves:?}");
        for (i, &id) in moves.iter().enumerate() {
            let m = rom.move_info(id).unwrap();
            assert_eq!(m.type_id, NORMAL);
            assert!(!moves[..i].contains(&id));
            if i < 2 {
                assert!(m.power > 1 && m.effect == 0, "{id}: {m:?}");
            } else {
                assert_eq!(m.power, 0, "{id}: {m:?}");
            }
        }
    }

    #[test]
    fn profiles_with_and_without_a_partner() {
        let Some(rom) = rom() else { return };
        let basic = basic_moves(&rom);
        let none = profile(&rom, None, true);
        assert_eq!(none.types, [NORMAL; 2]);
        assert_eq!(none.ability, 0);
        assert_eq!(none.moves, basic);
        assert_eq!(none.partner, None);

        // The first gift species, as it comes at level 5 with no ability recorded.
        let species = rom.starters().unwrap()[0];
        let info = rom.species(species).unwrap();
        let taught = rom.moves_at_level(species, 5);
        let learned = Learned {
            partner: Partner {
                species,
                types: info.types,
                moves: taught,
                level: 5,
                ability: 0,
                from_save: false,
            },
            moves: super::super::store::teach([0; 4], taught),
            save_moves: 0,
        };
        let p = profile(&rom, Some(&learned), true);
        assert_eq!(profile(&rom, Some(&learned), false), p);
        assert_eq!(p.types, info.types);
        assert_ne!(p.ability, 0);
        assert_eq!(p.ability, info.abilities[0]);
        assert_eq!(p.partner, Some(species));
        let n = taught.iter().filter(|&&id| id != 0).count();
        assert!(n >= 1);
        assert_eq!(p.moves[..n], learned.moves[..n]);
        // Its empty slots take basic moves, never twice the same.
        assert!(p.moves.iter().all(|&id| id != 0));
        for i in 0..4 {
            assert!(!p.moves[..i].contains(&p.moves[i]));
        }

        // A recorded ability is kept; moves the ROM doesn't have are dropped.
        let odd = Learned {
            partner: Partner {
                ability: 3,
                ..learned.partner
            },
            moves: [MOVE_COUNT + 5, 0, 0, 0],
            save_moves: 0,
        };
        let p = profile(&rom, Some(&odd), true);
        assert_eq!(p.ability, 3);
        assert_eq!(p.moves, basic);

        // From a GBA save: all of it with save partners allowed; without, its moves and its
        // partner's types and ability go, and the moves a wild one taught stay.
        let wild = basic[0];
        let saved = Learned {
            partner: Partner {
                from_save: true,
                ..learned.partner
            },
            moves: [taught[0], wild, 0, 0],
            save_moves: 0b01,
        };
        let with = profile(&rom, Some(&saved), true);
        assert_eq!(with.types, info.types);
        assert_eq!(with.moves[..2], [taught[0], wild]);
        let without = profile(&rom, Some(&saved), false);
        assert_eq!(without.types, [NORMAL; 2]);
        assert_eq!(without.ability, 0);
        assert_eq!(without.partner, None);
        assert_eq!(without.moves[0], wild);
        assert!(!without.moves.contains(&taught[0]) || basic.contains(&taught[0]));
    }
}

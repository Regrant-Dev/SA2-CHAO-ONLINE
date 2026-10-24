//! A Chao's side in a battle: its types, ability, four moves and battle level. A Chao battles
//! as the one or two types and with the ability it chose from every one its partners gave it
//! (`store::Traits`), with the four moves it chose from every move it learned (`store::Moves`).
//! One without types is Normal, one without an ability has none, and empty move slots take
//! basic moves read from the ROM. Every id is the ROM's own.

use crate::gba::rom::{ABILITY_COUNT, MOVE_COUNT, Rom, SPECIES_COUNT, TYPE_COUNT};

use super::store::{Moves, Partner, Traits};

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
    /// Move ids, the ones it battles with first; 0 for an empty slot.
    pub moves: [u16; 4],
    /// The species of the partner it has these from.
    pub partner: Option<u16>,
    /// Its battle level (`battle::levels`).
    pub level: u8,
}

/// The profile of a Chao at `level` with its last `partner` (`None`, or species 0: none), its
/// `traits` and `moves`. It battles as its active types with its active ability and moves;
/// ids the ROM doesn't have are dropped, empty move slots are filled with basic moves it doesn't
/// know yet, and without types it is Normal. Without `save_partners` (a live battle's rule),
/// moves, types and abilities only partners from a GBA save gave are left out: their places take
/// the Chao's others (newest first), then basic moves, Normal or no ability.
pub fn profile(
    rom: &Rom,
    partner: Option<&Partner>,
    traits: &Traits,
    moves: &Moves,
    save_partners: bool,
    level: u8,
) -> Profile {
    let counts = |id: u16| save_partners || !moves.only_from_saves(id);
    let active: Vec<u16> = moves.active.iter().copied().filter(|&id| id != 0).collect();
    let left_out = active.iter().filter(|&&id| !counts(id)).count();
    let others = moves
        .known
        .iter()
        .rev()
        .map(|&(id, _)| id)
        .filter(|&id| !moves.is_active(id) && counts(id))
        .take(left_out);
    let mut picked = [0u16; 4];
    let mut n = 0;
    let candidates = active
        .iter()
        .copied()
        .filter(|&id| counts(id))
        .chain(others);
    for id in candidates.chain(basic_moves(rom)) {
        if n < picked.len()
            && id != 0
            && id < MOVE_COUNT
            && rom.move_info(id).is_some()
            && !picked[..n].contains(&id)
        {
            picked[n] = id;
            n += 1;
        }
    }
    let (types, ability) = traits_ruled(traits, save_partners);
    Profile {
        types,
        ability,
        moves: picked,
        partner: partner
            .filter(|p| p.species != 0 && (save_partners || !p.from_save))
            .map(|p| p.species),
        level,
    }
}

/// The types (the same twice for one) and ability a Chao with `traits` battles with: its active
/// ones the ROM has; without `save_partners`, the ones only partners from a GBA save gave are
/// left out, their places taking its other ones (newest first), else Normal and no ability.
pub fn traits_ruled(traits: &Traits, save_partners: bool) -> ([u8; 2], u8) {
    let type_counts = |t: u8| t < TYPE_COUNT && (save_partners || !traits.type_only_from_saves(t));
    let active = traits.active_type_list();
    let kept: Vec<u8> = active.iter().copied().filter(|&t| type_counts(t)).collect();
    let others = traits
        .types
        .iter()
        .rev()
        .map(|&(t, _)| t)
        .filter(|&t| !traits.is_type_active(t) && type_counts(t))
        .take(active.len() - kept.len());
    let picked: Vec<u8> = kept.iter().copied().chain(others).collect();
    let types = match picked[..] {
        [] => [NORMAL; 2],
        [a] => [a; 2],
        [a, b, ..] => [a, b],
    };
    let ability_counts = |a: u8| {
        (1..ABILITY_COUNT).contains(&a) && (save_partners || !traits.ability_only_from_saves(a))
    };
    let ability = match traits.ability {
        0 => 0,
        a if ability_counts(a) => a,
        a => traits
            .abilities
            .iter()
            .rev()
            .map(|&(k, _)| k)
            .find(|&k| k != a && ability_counts(k))
            .unwrap_or(0),
    };
    (types, ability)
}

/// `traits` of a Chao whose last partner is `partner`, with the ability older files didn't
/// record: a Chao that has no ability yet and a partner with none recorded has the partner's
/// species' first ability from the ROM (marked as the partner was), active.
pub fn recorded(rom: &Rom, partner: Option<&Partner>, mut traits: Traits) -> Traits {
    if let Some(p) = partner.filter(|p| p.species != 0 && p.ability == 0)
        && traits.abilities.is_empty()
        && let Some(first) = rom.species(p.species).map(|s| s.abilities[0])
        && first != 0
    {
        traits.learn(&Partner {
            ability: first,
            ..*p
        });
    }
    traits
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
        let none = profile(&rom, None, &Traits::default(), &Moves::default(), true, 5);
        assert_eq!(none.types, [NORMAL; 2]);
        assert_eq!(none.ability, 0);
        assert_eq!(none.moves, basic);
        assert_eq!(none.partner, None);
        assert_eq!(none.level, 5);

        // The first gift species, as it comes at level 5 with no ability recorded (older
        // files): its species' first ability.
        let species = rom.starters().unwrap()[0];
        let info = rom.species(species).unwrap();
        let taught = rom.moves_at_level(species, 5);
        let partner = Partner {
            species,
            types: info.types,
            moves: taught,
            level: 5,
            ability: 0,
            from_save: false,
        };
        let traits = recorded(&rom, Some(&partner), Traits::from_partner(&partner));
        assert_eq!(traits.ability, info.abilities[0]);
        assert!(!traits.ability_only_from_saves(traits.ability));
        // One with an ability already keeps it, and no partner gives nothing.
        let mut chose = traits.clone();
        chose.choose_ability(0).unwrap();
        assert_eq!(recorded(&rom, Some(&partner), chose.clone()), chose);
        assert_eq!(recorded(&rom, None, Traits::default()), Traits::default());
        let mut moves = Moves::default();
        moves.learn(taught, false);
        let p = profile(&rom, Some(&partner), &traits, &moves, true, 30);
        assert_eq!(profile(&rom, Some(&partner), &traits, &moves, false, 30), p);
        assert_eq!(p.types, info.types);
        assert_ne!(p.ability, 0);
        assert_eq!(p.partner, Some(species));
        assert_eq!(p.level, 30);
        let n = taught.iter().filter(|&&id| id != 0).count();
        assert!(n >= 1);
        assert_eq!(p.moves[..n], moves.active[..n]);
        // Its empty slots take basic moves, never twice the same.
        assert!(p.moves.iter().all(|&id| id != 0));
        for i in 0..4 {
            assert!(!p.moves[..i].contains(&p.moves[i]));
        }
        // No ability chosen: none.
        assert_eq!(
            profile(&rom, Some(&partner), &chose, &moves, true, 30).ability,
            0
        );

        // Moves the ROM doesn't have are dropped; species 0 is no partner.
        let unknown = Moves {
            known: vec![(MOVE_COUNT + 5, false)],
            active: [MOVE_COUNT + 5, 0, 0, 0],
        };
        let p = profile(&rom, Some(&partner), &traits, &unknown, true, 5);
        assert_eq!(p.moves, basic);
        let p = profile(
            &rom,
            Some(&Partner {
                species: 0,
                ..partner
            }),
            &Traits::default(),
            &unknown,
            true,
            5,
        );
        assert_eq!((p.partner, p.types, p.ability), (None, [NORMAL; 2], 0));

        // From a GBA save: all of it with save partners allowed; without, its moves and its
        // partner's types and ability go, the moves a wild one taught stay, and the Chao's
        // other moves fill in for the ones left out.
        let mut spare = basic.into_iter().filter(|m| !taught.contains(m));
        let (wild, other) = (spare.next().unwrap(), spare.next().unwrap());
        let mut saved_moves = Moves::default();
        saved_moves.learn([other, 0, 0, 0], false);
        saved_moves.learn([taught[0], wild, 0, 0], true);
        saved_moves.learn([wild, 0, 0, 0], false);
        saved_moves.toggle(other).unwrap();
        assert_eq!(saved_moves.active, [0, taught[0], wild, 0]);
        let saved = Partner {
            from_save: true,
            ..partner
        };
        let saved_traits = recorded(&rom, Some(&saved), Traits::from_partner(&saved));
        let with = profile(&rom, Some(&saved), &saved_traits, &saved_moves, true, 5);
        assert_eq!(with.types, info.types);
        assert_eq!(with.moves[..2], [taught[0], wild]);
        let without = profile(&rom, Some(&saved), &saved_traits, &saved_moves, false, 5);
        assert_eq!(without.types, [NORMAL; 2]);
        assert_eq!(without.ability, 0);
        assert_eq!(without.partner, None);
        assert_eq!(without.moves[..2], [wild, other]);
        assert!(!without.moves.contains(&taught[0]) || basic.contains(&taught[0]));
    }

    fn partner(types: [u8; 2], ability: u8, from_save: bool) -> Partner {
        Partner {
            species: 1,
            types,
            moves: [0; 4],
            level: 5,
            ability,
            from_save,
        }
    }

    #[test]
    fn the_save_rule_leaves_out_types_and_abilities_only_saves_gave() {
        // Fire 9 and Flying 2 with ability 30 from a save; Water 11 with 40 from a wild one.
        let mut t = Traits::default();
        t.learn(&partner([9, 2], 30, true));
        t.learn(&partner([11, 11], 40, false));
        assert_eq!(traits_ruled(&t, true), ([9, 2], 30));
        // Both types and the ability go: the wild one's take their places.
        assert_eq!(traits_ruled(&t, false), ([11, 11], 40));
        // One of two kept, the other replaced by one that counts.
        t.toggle_type(2).unwrap();
        t.toggle_type(11).unwrap();
        assert_eq!(t.active_types, [9, 11]);
        assert_eq!(traits_ruled(&t, false), ([11, 11], 40));
        t.learn(&partner([4, 4], 0, false));
        assert_eq!(traits_ruled(&t, false), ([11, 4], 40));
        // Nothing else that counts: Normal and no ability.
        let mut only = Traits::default();
        only.learn(&partner([9, 2], 30, true));
        assert_eq!(traits_ruled(&only, false), ([NORMAL; 2], 0));
        // No ability chosen stays none either way; ids the ROM doesn't have don't count.
        t.choose_ability(0).unwrap();
        assert_eq!(traits_ruled(&t, true).1, 0);
        let odd = Traits::chosen([TYPE_COUNT, 3], ABILITY_COUNT);
        assert_eq!(traits_ruled(&odd, true), ([3, 3], 0));
        assert_eq!(traits_ruled(&Traits::default(), true), ([NORMAL; 2], 0));
    }
}

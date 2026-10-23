//! Reads the trainer ladder from the GBA ROM (re/chao-battle-online.md 2-4): the route trainers
//! grouped by strength, the gym leaders in order with their towns and the TM each gives, the
//! league's four and the champion, and what the game gives a trainer's creatures (ability,
//! nature, friendship).

use std::collections::BTreeSet;

use super::rom::Rom;
use super::world::{TRAINER_COUNT, TrainerMon};

/// Trainer records (see `world`): the name at +4 (12 bytes), the class at +1, the music and
/// female bit at +2, double battles at +0x18.
const TRAINERS: usize = 0x310030;
const TRAINER_SIZE: usize = 40;
const TRAINER_NAME: usize = 4;
const TRAINER_NAME_LEN: usize = 12;
/// Species names: 11-byte strings ended by 0xFF, indexed by species id.
const SPECIES_NAMES: usize = 0x3185C8;
const SPECIES_NAME_LEN: usize = 11;
/// Species records: 28 bytes; base friendship at +0x12.
const SPECIES_INFO: usize = 0x3203CC;
const SPECIES_INFO_SIZE: usize = 28;
const SPECIES_FRIENDSHIP: usize = 0x12;
/// Rematch rows (pret `gRematchTable`): 16 bytes {5 u16 trainer ids (the first battle, then the
/// rematches), u16 map group, u16 map number, pad}.
const REMATCHES: usize = 0x5500A4;
const REMATCH_ROWS: usize = 78;
const REMATCH_ROW: usize = 16;
const REMATCH_IDS: usize = 5;
/// The ROM's size, scanned for the gyms' event scripts.
const ROM_LEN: usize = 16 << 20;
/// Event script bytes: the trainer battle command (`5C type id:u16 local:u16 pointers...`)
/// and the item gift (`1A 00 80 item:u16 1A 01 80 01 00 09 00`).
const BATTLE_COMMAND: u8 = 0x5C;
const GIFT_HEAD: [u8; 3] = [0x1A, 0x00, 0x80];
const GIFT_TAIL: [u8; 7] = [0x1A, 0x01, 0x80, 0x01, 0x00, 0x09, 0x00];
/// The gift is looked for this far after the battle command's arguments.
const GIFT_WINDOW: usize = 0x200;
/// Battle command types that are rematches.
const REMATCH_TYPES: [u8; 2] = [5, 7];
/// Pointer arguments per battle command type 0..=8.
const BATTLE_POINTERS: [usize; 9] = [2, 3, 3, 1, 3, 2, 4, 3, 4];
/// ROM pointers are 0x0800_0000 + offset.
const ROM_POINTERS: std::ops::Range<u32> = 0x0800_0000..0x0900_0000;
/// TM items (the first TM's item id; TM/HM index = item - this).
const TM_ITEM_FIRST: u16 = 289;

/// Trainer class ids (record byte 1).
pub const CLASS_LEAGUE: u8 = 0x1F;
pub const CLASS_LEADER: u8 = 0x20;
pub const CLASS_CHAMPION: u8 = 0x26;
/// The battle facility leaders and two placeholder records: never on the ladder.
const CLASS_FACILITY: std::ops::RangeInclusive<u8> = 0x3A..=0x41;

/// Gym leaders and league trainers on the ladder.
pub const LEADERS: usize = 8;
pub const LEAGUE: usize = 4;
/// Route trainer tiers: one up to each leader's strength, and one above the last.
pub const TIERS: usize = LEADERS + 1;

/// What makes two route trainer records the same trainer: class, raw name, and each
/// creature's species and level.
type Repeat = (u8, Vec<u8>, Vec<(u16, u8)>);

/// A gym leader's first battle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leader {
    pub id: u16,
    /// The TM/HM index (0..TM_COUNT) of the TM the leader gives, from the gym's script.
    pub tm: Option<u8>,
    /// The name of the place the leader stands.
    pub town: Option<String>,
}

/// Every trainer on the ladder, by trainer id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ladder {
    /// Route trainers in `TIERS` tiers: tier k (k < LEADERS) holds the ones whose strongest
    /// creature is no stronger than leader k's (and stronger than leader k-1's), the last tier
    /// the ones above the last leader. Each tier is sorted by (top level, id).
    pub route: Vec<Vec<u16>>,
    /// `LEADERS` gym leaders in gym order.
    pub leaders: Vec<Leader>,
    /// `LEAGUE` league trainers in battle order.
    pub league: Vec<u16>,
    pub champion: u16,
}

/// One of a trainer's creatures as the game makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyMember {
    pub mon: TrainerMon,
    pub ability: u8,
    /// 0..25: raises stat `nature / 5` and lowers stat `nature % 5` (attack, defense, speed,
    /// sp. attack, sp. defense).
    pub nature: u8,
    pub friendship: u8,
}

impl Rom {
    fn trainer_byte(&self, id: u16, at: usize) -> Option<u8> {
        let off = TRAINERS + usize::from(id) * TRAINER_SIZE + at;
        self.bytes_at(off, 1)?.first().copied()
    }

    /// Raw name bytes up to the 0xFF terminator.
    fn raw_name(&self, off: usize, len: usize) -> Option<&[u8]> {
        let b = self.bytes_at(off, len)?;
        Some(&b[..b.iter().position(|&c| c == 0xFF).unwrap_or(len)])
    }

    /// The trainer's creatures with the ability, nature and friendship the game gives them
    /// (pret `CreateNPCTrainerParty`): a personality value per creature from a start value and
    /// a running sum of the trainer's and the species' name bytes.
    pub fn trainer_party(&self, id: u16) -> Option<Vec<PartyMember>> {
        let t = self.trainer(id)?;
        let name = self.raw_name(
            TRAINERS + usize::from(id) * TRAINER_SIZE + TRAINER_NAME,
            TRAINER_NAME_LEN,
        )?;
        let start: u32 = if t.double {
            0x80
        } else if t.female {
            0x78
        } else {
            0x88
        };
        let mut hash = 0u32;
        let mut out = Vec::with_capacity(t.party.len());
        for mon in t.party {
            let species = usize::from(mon.species);
            let species_name =
                self.raw_name(SPECIES_NAMES + species * SPECIES_NAME_LEN, SPECIES_NAME_LEN)?;
            for &b in name.iter().chain(species_name) {
                hash = hash.wrapping_add(u32::from(b));
            }
            let personality = start.wrapping_add(hash << 8);
            let info = self.species(mon.species)?;
            let ability = match info.abilities {
                [_, second] if personality & 1 != 0 && second != 0 => second,
                [first, _] => first,
            };
            let friendship = *self
                .bytes_at(
                    SPECIES_INFO + species * SPECIES_INFO_SIZE + SPECIES_FRIENDSHIP,
                    1,
                )?
                .first()?;
            out.push(PartyMember {
                mon,
                ability,
                nature: (personality % 25) as u8,
                friendship,
            });
        }
        Some(out)
    }

    /// The level of a trainer's strongest creature.
    fn top_level(&self, id: u16) -> Option<u8> {
        self.trainer(id)?.party.iter().map(|m| m.level).max()
    }

    /// The trainer ladder; `None` when the ROM's tables don't have the expected shape.
    pub fn ladder(&self) -> Option<Ladder> {
        let classes: Vec<(u16, u8)> = (1..TRAINER_COUNT)
            .filter_map(|id| Some((id, self.trainer_byte(id, 1)?)))
            .collect();
        let of_class = |class: u8| -> Vec<u16> {
            classes
                .iter()
                .filter(|&&(_, c)| c == class)
                .map(|&(id, _)| id)
                .collect()
        };
        let rows: Vec<[u16; REMATCH_IDS + 2]> = (0..REMATCH_ROWS)
            .map(|r| {
                let at = REMATCHES + r * REMATCH_ROW;
                let mut row = [0u16; REMATCH_IDS + 2];
                for (i, v) in row.iter_mut().enumerate() {
                    *v = self.u16_at(at + i * 2)?;
                }
                Some(row)
            })
            .collect::<Option<_>>()?;
        let rematches: BTreeSet<u16> = rows
            .iter()
            .flat_map(|r| r[1..REMATCH_IDS].iter().filter(move |&&id| id != r[0]))
            .copied()
            .collect();

        // Gym leaders: the lowest-id first battles, in gym order.
        let mut leader_ids: Vec<u16> = of_class(CLASS_LEADER)
            .into_iter()
            .filter(|id| !rematches.contains(id))
            .collect();
        leader_ids.truncate(LEADERS);
        let league = of_class(CLASS_LEAGUE);
        let champion = of_class(CLASS_CHAMPION);
        if leader_ids.len() != LEADERS || league.len() != LEAGUE || champion.len() != 1 {
            return None;
        }
        let gifts = self.gym_gifts(&leader_ids);
        let leaders: Vec<Leader> = leader_ids
            .iter()
            .zip(gifts)
            .map(|(&id, tm)| Leader {
                id,
                tm,
                town: rows
                    .iter()
                    .find(|r| r[0] == id)
                    .and_then(|r| self.map_name(r[REMATCH_IDS] as u8, r[REMATCH_IDS + 1] as u8)),
            })
            .collect();
        let tops: Vec<u8> = leader_ids
            .iter()
            .map(|&id| self.top_level(id))
            .collect::<Option<_>>()?;

        // Route trainers: everyone else but rematches and exact repeats.
        let mut seen: BTreeSet<Repeat> = BTreeSet::new();
        let mut route: Vec<(u8, u16)> = Vec::new();
        for &(id, class) in &classes {
            if [CLASS_LEADER, CLASS_LEAGUE, CLASS_CHAMPION].contains(&class)
                || CLASS_FACILITY.contains(&class)
                || rematches.contains(&id)
            {
                continue;
            }
            let Some(t) = self.trainer(id) else { continue };
            let name = self
                .raw_name(
                    TRAINERS + usize::from(id) * TRAINER_SIZE + TRAINER_NAME,
                    TRAINER_NAME_LEN,
                )
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            let party: Vec<(u16, u8)> = t.party.iter().map(|m| (m.species, m.level)).collect();
            if !seen.insert((class, name, party)) {
                continue;
            }
            let top = t.party.iter().map(|m| m.level).max().unwrap_or(1);
            route.push((top, id));
        }
        route.sort_unstable();
        let mut tiers = vec![Vec::new(); TIERS];
        for (top, id) in route {
            let tier = tops.iter().position(|&t| top <= t).unwrap_or(LEADERS);
            tiers[tier].push(id);
        }
        Some(Ladder {
            route: tiers,
            leaders,
            league,
            champion: champion[0],
        })
    }

    /// The TM (as a TM/HM index) each of `leaders` gives after their battle: the item gift
    /// right after the one first-battle command for the leader in the event scripts.
    fn gym_gifts(&self, leaders: &[u16]) -> Vec<Option<u8>> {
        let mut found: Vec<Vec<u8>> = vec![Vec::new(); leaders.len()];
        let Some(rom) = self.bytes_at(0, ROM_LEN) else {
            return vec![None; leaders.len()];
        };
        let u16_at = |i: usize| u16::from_le_bytes([rom[i], rom[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([rom[i], rom[i + 1], rom[i + 2], rom[i + 3]]);
        for at in 0..ROM_LEN - 6 {
            if rom[at] != BATTLE_COMMAND || rom[at + 4] != 0 || rom[at + 5] != 0 {
                continue;
            }
            let kind = rom[at + 1];
            let Some(&pointers) = BATTLE_POINTERS.get(usize::from(kind)) else {
                continue;
            };
            let Some(which) = leaders.iter().position(|&id| id == u16_at(at + 2)) else {
                continue;
            };
            let args_end = at + 6 + pointers * 4;
            if REMATCH_TYPES.contains(&kind)
                || args_end + GIFT_WINDOW > ROM_LEN
                || !(0..pointers).all(|p| ROM_POINTERS.contains(&u32_at(at + 6 + p * 4)))
            {
                continue;
            }
            let window = &rom[args_end..args_end + GIFT_WINDOW];
            let gift = window.windows(12).find_map(|w| {
                let item = u16::from_le_bytes([w[3], w[4]]);
                (w[..3] == GIFT_HEAD && w[5..12] == GIFT_TAIL)
                    .then(|| item.checked_sub(TM_ITEM_FIRST))
                    .flatten()
                    .and_then(|i| u8::try_from(i).ok())
                    .filter(|&i| i < super::rom::TM_COUNT)
            });
            if let Some(tm) = gift {
                found[which].push(tm);
            }
        }
        // Exactly one battle per leader, else it isn't trusted.
        found
            .into_iter()
            .map(|f| match f[..] {
                [tm] => Some(tm),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn rom_ladder() {
        let Some(rom) = rom() else { return };
        let l = rom.ladder().expect("ladder");
        assert_eq!(l.leaders.len(), LEADERS);
        assert_eq!(l.league.len(), LEAGUE);
        assert_eq!(l.route.len(), TIERS);
        // Leaders get stronger in order; each gives a different TM and stands in a named town.
        let tops: Vec<u8> = l
            .leaders
            .iter()
            .map(|x| rom.top_level(x.id).unwrap())
            .collect();
        assert!(tops.windows(2).all(|w| w[0] < w[1]), "{tops:?}");
        let tms: BTreeSet<u8> = l.leaders.iter().filter_map(|x| x.tm).collect();
        assert_eq!(tms.len(), LEADERS);
        assert!(tms.iter().all(|&t| t < crate::gba::rom::TM_COUNT));
        assert!(l.leaders.iter().all(|x| x.town.is_some()));
        // One leader fights doubles.
        let doubles = l
            .leaders
            .iter()
            .filter(|x| rom.trainer(x.id).unwrap().double)
            .count();
        assert_eq!(doubles, 1);
        // The league and the champion are stronger than the last leader.
        let last = tops[LEADERS - 1];
        for &id in l.league.iter().chain([&l.champion]) {
            assert!(rom.top_level(id).unwrap() > last);
            assert!(!rom.trainer(id).unwrap().double);
        }
        // Route tiers: sorted, no repeats, each inside its band.
        let mut all = BTreeSet::new();
        for (k, tier) in l.route.iter().enumerate() {
            let levels: Vec<(u8, u16)> = tier
                .iter()
                .map(|&id| (rom.top_level(id).unwrap(), id))
                .collect();
            assert!(levels.windows(2).all(|w| w[0] < w[1]));
            for &(top, id) in &levels {
                assert!(all.insert(id));
                if k < LEADERS {
                    assert!(top <= tops[k]);
                }
                if k > 0 {
                    assert!(top > tops[k - 1]);
                }
            }
        }
        assert!((400..700).contains(&all.len()), "{}", all.len());
        assert!(l.route.iter().all(|t| !t.is_empty()));
        assert!(l.route[0].len() > 50, "{}", l.route[0].len());
    }

    #[test]
    fn rom_trainer_parties() {
        let Some(rom) = rom() else { return };
        let l = rom.ladder().unwrap();
        for id in l.leaders.iter().map(|x| x.id).chain(l.league.clone()) {
            let party = rom.trainer_party(id).unwrap();
            assert_eq!(party.len(), rom.trainer(id).unwrap().party.len());
            for m in &party {
                let s = rom.species(m.mon.species).unwrap();
                assert!(s.abilities.contains(&m.ability) && m.ability != 0);
                assert!(m.nature < 25);
            }
        }
        // Natures vary across trainers (the name sums differ).
        let natures: BTreeSet<u8> = l.route[0]
            .iter()
            .flat_map(|&id| rom.trainer_party(id).unwrap())
            .map(|m| m.nature)
            .collect();
        assert!(natures.len() > 10, "{natures:?}");
        assert!(rom.trainer_party(0).is_none());
    }
}

//! Trainer and encounter adapters over canonical source identities.
use super::{data::SpeciesKey, rom::Rom};
pub const TRAINER_COUNT: u16 = 653;
pub const TRAINER_CLASS_COUNT: u8 = 223;
pub const PARTY_MAX: usize = 6;
pub const MAP_GROUP_COUNT: u8 = 1;
pub const MAP_SECTION_COUNT: u8 = 18;
/// A trainer and their party.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trainer {
    /// Trainer class id (0..TRAINER_CLASS_COUNT).
    pub class: u8,
    pub name: String,
    pub female: bool,
    /// Fights double battles.
    pub double: bool,
    /// The game's AI behaviour bits.
    pub ai_flags: u32,
    /// 1..=PARTY_MAX creatures.
    pub party: Vec<TrainerMon>,
}

#[cfg(test)]
mod habitat_tests {
    use super::*;
    #[test]
    fn habitats_cover_canonical_bases_and_ordinary_forms_once_with_valid_rates() {
        let mut data = super::super::catalog::test_catalog();
        for (species, kind) in [(19, 16), (3, 11)] {
            let index = data.personal.len() as u16;
            data.personal[species].form_count = 2;
            data.personal[species].form_stats_index = index;
            let mut form = data.personal[species].clone();
            form.source_index = index;
            form.types = [kind; 2];
            data.personal.push(form);
            data.learnsets.push(Vec::new());
        }
        let envelope = super::super::catalog::Envelope::new(data).unwrap();
        let source = Rom::from_bytes(serde_json::to_vec(&envelope).unwrap()).unwrap();
        let maps = source.wild_maps().unwrap();
        let mut keys = std::collections::BTreeSet::new();
        for map in maps {
            for table in map.tables {
                assert_eq!(
                    table
                        .slots
                        .iter()
                        .map(|slot| usize::from(slot.chance))
                        .sum::<usize>(),
                    100
                );
                for slot in table.slots {
                    let key = SpeciesKey {
                        species: slot.species,
                        form: slot.form,
                    };
                    assert!(keys.insert(key));
                    assert!(super::super::data::ordinary_form(key));
                    assert_eq!(source.personal(key).unwrap().types[0], map.num);
                }
            }
        }
        assert!((1..=807).all(|species| keys.contains(&SpeciesKey { species, form: 0 })));
        assert!(keys.contains(&SpeciesKey {
            species: 19,
            form: 1
        }));
        assert!(!keys.contains(&SpeciesKey {
            species: 3,
            form: 1
        }));
    }
}

/// One creature in a trainer's party.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrainerMon {
    pub species: u16,
    pub form: u8,
    /// 1..=100.
    pub level: u8,
    /// The individual value every stat gets (0..=31).
    pub iv: u8,
    pub ivs: [u8; 6],
    pub evs: [u8; 6],
    /// Move ids (0 for none): the party's own moves, or the learnset's last four at `level`.
    pub moves: [u16; 4],
}

/// How a wild table's creatures are met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WildMethod {
    Land,
    Water,
}

/// One wild encounter slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WildSlot {
    pub form: u8,
    pub species: u16,
    /// Level range, `min_level <= max_level`.
    pub min_level: u8,
    pub max_level: u8,
    /// Chance in percent that an encounter picks this slot; a table's slots add up to 100.
    pub chance: u8,
}

/// One way of meeting wild creatures on a map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildTable {
    pub method: WildMethod,
    /// How often encounters happen (the game's per-step rate, 0..=255).
    pub rate: u8,
    pub slots: Vec<WildSlot>,
}

/// The wild tables of one map. A map can have several entries (variants the game picks between).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildMap {
    pub group: u8,
    pub num: u8,
    pub tables: Vec<WildTable>,
}

impl Rom {
    pub fn trainer(&self, id: u16) -> Option<Trainer> {
        if id == 0 {
            return None;
        }
        let t = self.catalog.trainers.get(usize::from(id))?;
        if t.party.is_empty() {
            return None;
        }
        let party = t
            .party
            .iter()
            .map(|m| {
                let learned = self.moves_at_level_form(m.key, m.level);
                let moves = std::array::from_fn(|i| {
                    if m.moves[i] == 0 {
                        learned[i]
                    } else {
                        m.moves[i]
                    }
                });
                TrainerMon {
                    species: m.key.species,
                    form: m.key.form,
                    level: m.level,
                    iv: m.ivs[0],
                    ivs: m.ivs,
                    evs: m.evs,
                    moves,
                }
            })
            .collect();
        Some(Trainer {
            class: t.class_id as u8,
            name: self.catalog.trainer_names.get(usize::from(id))?.clone(),
            female: false,
            double: t.mode != 0,
            ai_flags: u32::from(t.ai),
            party,
        })
    }
    pub fn trainer_class_name(&self, class: u8) -> Option<String> {
        self.catalog
            .trainer_classes
            .get(usize::from(class))
            .cloned()
    }

    /// Versioned mod-owned habitats cover every base species and declared ordinary form.
    /// Imported source map/SOS tables retain their own identities in the private catalog.
    pub fn wild_maps(&self) -> Option<Vec<WildMap>> {
        let mut by_type: [Vec<SpeciesKey>; 18] = std::array::from_fn(|_| Vec::new());
        for form in super::data::species_forms(&self.catalog.personal).ok()? {
            if !super::data::ordinary_form(form.key) {
                continue;
            }
            let record = self.personal(form.key)?;
            by_type
                .get_mut(usize::from(record.types[0]))?
                .push(form.key);
        }
        let mut maps = Vec::with_capacity(18);
        for (type_id, forms) in by_type.into_iter().enumerate() {
            let mut tables = Vec::new();
            for group in forms.chunks(10) {
                let each = 100 / group.len() as u8;
                let remainder = 100 % group.len() as u8;
                let slots = group
                    .iter()
                    .enumerate()
                    .map(|(index, &key)| WildSlot {
                        species: key.species,
                        form: key.form,
                        min_level: 5,
                        max_level: 30,
                        chance: each + u8::from(index < usize::from(remainder)),
                    })
                    .collect();
                tables.push(WildTable {
                    method: if type_id == 10 {
                        WildMethod::Water
                    } else {
                        WildMethod::Land
                    },
                    rate: 20,
                    slots,
                });
            }
            maps.push(WildMap {
                group: 0,
                num: type_id as u8,
                tables,
            });
        }
        Some(maps)
    }
    pub fn map_section(&self, group: u8, num: u8) -> Option<u8> {
        (group == 0 && num < 18).then_some(num)
    }
    pub fn map_section_name(&self, section: u8) -> Option<String> {
        self.type_name(section)
            .map(|name| format!("{name} habitat"))
    }
}

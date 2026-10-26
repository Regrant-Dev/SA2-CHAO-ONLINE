//! Small private runtime catalog. Source parsing runs in a separate helper process.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::data::{
    self, DayNightEncounters, LearnedMove, MoveRecord, PersonalRecord, TrainerRecord,
};

pub const FORMAT: u32 = 1;
pub const PROFILE: &str = "base-v1";
pub const FILE: &str = "catalog.json";
pub const MAX_BYTES: usize = 16 << 20;
pub const READY_FILE: &str = "ready.json";
pub const MAX_READY_BYTES: usize = 16 << 10;
/// Versions of the mod's own rules built on the catalog: the type habitats and the strength
/// ladder. Peers compare them through `dex_print`; like the battle rules, they never make a
/// cache outdated.
pub const POLICIES: &str = "type-habitats-v2-ordinary-forms|strength-milestones-v1";

#[derive(Serialize, Deserialize)]
pub struct EncounterArea {
    pub source_entry: u16,
    pub tables: Vec<DayNightEncounters>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Catalog {
    pub source_sha256: String,
    pub personal: Vec<PersonalRecord>,
    pub moves: Vec<MoveRecord>,
    pub learnsets: Vec<Vec<LearnedMove>>,
    pub species_names: Vec<String>,
    pub form_names: Vec<String>,
    pub move_names: Vec<String>,
    pub move_descriptions: Vec<String>,
    pub ability_names: Vec<String>,
    pub ability_descriptions: Vec<String>,
    pub type_names: Vec<String>,
    pub trainer_names: Vec<String>,
    pub trainer_classes: Vec<String>,
    pub trainers: Vec<TrainerRecord>,
    pub encounters: Vec<EncounterArea>,
    pub teaching_moves: Vec<u16>,
    /// Attack-major 18 x 18 source table; multipliers are quarters (0,2,4,8).
    pub type_chart: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct Envelope {
    pub format: u32,
    pub profile: String,
    pub fingerprint: String,
    pub data: Catalog,
}

pub fn fingerprint(data: &Catalog) -> Result<String, String> {
    let mut normalized = serde_json::to_value(data).map_err(|e| e.to_string())?;
    let fields = normalized.as_object_mut().ok_or("Invalid catalog object")?;
    // Provenance/private cartridge headers, localization and presentation cannot change the
    // mechanics exchanged by peers. Preserve those fields in the cache, outside this hash.
    for key in [
        "source_sha256",
        "species_names",
        "form_names",
        "move_names",
        "move_descriptions",
        "ability_names",
        "ability_descriptions",
        "type_names",
        "trainer_names",
        "trainer_classes",
    ] {
        fields.remove(key);
    }
    if let Some(personal) = fields
        .get_mut("personal")
        .and_then(serde_json::Value::as_array_mut)
    {
        for record in personal {
            if let Some(record) = record.as_object_mut() {
                record.remove("form_sprite_index");
            }
        }
    }
    // Only the imported data and its layout: rules applied at runtime stay out, so changing them
    // never asks for a new import.
    let compatibility = serde_json::json!({"data_profile":PROFILE, "data":normalized});
    let bytes = serde_json::to_vec(&compatibility).map_err(|e| e.to_string())?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Published last by the helper. A catalog alone is not a complete runtime asset cache.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub format: u32,
    pub profile: String,
    pub fingerprint: String,
    pub catalog_sha256: String,
    pub actors: u16,
    pub variants: u16,
    pub asset_format: String,
    pub venue: u16,
}
impl Ready {
    pub fn new(fingerprint: &str, catalog_bytes: &[u8]) -> Self {
        Self {
            format: 1,
            profile: PROFILE.into(),
            fingerprint: fingerprint.into(),
            catalog_sha256: hex_digest(catalog_bytes),
            actors: data::MAX_SPECIES,
            variants: data::MAX_SPECIES * 2,
            asset_format: "PKA2".into(),
            venue: 14,
        }
    }
    pub fn validate(&self, fingerprint: &str, catalog_bytes: &[u8]) -> Result<(), String> {
        if self.format != 1
            || self.profile != PROFILE
            || self.fingerprint != fingerprint
            || self.catalog_sha256 != hex_digest(catalog_bytes)
            || self.actors != data::MAX_SPECIES
            || self.variants != data::MAX_SPECIES * 2
            || self.asset_format != "PKA2"
            || self.venue != 14
        {
            return Err("The private creature cache is incomplete or outdated; rebuild it from the source image".into());
        }
        Ok(())
    }
}
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
pub(super) fn test_catalog() -> Catalog {
    let mut personal_bytes = [0u8; data::PERSONAL_SIZE];
    personal_bytes[..6].fill(50);
    personal_bytes[0x18] = 1;
    personal_bytes[0x20] = 1;
    let personal = (0..=data::MAX_SPECIES)
        .map(|index| data::parse_personal(&personal_bytes, index).unwrap())
        .collect::<Vec<_>>();
    let mut move_bytes = [0u8; data::MOVE_SIZE];
    move_bytes[4] = 100;
    move_bytes[5] = 1;
    let moves = (0..=data::MAX_MOVE)
        .map(|id| data::parse_move(&move_bytes, id).unwrap())
        .collect();
    let empty_table = data::EncounterTable {
        min_level: 0,
        max_level: 0,
        rates: [0; 10],
        slots: (0..9)
            .map(|i| vec![None; if i == 8 { 6 } else { 10 }])
            .collect(),
    };
    Catalog {
        source_sha256: "11".repeat(32),
        learnsets: vec![Vec::new(); personal.len()],
        personal,
        moves,
        species_names: vec![String::new(); 808],
        form_names: Vec::new(),
        move_names: vec![String::new(); 729],
        move_descriptions: vec![String::new(); 729],
        ability_names: vec![String::new(); 234],
        ability_descriptions: vec![String::new(); 234],
        type_names: vec![String::new(); 18],
        trainer_names: vec![String::new(); 653],
        trainer_classes: vec![String::new(); 223],
        trainers: (0..653)
            .map(|id| data::TrainerRecord {
                id,
                class_id: 0,
                mode: 0,
                ai: 0,
                items: [0; 4],
                party: Vec::new(),
            })
            .collect(),
        encounters: vec![EncounterArea {
            source_entry: 9,
            tables: vec![data::DayNightEncounters {
                source_table: 0,
                day: empty_table.clone(),
                night: empty_table,
            }],
        }],
        teaching_moves: vec![1; 100],
        type_chart: vec![4; 18 * 18],
    }
}

impl Envelope {
    pub fn new(data: Catalog) -> Result<Self, String> {
        let result = Self {
            format: FORMAT,
            profile: PROFILE.into(),
            fingerprint: fingerprint(&data)?,
            data,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        let d = &self.data;
        if self.format != FORMAT || self.profile != PROFILE || self.fingerprint != fingerprint(d)? {
            return Err(
                "The local data cache is outdated or damaged; rebuild it from the source image"
                    .into(),
            );
        }
        let forms = data::species_forms(&d.personal)?;
        for (index, personal) in d.personal.iter().enumerate() {
            if usize::from(personal.source_index) != index
                || personal.types.iter().any(|&t| t >= data::TYPE_COUNT)
                || personal.abilities.iter().any(|&a| a > data::MAX_ABILITY)
                || personal.growth_rate >= 6
            {
                return Err(format!("Invalid cached personal record {index}"));
            }
        }
        let keys: std::collections::BTreeSet<_> = forms.iter().map(|form| form.key).collect();
        if d.moves.len() != usize::from(data::MAX_MOVE) + 1
            || d.learnsets.len() != d.personal.len()
            || d.species_names.len() != usize::from(data::MAX_SPECIES) + 1
            || d.move_names.len() != d.moves.len()
            || !(d.moves.len()..=d.moves.len() + 1).contains(&d.move_descriptions.len())
            || d.ability_names.len() != usize::from(data::MAX_ABILITY) + 1
            || d.ability_descriptions.len() != d.ability_names.len()
            || d.type_names.len() != usize::from(data::TYPE_COUNT)
            || d.trainers.len() != 653
            || d.trainer_names.len() != d.trainers.len()
            || d.trainer_classes.len() != 223
            || d.encounters.is_empty()
            || d.teaching_moves.len() != 100
            || d.type_chart.len() != 18 * 18
            || d.type_chart.iter().any(|n| ![0, 2, 4, 8].contains(n))
            || d.teaching_moves
                .iter()
                .any(|&id| id == 0 || id > data::MAX_MOVE)
            || d.source_sha256.len() != 64
            || !d.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(format!(
                "Incomplete or invalid cached tables: moves {} / names {} / descriptions {}, learnsets {} / personal {}, species names {}, ability names {}, types {}, trainers {} / names {}, teaching moves {} (invalid: {}), chart {} (invalid: {})",
                d.moves.len(),
                d.move_names.len(),
                d.move_descriptions.len(),
                d.learnsets.len(),
                d.personal.len(),
                d.species_names.len(),
                d.ability_names.len(),
                d.type_names.len(),
                d.trainers.len(),
                d.trainer_names.len(),
                d.teaching_moves.len(),
                d.teaching_moves
                    .iter()
                    .any(|&id| id == 0 || id > data::MAX_MOVE),
                d.type_chart.len(),
                d.type_chart.iter().any(|n| ![0, 2, 4, 8].contains(n))
            ));
        }
        for (id, m) in d.moves.iter().enumerate() {
            if usize::from(m.id) != id
                || m.policy != data::move_policy(m.id)
                || m.type_id >= data::TYPE_COUNT
                || m.accuracy > 101
                || m.pp > 40
                || !(-7..=7).contains(&m.priority)
            {
                return Err(format!("Invalid cached move {id}"));
            }
        }
        for (id, t) in d.trainers.iter().enumerate() {
            if usize::from(t.id) != id
                || usize::from(t.class_id) >= d.trainer_classes.len()
                || t.party.len() > 6
                || t.mode > 2
                || t.party.iter().any(|m| {
                    !keys.contains(&m.key)
                        || !(1..=100).contains(&m.level)
                        || m.ivs.iter().any(|&iv| iv > 31)
                        || m.ability_selector > 3
                        || m.gender_selector > 3
                        || (m.nature >= 25 && m.nature != 255)
                        || m.moves.iter().any(|&id| id > data::MAX_MOVE)
                })
            {
                return Err(format!("Invalid cached trainer {id}"));
            }
        }
        for learned in &d.learnsets {
            if learned.len() > 512
                || learned
                    .iter()
                    .any(|m| m.move_id == 0 || m.move_id > data::MAX_MOVE || m.level > 100)
            {
                return Err("Invalid cached level-up table".into());
            }
        }
        let mut areas = std::collections::HashSet::new();
        for area in &d.encounters {
            if !areas.insert(area.source_entry) || area.tables.len() > 4096 {
                return Err("Invalid cached encounter area".into());
            }
            for pair in &area.tables {
                for table in [&pair.day, &pair.night] {
                    let occupied = table.slots.iter().flatten().any(Option::is_some);
                    if table.slots.len() != 9
                        || table
                            .slots
                            .iter()
                            .enumerate()
                            .any(|(i, slots)| slots.len() != if i == 8 { 6 } else { 10 })
                        || table.rates.iter().any(|&n| n > 100)
                        || table
                            .slots
                            .iter()
                            .flatten()
                            .flatten()
                            .any(|key| !keys.contains(key))
                        || (occupied
                            && (!(1..=100).contains(&table.min_level)
                                || table.max_level < table.min_level
                                || table.max_level > 100))
                    {
                        return Err("Invalid cached day/night encounter table".into());
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "game-import")]
pub fn import(source: &super::source::GameSource) -> super::source::Result<Envelope> {
    use super::archive::{Garc, decompress_lz11};
    use super::source::ImportError;
    const LIMIT: usize = 32 << 20;
    let garc = |path| Garc::open(source, source.file(path)?);
    let personal = garc("a/0/1/7")?;
    let personal =
        data::parse_personal_table(&personal.read_entry(personal.entry_count() - 1, LIMIT)?)
            .map_err(ImportError::new)?;
    let moves = data::parse_move_archive(&garc("a/0/1/1")?.read_entry(0, LIMIT)?)
        .map_err(ImportError::new)?;
    let level_archive = garc("a/0/1/3")?;
    let learnsets = (0..personal.len())
        .map(|i| {
            data::parse_learnset(&level_archive.read_entry(i, LIMIT)?).map_err(ImportError::new)
        })
        .collect::<super::source::Result<Vec<_>>>()?;
    let text = garc("a/0/3/2")?;
    let names = |entry| data::parse_text(&text.read_entry(entry, LIMIT)?).map_err(ImportError::new);
    let headers = garc("a/1/0/6")?;
    let parties = garc("a/1/0/7")?;
    let trainers = (0..headers.entry_count())
        .map(|i| {
            let header = headers.read_entry(i, LIMIT)?;
            let party = parties.read_entry(i, LIMIT)?;
            // Record zero is a six-byte empty sentinel, not a party record.
            let bytes = if i == 0 && header.get(3) == Some(&0) && party == [0; 6] {
                &[][..]
            } else {
                &party
            };
            data::parse_trainer(&header, bytes, i as u16)
                .map_err(|e| ImportError::new(format!("trainer {i}: {e}")))
        })
        .collect::<super::source::Result<Vec<_>>>()?;
    let maps = garc("a/0/8/2")?;
    let mut encounters = Vec::new();
    for entry in (9..maps.entry_count()).step_by(11) {
        let bytes = maps.read_entry(entry, LIMIT)?;
        if bytes.is_empty() {
            continue;
        }
        let bytes = if bytes[0] == 0x11 {
            decompress_lz11(&bytes, LIMIT)?
        } else {
            bytes
        };
        if bytes.get(..2) != Some(b"EA") {
            continue;
        }
        let tables = data::parse_encounter_archive(&bytes).map_err(ImportError::new)?;
        if !tables.is_empty() {
            encounters.push(EncounterArea {
                source_entry: entry as u16,
                tables,
            });
        }
    }
    let code = source.code_bytes()?;
    let teaching_signature = [3, 0x40, 3, 0x41, 3, 0x42, 3, 0x43, 3];
    let find = |signature: &[u8]| -> super::source::Result<usize> {
        let positions: Vec<_> = code
            .windows(signature.len())
            .enumerate()
            .filter_map(|(i, b)| (b == signature).then_some(i))
            .collect();
        if positions.len() != 1 {
            return Err(ImportError::new(
                "source executable table signature is missing or ambiguous",
            ));
        }
        Ok(positions[0] + signature.len())
    };
    let at = find(&teaching_signature)? + 0x22;
    let teaching_moves = code
        .get(at..at + 200)
        .ok_or_else(|| ImportError::new("truncated teaching table"))?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let chart_signature: Vec<u8> = [
        2u32, 0, 1, 2, 0xc3, 0xcb, 0xd3, 0xdb, 0xf3, 0xfb, 0, 1, 2, 0, 1, 2,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let at = find(&chart_signature)?;
    let type_chart = code
        .get(at..at + 324)
        .ok_or_else(|| ImportError::new("truncated type chart"))?
        .to_vec();
    let abilities = names(101)?;
    // This source stores ability flavor text adjacent to ability names.
    let ability_descriptions = names(102)?;
    let data = Catalog {
        source_sha256: source
            .sha256()?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        personal,
        moves,
        learnsets,
        species_names: names(60)?,
        form_names: names(119)?,
        move_names: names(118)?,
        move_descriptions: names(117)?,
        ability_names: abilities,
        ability_descriptions,
        type_names: names(112)?,
        trainer_names: names(110)?,
        trainer_classes: names(111)?,
        trainers,
        encounters,
        teaching_moves,
        type_chart,
    };
    source.verify_unchanged()?;
    Envelope::new(data).map_err(ImportError::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gameplay_compatibility_ignores_provenance_text_and_art_but_not_stats() {
        let mut data = test_catalog();
        let original = fingerprint(&data).unwrap();
        data.source_sha256 = "ff".repeat(32);
        data.species_names[25] = "Localized label".into();
        data.move_descriptions[1] = "Display-only text".into();
        data.personal[25].form_sprite_index = 500;
        assert_eq!(fingerprint(&data).unwrap(), original);
        data.personal[25].weight_hg += 1;
        assert_ne!(fingerprint(&data).unwrap(), original);
    }
    #[test]
    fn cache_validation_rejects_wrapping_abilities_forms_and_encounter_keys() {
        let mut data = test_catalog();
        data.personal[25].abilities[0] = 257;
        assert!(Envelope::new(data).is_err());
        let mut data = test_catalog();
        data.personal[25].types[1] = 18;
        assert!(Envelope::new(data).is_err());
        let mut data = test_catalog();
        data.encounters[0].tables[0].day.slots[0][0] = Some(data::SpeciesKey {
            species: 25,
            form: 1,
        });
        assert!(Envelope::new(data).is_err());
        let mut data = test_catalog();
        data.trainers[1].party.push(data::TrainerCreature {
            key: data::SpeciesKey {
                species: 25,
                form: 1,
            },
            level: 5,
            ability_selector: 0,
            gender_selector: 0,
            nature: 0,
            ivs: [0; 6],
            evs: [0; 6],
            shiny: false,
            held_item: 0,
            moves: [1, 0, 0, 0],
        });
        assert!(Envelope::new(data).is_err());
        assert!(Envelope::new(test_catalog()).is_ok());
    }
    #[test]
    fn ready_marker_binds_complete_base_assets_to_exact_catalog_bytes() {
        let envelope = Envelope::new(test_catalog()).unwrap();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        let mut marker = Ready::new(&envelope.fingerprint, &bytes);
        assert!(marker.validate(&envelope.fingerprint, &bytes).is_ok());
        let mut altered = bytes.clone();
        altered.push(b' ');
        assert!(marker.validate(&envelope.fingerprint, &altered).is_err());
        marker.variants -= 1;
        assert!(marker.validate(&envelope.fingerprint, &bytes).is_err());
    }
}

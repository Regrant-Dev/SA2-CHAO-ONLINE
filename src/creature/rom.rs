//! Source-independent runtime data loaded from a private, versioned local cache.
//! The separate importer owns source-image reads and 3D conversion.
use super::{
    catalog::{self, Envelope},
    data::{self, MoveRecord, SpeciesKey},
};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const SPECIES_COUNT: u16 = 808;
pub const MOVE_COUNT: u16 = 729;
pub const TYPE_COUNT: u8 = 18;
pub const ABILITY_COUNT: u8 = 234;
pub const EFFECT_COUNT: u8 = 255;
pub const TEACHING_MOVE_COUNT: u8 = 100;
/// Move target bits (`MoveInfo::target`); 0 is one selected target.
pub const TARGET_SELECTED: u8 = 0;
/// Decided by the move's effect (the user's last attacker, or a random foe).
pub const TARGET_DEPENDS: u8 = 1 << 0;
/// The user or a chosen ally.
pub const TARGET_USER_OR_ALLY: u8 = 1 << 1;
pub const TARGET_RANDOM: u8 = 1 << 2;
/// Both foes.
pub const TARGET_BOTH_FOES: u8 = 1 << 3;
pub const TARGET_USER: u8 = 1 << 4;
/// Both foes and the user's ally.
pub const TARGET_ALL_OTHERS: u8 = 1 << 5;
/// The foes' side of the field (entry hazards and the like).
pub const TARGET_FOES_FIELD: u8 = 1 << 6;

/// Move flag bits (`MoveInfo::flags`).
pub const FLAG_CONTACT: u8 = 1 << 0;
/// Blocked by the protecting moves.
pub const FLAG_PROTECTABLE: u8 = 1 << 1;
/// Bounced back by the reflecting move.
pub const FLAG_REFLECTABLE: u8 = 1 << 2;
/// Taken by the stealing move.
pub const FLAG_STEALABLE: u8 = 1 << 3;
/// Repeatable by the copying move.
pub const FLAG_COPYABLE: u8 = 1 << 4;
/// May make the target flinch through the flinch-causing held item.
pub const FLAG_FLINCH_ITEM: u8 = 1 << 5;

/// A species record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Species {
    /// Primary and secondary type ids (equal for single-typed species).
    pub types: [u8; 2],
    /// Base HP, attack, defense, speed, sp. attack, sp. defense (catalog order).
    pub base: [u8; 6],
    /// Growth rate: index into the experience tables (0..6).
    pub growth: u8,
    /// Two ordinary ability slots and the hidden slot; zero means the slot is absent.
    pub abilities: [u8; 3],
    /// The XP a defeated one gives (before its level counts).
    pub exp_yield: u16,
}

/// A move record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveInfo {
    /// Effect id (0..EFFECT_COUNT); 0 is plain damage.
    pub effect: u8,
    /// Base power; 0 for status moves, 1 for moves whose effect sets the damage.
    pub power: u8,
    pub type_id: u8,
    /// Accuracy in percent; 0 never misses.
    pub accuracy: u8,
    pub pp: u8,
    /// Chance in percent that the effect happens on a hit; 0 for moves where it always does.
    pub effect_chance: u8,
    /// `TARGET_*` bits.
    pub target: u8,
    /// Higher goes first; 0 for most moves.
    pub priority: i8,
    /// `FLAG_*` bits.
    pub flags: u32,
    pub category: crate::battle::data::Category,
    pub details: crate::battle::data::MoveDetails,
}

/// One type matchup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeMatchup {
    pub attack: u8,
    pub defend: u8,
    /// Damage multiplier in tenths: 0, 5 or 20.
    pub tenths: u8,
    /// Not applied while the defender is identified (lets normal and fighting moves hit it).
    pub identify_removes: bool,
}

/// The type effectiveness chart. Pairs not listed are neutral.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeChart {
    /// Matchups in catalog order, applied in this order, rounding down after each.
    pub matchups: Vec<TypeMatchup>,
}

impl TypeChart {
    /// Multiplier in tenths (0, 5, 10 or 20) of `attack` against one defending type.
    pub fn multiplier(&self, attack: u8, defend: u8, identified: bool) -> u8 {
        self.matchups
            .iter()
            .find(|m| {
                m.attack == attack && m.defend == defend && !(identified && m.identify_removes)
            })
            .map_or(10, |m| m.tenths)
    }

    /// Combined multiplier in percent of `attack` against a defender's types (a repeated type
    /// counts once): 0, 25, 50, 100, 200 or 400.
    pub fn effectiveness(&self, attack: u8, defend: [u8; 2], identified: bool) -> u16 {
        let first = u16::from(self.multiplier(attack, defend[0], identified));
        let second = if defend[1] == defend[0] {
            10
        } else {
            u16::from(self.multiplier(attack, defend[1], identified))
        };
        first * second
    }
}

pub struct Rom {
    pub(super) catalog: catalog::Catalog,
    root: PathBuf,
    fingerprint: String,
    type_chart: TypeChart,
}

impl Rom {
    /// Loads only the bounded catalog, never a whole source image into the game process.
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = if path.is_dir() {
            path.join(catalog::FILE)
        } else {
            path.to_owned()
        };
        let bytes = read_cache_file(&path, catalog::MAX_BYTES)?;
        let mut result = Self::from_bytes(bytes)?;
        result.root = path.parent().unwrap_or(Path::new(".")).to_owned();
        Ok(result)
    }

    /// Game-side opening requires the helper's final publication marker and source venue.
    /// It reads at most the small catalog/marker; actor payloads validate lazily on use.
    pub fn open_ready(path: &Path) -> Result<Self, String> {
        let catalog_path = if path.is_dir() {
            path.join(catalog::FILE)
        } else {
            path.to_owned()
        };
        let root = catalog_path.parent().unwrap_or(Path::new("."));
        let bytes = read_cache_file(&catalog_path, catalog::MAX_BYTES)?;
        let mut result = Self::from_bytes(bytes.clone())?;
        let marker_bytes =
            read_cache_file(&root.join(catalog::READY_FILE), catalog::MAX_READY_BYTES)
                .map_err(|e| format!("The private cache is not ready: {e}"))?;
        let marker: catalog::Ready = serde_json::from_slice(&marker_bytes)
            .map_err(|e| format!("Invalid private cache readiness marker: {e}"))?;
        marker.validate(result.fingerprint(), &bytes)?;
        let venue = root.join("assets/venue/default.pka");
        let metadata = std::fs::metadata(&venue)
            .map_err(|e| format!("The imported battle venue is missing: {e}"))?;
        if !metadata.is_file()
            || metadata.len() < 16
            || metadata.len() > super::assets::MAX_FILE as u64
        {
            return Err("The imported battle venue is invalid; rebuild the private cache".into());
        }
        // The supported static venue is small (about 256 KiB); bounded decoding catches
        // interrupted/corrupt geometry before accepting a cache. Creature actors stay lazy.
        let venue_bytes = read_cache_file(&venue, 8 << 20)?;
        let venue = super::assets::Actor::decode(&venue_bytes)
            .map_err(|e| format!("The imported battle venue is damaged: {e}"))?;
        if venue.species != 0 || venue.form != 0 || venue.meshes.is_empty() {
            return Err("The imported battle venue has the wrong identity or no geometry".into());
        }
        result.root = root.to_owned();
        Ok(result)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() > catalog::MAX_BYTES {
            return Err("Local data cache is too large".into());
        }
        let envelope: Envelope =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid local data cache: {e}"))?;
        envelope.validate()?;
        let mut matchups = Vec::new();
        for (pair, &factor) in envelope.data.type_chart.iter().enumerate() {
            if factor == 4 {
                continue;
            }
            let attack = (pair / 18) as u8;
            let defend = (pair % 18) as u8;
            matchups.push(TypeMatchup {
                attack,
                defend,
                tenths: factor * 10 / 4,
                identify_removes: defend == 7 && [0, 1].contains(&attack) && factor == 0,
            });
        }
        // Identification immunity exceptions go last, matching the engine chart contract.
        matchups.sort_by_key(|m| m.identify_removes);
        Ok(Self {
            catalog: envelope.data,
            root: PathBuf::new(),
            fingerprint: envelope.fingerprint,
            type_chart: TypeChart { matchups },
        })
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn asset_root(&self) -> PathBuf {
        self.root.join("assets")
    }
    pub(super) fn species_exists(&self, id: u16) -> bool {
        id > 0 && id < SPECIES_COUNT
    }
    pub fn personal(&self, key: SpeciesKey) -> Option<&data::PersonalRecord> {
        if !self.species_exists(key.species) {
            return None;
        }
        let base = self.catalog.personal.get(usize::from(key.species))?;
        if key.form >= base.form_count {
            return None;
        }
        let index = if key.form == 0 || base.form_stats_index == 0 {
            key.species
        } else {
            base.form_stats_index + u16::from(key.form) - 1
        };
        self.catalog.personal.get(usize::from(index))
    }
    pub fn species_form(&self, key: SpeciesKey) -> Option<Species> {
        if !data::ordinary_form(key) {
            return None;
        }
        let p = self.personal(key)?;
        Some(Species {
            types: p.types,
            base: p.base_stats,
            growth: p.growth_rate,
            abilities: p.abilities.map(|a| a as u8),
            exp_yield: p.base_exp,
        })
    }
    pub fn species(&self, id: u16) -> Option<Species> {
        self.species_form(SpeciesKey {
            species: id,
            form: 0,
        })
    }
    pub fn species_keys(&self) -> Vec<SpeciesKey> {
        data::species_forms(&self.catalog.personal)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.key)
            .filter(|&key| data::ordinary_form(key))
            .collect()
    }

    /// Size-only forms share source geometry; their authored dimensions stay distinct.
    pub fn size_ratio(&self, key: SpeciesKey) -> f32 {
        if ![710, 711].contains(&key.species) || key.form == 0 {
            return 1.;
        }
        let base = self
            .personal(SpeciesKey {
                species: key.species,
                form: 0,
            })
            .map_or(0, |p| p.height_cm);
        let form = self.personal(key).map_or(base, |p| p.height_cm);
        if base == 0 {
            1.
        } else {
            (f32::from(form) / f32::from(base)).clamp(0.25, 4.)
        }
    }
    pub fn species_name(&self, id: u16) -> Option<String> {
        self.species_exists(id)
            .then(|| self.catalog.species_names.get(usize::from(id)).cloned())
            .flatten()
    }
    pub fn species_label(&self, key: SpeciesKey) -> Option<String> {
        let name = self.species_name(key.species)?;
        let personal = self.personal(key)?;
        if key.form == 0 {
            Some(name)
        } else if personal.regional_variant {
            Some(format!("{name} (regional)"))
        } else {
            Some(format!("{name} (form {})", key.form))
        }
    }
    pub fn modern_move(&self, id: u16) -> Option<&MoveRecord> {
        if id == 0 {
            return None;
        }
        self.catalog.moves.get(usize::from(id))
    }
    pub fn move_name(&self, id: u16) -> Option<String> {
        if id == 0 {
            return None;
        }
        self.catalog.move_names.get(usize::from(id)).cloned()
    }
    pub fn move_info(&self, id: u16) -> Option<MoveInfo> {
        let m = crate::battle::source_rules::compile_move(self.modern_move(id)?)?;
        Some(MoveInfo {
            effect: m.effect,
            power: m.power,
            type_id: m.type_id,
            category: m.category,
            accuracy: m.accuracy,
            pp: m.pp,
            effect_chance: m.chance,
            target: m.target.to_rom(),
            priority: m.priority,
            flags: m.flags,
            details: m.details,
        })
    }
    pub fn move_description(&self, id: u16) -> Option<String> {
        self.catalog.move_descriptions.get(usize::from(id)).cloned()
    }
    pub fn sound_moves(&self) -> Vec<u16> {
        self.catalog
            .moves
            .iter()
            .filter(|m| m.flags & (1 << 8) != 0)
            .map(|m| m.id)
            .collect()
    }
    pub fn type_chart(&self) -> &TypeChart {
        &self.type_chart
    }
    pub fn ability_name(&self, id: u8) -> Option<String> {
        self.catalog.ability_names.get(usize::from(id)).cloned()
    }
    pub fn ability_description(&self, id: u8) -> Option<String> {
        self.catalog
            .ability_descriptions
            .get(usize::from(id))
            .cloned()
    }
    pub fn teaching_move(&self, index: u8) -> Option<u16> {
        self.catalog
            .teaching_moves
            .get(usize::from(index))
            .copied()
            .filter(|&id| self.move_info(id).is_some())
    }
    pub fn type_name(&self, id: u8) -> Option<String> {
        self.catalog.type_names.get(usize::from(id)).cloned()
    }
    pub fn learnset_form(&self, key: SpeciesKey) -> Vec<(u8, u16)> {
        self.personal(key)
            .and_then(|p| self.catalog.learnsets.get(usize::from(p.source_index)))
            .map(|moves| {
                moves
                    .iter()
                    .filter(|m| self.move_info(m.move_id).is_some())
                    .map(|m| (m.level, m.move_id))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn learnset(&self, species: u16) -> Vec<(u8, u16)> {
        self.learnset_form(SpeciesKey { species, form: 0 })
    }
    pub fn moves_at_level_form(&self, key: SpeciesKey, level: u8) -> [u16; 4] {
        let mut known = Vec::new();
        for (lv, id) in self.learnset_form(key) {
            if lv == 0 || lv > level {
                continue;
            }
            if !known.contains(&id) {
                known.push(id);
            }
        }
        let mut result = [0; 4];
        let start = known.len().saturating_sub(4);
        for (to, from) in result.iter_mut().zip(&known[start..]) {
            *to = *from;
        }
        result
    }
    pub fn moves_at_level(&self, species: u16, level: u8) -> [u16; 4] {
        self.moves_at_level_form(SpeciesKey { species, form: 0 }, level)
    }
    pub fn starters(&self) -> Option<[u16; 3]> {
        Some([722, 725, 728])
    }
}

fn read_cache_file(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err("Cache file is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Cache file changed beyond its size bound".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    #[test]
    fn runtime_loading_refuses_unpublished_or_corrupt_venue_caches() {
        let root =
            std::env::temp_dir().join(format!("sa2co-cache-readiness-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/venue")).unwrap();
        let envelope = catalog::Envelope::new(catalog::test_catalog()).unwrap();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        std::fs::write(root.join(catalog::FILE), &bytes).unwrap();
        assert!(Rom::open(&root).is_ok());
        assert!(Rom::open_ready(&root).is_err());
        let marker = catalog::Ready::new(&envelope.fingerprint, &bytes);
        std::fs::write(
            root.join(catalog::READY_FILE),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        assert!(Rom::open_ready(&root).is_err());
        std::fs::write(
            root.join("assets/venue/default.pka"),
            [b"PKA2".as_slice(), &[0; 32]].concat(),
        )
        .unwrap();
        assert!(Rom::open_ready(&root).is_err());
        // A marker cannot trigger an unbounded read, even if catalog bytes are valid.
        std::fs::File::create(root.join(catalog::READY_FILE))
            .unwrap()
            .set_len(catalog::MAX_READY_BYTES as u64 + 1)
            .unwrap();
        assert!(Rom::open_ready(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

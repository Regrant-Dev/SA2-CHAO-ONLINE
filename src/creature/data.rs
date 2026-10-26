//! Checked, source-independent identities and the source data adapters.
//!
//! The parsers follow record layout notes (format descriptions, not imported game data) for the
//! personal, move, learnset and trainer records. The importer supplies bounded archive entries;
//! this module never opens a game image, talks to a server, or changes the running game.

pub const MAX_SPECIES: u16 = 807;
pub const MAX_MOVE: u16 = 728;
pub const MAX_ABILITY: u16 = 233;
pub const TYPE_COUNT: u8 = 18;
pub const PERSONAL_SIZE: usize = 0x54;
pub const MOVE_SIZE: usize = 0x28;
const MAX_RECORDS: usize = 4096;
const MAX_LEARNED: usize = 512;

type Result<T> = std::result::Result<T, String>;

/// National species id and an explicit source-defined form, never a personal/model index.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct SpeciesKey {
    pub species: u16,
    pub form: u8,
}

/// Cosmetic and regional identities keep their explicit form. Power-up transformations,
/// forms that only exist mid-battle and forms that come from a held item are outside the
/// ordinary, itemless battle format, including when received over the wire: a fighter could
/// neither enter nor keep them, so they are refused rather than quietly read as another form.
pub fn ordinary_form(key: SpeciesKey) -> bool {
    if key.species == 0 || key.species > MAX_SPECIES || key.form > 31 {
        return false;
    }
    if key.form == 0 {
        return true;
    }
    if [
        3, 6, 9, 15, 18, 65, 80, 94, 115, 127, 130, 142, 150, 181, 208, 212, 214, 229, 248, 254,
        257, 260, 282, 302, 303, 306, 308, 310, 319, 323, 334, 354, 359, 362, 373, 376, 380, 381,
        382, 383, 384, 428, 445, 448, 460, 475, 531, 719,
    ]
    .contains(&key.species)
    {
        return false;
    }
    if matches!(
        (key.species, key.form),
        (20, 2) | (105, 2) | (735 | 738 | 743 | 752 | 754 | 758 | 777 | 784, 1)
    ) {
        return false;
    }
    if matches!(key.species, 658)
        || (key.species == 718 && key.form >= 2)
        || (key.species == 778 && key.form >= 1)
    {
        return false;
    }
    // Battle-only forms (weather, sunshine, low HP, a move, a stance, a shell breaking) and
    // held-item forms (an orb, a plate, a drive, a memory).
    if matches!(
        (key.species, key.form),
        (351, 1..=3) | (421 | 487 | 555 | 648 | 681 | 746, 1) | (493 | 649 | 773, 1..) | (774, 7..)
    ) {
        return false;
    }
    !matches!((key.species, key.form), (800, 3))
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpecialPowerUp {
    pub item: u16,
    pub base_move: u16,
    pub power_up_move: u16,
}

/// Gameplay fields of one source personal record. Its index alone is not a species id.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PersonalRecord {
    pub source_index: u16,
    /// HP, attack, defense, speed, special attack, special defense (the source's stat order).
    pub base_stats: [u8; 6],
    /// Modern type ids: Fire=9, Dark=16, Fairy=17; no retired mystery-type slot.
    pub types: [u8; 2],
    /// The two ordinary slots and the hidden slot. Values are ability ids, not slot numbers.
    pub abilities: [u16; 3],
    pub gender_ratio: u8,
    pub base_friendship: u8,
    pub growth_rate: u8,
    pub base_exp: u16,
    pub height_cm: u16,
    pub weight_hg: u16,
    pub form_count: u8,
    pub form_stats_index: u16,
    pub form_sprite_index: u16,
    pub regional_variant: bool,
    pub tm_bits: [u8; 16],
    pub type_tutor_bits: [u8; 4],
    /// The complete source span, reserved bits included.
    pub special_tutor_bits: [u8; 16],
    /// Preserved for auditing. Importing this does not enable a power-up move mechanic.
    pub special_power_up: SpecialPowerUp,
}

/// A catalog identity and the record supplying its stats. Cosmetic forms may share a record.
/// This is source coverage, not approval of a form for the mod's battle rules.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeciesForm {
    pub key: SpeciesKey,
    pub personal_index: u16,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveCategory {
    Status,
    Physical,
    Special,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovePolicy {
    /// Ordinary data; the battle rules decide separately whether the engine supports it.
    Ordinary,
    ExcludedPowerUpMove,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerUpMeta {
    pub move_id: u16,
    pub power: u8,
    pub effect: u8,
}

/// Source data. Its effect-sequence number is kept as data and never read as an engine effect.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MoveRecord {
    pub id: u16,
    pub type_id: u8,
    pub category: MoveCategory,
    pub quality: u8,
    pub power: u8,
    /// Source accuracy: 0..=100 percent; 101 means unconditional, not a 101% roll.
    pub accuracy: u8,
    pub pp: u8,
    pub priority: i8,
    pub min_hits: u8,
    pub max_hits: u8,
    pub inflict: u16,
    pub inflict_percent: u8,
    pub duration_raw: u8,
    pub min_turns: u8,
    pub max_turns: u8,
    pub crit_stage: u8,
    pub flinch_percent: u8,
    pub effect_sequence: u16,
    pub recoil_raw: u8,
    pub healing_raw: u8,
    pub target_raw: u8,
    pub stat_ids: [u8; 3],
    pub stat_stages: [i8; 3],
    pub stat_percents: [u8; 3],
    /// Most ordinary moves also have power-up metadata. It must not make them excluded power-up
    /// moves.
    pub power_up: PowerUpMeta,
    pub refresh_affliction_raw: u8,
    pub refresh_percent: u8,
    pub flags: u32,
    pub policy: MovePolicy,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LearnedMove {
    pub move_id: u16,
    /// Zero is an evolution reminder entry; preserve it rather than losing that move.
    pub level: u8,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TrainerCreature {
    pub key: SpeciesKey,
    pub level: u8,
    /// Preserve source selectors: zero can request the game's generated choice.
    pub ability_selector: u8,
    pub gender_selector: u8,
    pub nature: u8,
    /// HP, attack, defense, speed, special attack, special defense.
    pub ivs: [u8; 6],
    pub evs: [u8; 6],
    pub shiny: bool,
    pub held_item: u16,
    /// Zero slots request source learnset moves; do not invent them during parsing.
    pub moves: [u16; 4],
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TrainerRecord {
    pub id: u16,
    pub class_id: u16,
    /// Source battle mode: zero singles, one doubles, two multi.
    pub mode: u8,
    pub ai: u8,
    pub items: [u16; 4],
    pub party: Vec<TrainerCreature>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EncounterTable {
    pub min_level: u8,
    pub max_level: u8,
    pub rates: [u8; 10],
    /// Base slots, seven SOS slot sets, and six additional weather SOS slots.
    pub slots: Vec<Vec<Option<SpeciesKey>>>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DayNightEncounters {
    pub source_table: u16,
    pub day: EncounterTable,
    pub night: EncounterTable,
}

fn u16_at(data: &[u8], at: usize) -> Result<u16> {
    let bytes = data.get(at..at + 2).ok_or("truncated 16-bit data field")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Result<u32> {
    let bytes = data.get(at..at + 4).ok_or("truncated 32-bit data field")?;
    Ok(u32::from_le_bytes(
        bytes.try_into().map_err(|_| "truncated data field")?,
    ))
}

pub fn parse_personal(data: &[u8], source_index: u16) -> Result<PersonalRecord> {
    if data.len() != PERSONAL_SIZE {
        return Err(format!(
            "personal record {source_index}: expected {PERSONAL_SIZE} bytes, got {}",
            data.len()
        ));
    }
    let types = [data[6], data[7]];
    let abilities = [
        u16::from(data[0x18]),
        u16::from(data[0x19]),
        u16::from(data[0x1A]),
    ];
    if types.iter().any(|&t| t >= TYPE_COUNT)
        || abilities.iter().any(|&a| a > MAX_ABILITY)
        || data[0x15] >= 6
        || data[0x52] > 1
    {
        return Err(format!(
            "personal record {source_index}: invalid types, abilities, growth rate or regional flag"
        ));
    }
    Ok(PersonalRecord {
        source_index,
        base_stats: data[..6].try_into().map_err(|_| "missing base stats")?,
        types,
        abilities,
        gender_ratio: data[0x12],
        base_friendship: data[0x14],
        growth_rate: data[0x15],
        base_exp: u16_at(data, 0x22)?,
        height_cm: u16_at(data, 0x24)?,
        weight_hg: u16_at(data, 0x26)?,
        form_count: data[0x20],
        form_stats_index: u16_at(data, 0x1C)?,
        form_sprite_index: u16_at(data, 0x1E)?,
        regional_variant: data[0x52] == 1,
        tm_bits: data[0x28..0x38]
            .try_into()
            .map_err(|_| "missing teaching bits")?,
        type_tutor_bits: data[0x38..0x3C]
            .try_into()
            .map_err(|_| "missing tutor bits")?,
        special_tutor_bits: data[0x3C..0x4C]
            .try_into()
            .map_err(|_| "missing special tutor bits")?,
        special_power_up: SpecialPowerUp {
            item: u16_at(data, 0x4C)?,
            base_move: u16_at(data, 0x4E)?,
            power_up_move: u16_at(data, 0x50)?,
        },
    })
}

/// The personal GARC's final entry is the concatenated table, including record zero.
pub fn parse_personal_table(data: &[u8]) -> Result<Vec<PersonalRecord>> {
    if !data.len().is_multiple_of(PERSONAL_SIZE)
        || !(usize::from(MAX_SPECIES) + 1..=MAX_RECORDS).contains(&(data.len() / PERSONAL_SIZE))
    {
        return Err(
            "personal table has an invalid length or does not cover all 807 species".into(),
        );
    }
    data.as_chunks::<PERSONAL_SIZE>()
        .0
        .iter()
        .enumerate()
        .map(|(i, record)| parse_personal(record, i as u16))
        .collect()
}

/// Resolve every declared form without guessing that a model/archive index is its species.
pub fn species_forms(records: &[PersonalRecord]) -> Result<Vec<SpeciesForm>> {
    if records.len() <= usize::from(MAX_SPECIES) || records.len() > MAX_RECORDS {
        return Err("personal catalog does not contain the supported 807 base species".into());
    }
    let mut forms = Vec::new();
    for species in 1..=MAX_SPECIES {
        let base = &records[usize::from(species)];
        if base.source_index != species
            || base.form_count == 0
            || base.base_stats.contains(&0)
            || base.abilities[0] == 0
        {
            return Err(format!(
                "species {species}: missing or invalid base personal record"
            ));
        }
        for form in 0..base.form_count {
            let index = if form == 0 || base.form_stats_index == 0 {
                species
            } else {
                base.form_stats_index
                    .checked_add(u16::from(form) - 1)
                    .ok_or_else(|| format!("species {species}: overflowing form index"))?
            };
            let record = records.get(usize::from(index)).ok_or_else(|| {
                format!("species {species} form {form}: personal index {index} is out of bounds")
            })?;
            if record.source_index != index
                || record.base_stats.contains(&0)
                || record.abilities[0] == 0
            {
                return Err(format!(
                    "species {species} form {form}: invalid personal record"
                ));
            }
            forms.push(SpeciesForm {
                key: SpeciesKey { species, form },
                personal_index: index,
            });
        }
    }
    Ok(forms)
}

/// The source's own power-up move ids, not ordinary moves which merely carry power-up metadata.
pub fn move_policy(id: u16) -> MovePolicy {
    match id {
        622..=658 | 695..=703 | 719 | 723..=728 => MovePolicy::ExcludedPowerUpMove,
        _ => MovePolicy::Ordinary,
    }
}

pub fn parse_move(data: &[u8], id: u16) -> Result<MoveRecord> {
    if data.len() != MOVE_SIZE || id > MAX_MOVE {
        return Err(format!(
            "move {id}: unsupported id or record length {}",
            data.len()
        ));
    }
    let category = match data[2] {
        0 => MoveCategory::Status,
        1 => MoveCategory::Physical,
        2 => MoveCategory::Special,
        _ => return Err(format!("move {id}: invalid damage category")),
    };
    if data[0] >= TYPE_COUNT
        || data[4] > 101
        || data[5] > 40
        || !(-7..=7).contains(&(data[6] as i8))
    {
        return Err(format!("move {id}: invalid type, accuracy, PP or priority"));
    }
    Ok(MoveRecord {
        id,
        type_id: data[0],
        category,
        quality: data[1],
        power: data[3],
        accuracy: data[4],
        pp: data[5],
        priority: data[6] as i8,
        min_hits: data[7] & 15,
        max_hits: data[7] >> 4,
        inflict: u16_at(data, 8)?,
        inflict_percent: data[0xA],
        duration_raw: data[0xB],
        min_turns: data[0xC],
        max_turns: data[0xD],
        crit_stage: data[0xE],
        flinch_percent: data[0xF],
        effect_sequence: u16_at(data, 0x10)?,
        recoil_raw: data[0x12],
        healing_raw: data[0x13],
        target_raw: data[0x14],
        stat_ids: [data[0x15], data[0x16], data[0x17]],
        stat_stages: [data[0x18] as i8, data[0x19] as i8, data[0x1A] as i8],
        stat_percents: [data[0x1B], data[0x1C], data[0x1D]],
        power_up: PowerUpMeta {
            move_id: u16_at(data, 0x1E)?,
            power: data[0x20],
            effect: data[0x21],
        },
        refresh_affliction_raw: data[0x22],
        refresh_percent: data[0x23],
        flags: u32_at(data, 0x24)?,
        policy: move_policy(id),
    })
}

/// Small typed container inside a GARC entry: two-byte magic, u16 count, count+1 offsets.
pub fn mini_entries<'a>(data: &'a [u8], magic: &[u8; 2]) -> Result<Vec<&'a [u8]>> {
    if data.get(..2) != Some(magic.as_slice()) {
        return Err("unexpected mini archive magic".into());
    }
    let count = usize::from(u16_at(data, 2)?);
    if count == 0 || count > MAX_RECORDS {
        return Err("invalid mini archive entry count".into());
    }
    let table_end = 4 + (count + 1) * 4;
    if table_end > data.len() {
        return Err("truncated mini archive offset table".into());
    }
    let offsets = (0..=count)
        .map(|i| u32_at(data, 4 + i * 4).map(|n| n as usize))
        .collect::<Result<Vec<_>>>()?;
    if offsets[0] < table_end
        || offsets.last().is_none_or(|&last| last > data.len())
        || offsets.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err("mini archive has overlapping, reversed or out-of-range offsets".into());
    }
    Ok(offsets
        .windows(2)
        .map(|pair| &data[pair[0]..pair[1]])
        .collect())
}

pub fn parse_move_archive(data: &[u8]) -> Result<Vec<MoveRecord>> {
    let entries = mini_entries(data, b"WD")?;
    if entries.len() != usize::from(MAX_MOVE) + 1 {
        return Err(format!(
            "expected {} move records, got {}",
            MAX_MOVE + 1,
            entries.len()
        ));
    }
    entries
        .into_iter()
        .enumerate()
        .map(|(id, record)| parse_move(record, id as u16))
        .collect()
}

/// Level-up pairs are u16 move + u16 level, terminated by FFFF FFFF.
pub fn parse_learnset(data: &[u8]) -> Result<Vec<LearnedMove>> {
    if !data.len().is_multiple_of(4) || data.len() < 4 || data.len() > (MAX_LEARNED + 1) * 4 {
        return Err("invalid learnset length".into());
    }
    if data[data.len() - 4..] != [255; 4] {
        return Err("learnset is missing its terminator".into());
    }
    let mut moves = Vec::new();
    for pair in data[..data.len() - 4].as_chunks::<4>().0 {
        let move_id = u16_at(pair, 0)?;
        let level = u16_at(pair, 2)?;
        if move_id == 0 || move_id > MAX_MOVE || level > 100 {
            return Err("learnset contains an invalid move or level".into());
        }
        moves.push(LearnedMove {
            move_id,
            level: level as u8,
        });
    }
    Ok(moves)
}

/// Name tables use one section, rotating-XOR UTF-16, and per-line offsets.
/// Unknown text variables stay visible as markers rather than being silently discarded.
pub fn parse_text(data: &[u8]) -> Result<Vec<String>> {
    const MAX_TEXT_BYTES: usize = 16 << 20;
    const MAX_TEXT_LINES: usize = 8192;
    if data.len() < 20
        || data.len() > MAX_TEXT_BYTES
        || u16_at(data, 0)? != 1
        || u32_at(data, 8)? != 0
        || u32_at(data, 12)? != 16
        || u32_at(data, 4)? as usize != data.len() - 16
        || u32_at(data, 16)? != u32_at(data, 4)?
    {
        return Err("unsupported or truncated text header".into());
    }
    let count = usize::from(u16_at(data, 2)?);
    let table_end = 20 + count * 8;
    if count > MAX_TEXT_LINES || table_end > data.len() {
        return Err("invalid text line count or truncated offset table".into());
    }
    let mut lines = Vec::with_capacity(count);
    let mut total_units = 0usize;
    for line in 0..count {
        let start = (u32_at(data, 20 + line * 8)? as usize)
            .checked_add(16)
            .ok_or("overflowing text line offset")?;
        let units = usize::from(u16_at(data, 24 + line * 8)?);
        total_units += units;
        if start < table_end || !start.is_multiple_of(2) || units > 4096 || total_units > 2 << 20 {
            return Err("text line offset, length or total output exceeds bounds".into());
        }
        let end = start
            .checked_add(units * 2)
            .ok_or("overflowing text line length")?;
        let encrypted = data.get(start..end).ok_or("text line exceeds its file")?;
        let mut key = 0x7C89u16.wrapping_add((line as u16).wrapping_mul(0x2983));
        let words = encrypted
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let word = u16::from_le_bytes([pair[0], pair[1]]) ^ key;
                key = key.rotate_left(3);
                word
            })
            .collect::<Vec<_>>();
        lines.push(text_line(&words)?);
    }
    Ok(lines)
}

fn text_line(words: &[u16]) -> Result<String> {
    let mut text = String::new();
    let mut plain = Vec::new();
    let flush = |text: &mut String, plain: &mut Vec<u16>| -> Result<()> {
        text.push_str(&String::from_utf16(plain).map_err(|_| "invalid UTF-16 in text line")?);
        plain.clear();
        Ok(())
    };
    let mut at = 0usize;
    while let Some(&word) = words.get(at) {
        at += 1;
        match word {
            0 => {
                flush(&mut text, &mut plain)?;
                return Ok(text);
            }
            0x10 => {
                flush(&mut text, &mut plain)?;
                let count = usize::from(*words.get(at).ok_or("truncated text variable count")?);
                let code = *words.get(at + 1).ok_or("truncated text variable code")?;
                at += 2;
                if count == 0 || count > 64 {
                    return Err("invalid text variable length".into());
                }
                // Native wait/null controls carry one argument even with a count of one.
                let args = if matches!(code, 0xBE02 | 0xBDFF) {
                    1
                } else {
                    count - 1
                };
                let parameters = words
                    .get(at..at + args)
                    .ok_or("truncated text variable arguments")?;
                text.push_str(&format!("[VAR {code:04X}"));
                for argument in parameters {
                    text.push_str(&format!(" {argument:04X}"));
                }
                text.push(']');
                at += args;
            }
            // Private font glyphs used in species names and punctuation.
            0xE07F => plain.push(0x202F),
            0xE08D => plain.push(0x2026),
            0xE08E => plain.push(0x2642),
            0xE08F => plain.push(0x2640),
            _ => plain.push(word),
        }
    }
    if words.is_empty() {
        return Ok(String::new());
    }
    Err("text line has no terminator".into())
}

pub fn parse_trainer(header: &[u8], party: &[u8], id: u16) -> Result<TrainerRecord> {
    if header.len() != 0x14 {
        return Err("invalid trainer header length".into());
    }
    let count = usize::from(header[3]);
    if count > 6 || header[2] > 2 || party.len() != count * 0x20 {
        return Err("invalid trainer mode, party count or party byte length".into());
    }
    let mut members = Vec::with_capacity(count);
    for data in party.as_chunks::<0x20>().0 {
        let species = u16_at(data, 0x10)?;
        let moves = [
            u16_at(data, 0x18)?,
            u16_at(data, 0x1A)?,
            u16_at(data, 0x1C)?,
            u16_at(data, 0x1E)?,
        ];
        if !(1..=MAX_SPECIES).contains(&species)
            || !(1..=100).contains(&data[0xE])
            || (data[1] > 24 && data[1] != 255)
            || moves.iter().any(|&id| id > MAX_MOVE)
        {
            return Err("trainer party contains an invalid species, level, nature or move".into());
        }
        let ivs = u32_at(data, 8)?;
        members.push(TrainerCreature {
            key: SpeciesKey {
                species,
                form: data[0x12],
            },
            level: data[0xE],
            ability_selector: (data[0] >> 4) & 3,
            gender_selector: data[0] & 3,
            nature: data[1],
            ivs: [0, 5, 10, 25, 15, 20].map(|shift| ((ivs >> shift) & 31) as u8),
            evs: [data[2], data[3], data[4], data[7], data[5], data[6]],
            shiny: ivs & (1 << 30) != 0,
            held_item: u16_at(data, 0x14)?,
            moves,
        });
    }
    Ok(TrainerRecord {
        id,
        class_id: u16_at(header, 0)?,
        mode: header[2],
        ai: header[0xC],
        items: [
            u16_at(header, 4)?,
            u16_at(header, 6)?,
            u16_at(header, 8)?,
            u16_at(header, 0xA)?,
        ],
        party: members,
    })
}

pub fn parse_encounter_table(data: &[u8]) -> Result<EncounterTable> {
    if data.len() != 0x164 {
        return Err("invalid encounter table length".into());
    }
    let mut slots = Vec::with_capacity(9);
    for group in 0..9 {
        let (start, count) = if group == 8 {
            (0x14C, 6)
        } else {
            (0xC + group * 40, 10)
        };
        let mut group_slots = Vec::with_capacity(count);
        for slot in 0..count {
            let packed = u32_at(data, start + slot * 4)?;
            let species = (packed & 0x7FF) as u16;
            let form = ((packed >> 11) & 31) as u8;
            if species > MAX_SPECIES || (species == 0 && form != 0) || packed >> 16 != 0 {
                return Err("encounter slot has an invalid species, form or reserved bits".into());
            }
            group_slots.push((species != 0).then_some(SpeciesKey { species, form }));
        }
        slots.push(group_slots);
    }
    let occupied = slots.iter().flatten().any(Option::is_some);
    if (occupied && (!(1..=100).contains(&data[0]) || data[1] < data[0] || data[1] > 100))
        || data[2..12].iter().any(|&rate| rate > 100)
    {
        return Err("invalid encounter level range or slot rates".into());
    }
    Ok(EncounterTable {
        min_level: data[0],
        max_level: data[1],
        rates: data[2..12]
            .try_into()
            .map_err(|_| "missing encounter rates")?,
        slots,
    })
}

pub fn parse_encounter_archive(data: &[u8]) -> Result<Vec<DayNightEncounters>> {
    let mut tables = Vec::new();
    for (index, entry) in mini_entries(data, b"EA")?.into_iter().enumerate() {
        if entry.is_empty() {
            continue;
        }
        // The source aligns EA members to 128 bytes; the night table's trailing padding
        // is not another table. Preserve the exact day/night boundaries.
        if entry.len() < 0x2CC || entry.len() > 0x300 || entry[0x2CC..].iter().any(|&b| b != 0) {
            return Err("encounter day/night entry has an invalid length or padding".into());
        }
        tables.push(DayNightEncounters {
            source_table: index as u16,
            day: parse_encounter_table(&entry[4..0x168])?,
            night: parse_encounter_table(&entry[0x168..0x2CC])?,
        });
    }
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn personal() -> [u8; PERSONAL_SIZE] {
        let mut data = [0; PERSONAL_SIZE];
        data[..6].copy_from_slice(&[40, 45, 35, 90, 50, 45]);
        data[6] = 12;
        data[7] = 12;
        data[0x18..0x1B].copy_from_slice(&[9, 9, 31]);
        data[0x20] = 1;
        data[0x22..0x24].copy_from_slice(&395u16.to_le_bytes());
        data
    }

    #[test]
    fn personal_preserves_hidden_ability_wide_exp_and_power_up_metadata() {
        let mut data = personal();
        data[0x4C..0x52].copy_from_slice(&[4, 3, 85, 0, 207, 2]);
        let got = parse_personal(&data, 25).unwrap();
        assert_eq!(got.base_exp, 395);
        assert_eq!(got.abilities, [9, 9, 31]);
        assert_eq!(
            got.special_power_up,
            SpecialPowerUp {
                item: 772,
                base_move: 85,
                power_up_move: 719
            }
        );
        for len in 0..PERSONAL_SIZE {
            assert!(parse_personal(&data[..len], 25).is_err());
        }
        data[6] = TYPE_COUNT;
        assert!(parse_personal(&data, 25).is_err());
    }

    #[test]
    fn canonical_forms_resolve_indirection_and_cosmetic_aliases_without_losing_807() {
        let record = personal();
        let mut table = (0..=MAX_SPECIES + 1)
            .map(|i| parse_personal(&record, i).unwrap())
            .collect::<Vec<_>>();
        table[26].form_count = 2;
        table[26].form_stats_index = MAX_SPECIES + 1;
        table[201].form_count = 28;
        let forms = species_forms(&table).unwrap();
        assert!(forms.contains(&SpeciesForm {
            key: SpeciesKey {
                species: 26,
                form: 1
            },
            personal_index: MAX_SPECIES + 1
        }));
        assert!(forms.contains(&SpeciesForm {
            key: SpeciesKey {
                species: 201,
                form: 27
            },
            personal_index: 201
        }));
        assert!(forms.contains(&SpeciesForm {
            key: SpeciesKey {
                species: 807,
                form: 0
            },
            personal_index: 807
        }));
        assert_eq!(forms.iter().filter(|f| f.key.form == 0).count(), 807);
        table[26].form_stats_index = 65000;
        assert!(species_forms(&table).is_err());
    }

    #[test]
    fn battle_only_and_held_item_forms_are_refused() {
        let key = |species, form| SpeciesKey { species, form };
        for (species, form) in [
            (351, 1),
            (351, 3),
            (421, 1),
            (487, 1),
            (493, 5),
            (555, 1),
            (648, 1),
            (649, 4),
            (681, 1),
            (746, 1),
            (773, 17),
            (774, 7),
            (774, 13),
            (778, 1),
        ] {
            assert!(!ordinary_form(key(species, form)), "{species} {form}");
            assert!(ordinary_form(key(species, 0)), "{species}");
        }
        // Cosmetic and regional forms stay.
        for (species, form) in [(201, 27), (774, 6), (666, 19), (26, 1), (745, 2), (720, 1)] {
            assert!(ordinary_form(key(species, form)), "{species} {form}");
        }
    }

    #[test]
    fn ordinary_moves_remain_ordinary_with_power_up_metadata_and_wide_effects() {
        let mut data = [0; MOVE_SIZE];
        data[0] = 9;
        data[2] = 2;
        data[3] = 90;
        data[4] = 100;
        data[5] = 15;
        data[6] = (-1i8) as u8;
        data[0x10..0x12].copy_from_slice(&301u16.to_le_bytes());
        data[0x20] = 175;
        data[0x24..0x28].copy_from_slice(&0x8000_0001u32.to_le_bytes());
        let got = parse_move(&data, 53).unwrap();
        assert_eq!(got.policy, MovePolicy::Ordinary);
        assert_eq!(got.power_up.power, 175);
        assert_eq!(got.effect_sequence, 301);
        assert_eq!(got.flags, 0x8000_0001);
        assert_eq!(got.priority, -1);
        data[4] = 101;
        assert_eq!(parse_move(&data, 53).unwrap().accuracy, 101);
        assert_eq!(
            parse_move(&data, 622).unwrap().policy,
            MovePolicy::ExcludedPowerUpMove
        );
        assert_eq!(
            (1..=MAX_MOVE)
                .filter(|&id| move_policy(id) == MovePolicy::ExcludedPowerUpMove)
                .count(),
            53
        );
        for id in [621, 659, 694, 704, 718, 720, 722] {
            assert_eq!(move_policy(id), MovePolicy::Ordinary);
        }
    }

    #[test]
    fn malformed_mini_tables_cannot_slice_outside_their_data() {
        let mut data = b"WD\x01\x00".to_vec();
        data.extend_from_slice(&12u32.to_le_bytes());
        data.extend_from_slice(&15u32.to_le_bytes());
        data.extend_from_slice(b"abc");
        assert_eq!(mini_entries(&data, b"WD").unwrap(), vec![&b"abc"[..]]);
        for len in 0..data.len() {
            assert!(mini_entries(&data[..len], b"WD").is_err());
        }
        data[4..8].copy_from_slice(&4u32.to_le_bytes());
        assert!(mini_entries(&data, b"WD").is_err());
    }

    #[test]
    fn learnsets_preserve_evolution_moves_and_reject_truncation_and_bad_ids() {
        let data = [33, 0, 0, 0, 45, 0, 5, 0, 255, 255, 255, 255];
        assert_eq!(
            parse_learnset(&data).unwrap(),
            vec![
                LearnedMove {
                    move_id: 33,
                    level: 0
                },
                LearnedMove {
                    move_id: 45,
                    level: 5
                }
            ]
        );
        for len in 0..data.len() {
            assert!(parse_learnset(&data[..len]).is_err());
        }
        let mut bad = data;
        bad[2] = 101;
        assert!(parse_learnset(&bad).is_err());
    }

    fn text_file(lines: &[&[u16]]) -> Vec<u8> {
        let mut data = vec![0u8; 20 + lines.len() * 8];
        data[..2].copy_from_slice(&1u16.to_le_bytes());
        data[2..4].copy_from_slice(&(lines.len() as u16).to_le_bytes());
        data[12..16].copy_from_slice(&16u32.to_le_bytes());
        for (line, words) in lines.iter().enumerate() {
            let start = data.len() as u32 - 16;
            data[20 + line * 8..24 + line * 8].copy_from_slice(&start.to_le_bytes());
            data[24 + line * 8..26 + line * 8].copy_from_slice(&(words.len() as u16).to_le_bytes());
            let mut key = 0x7C89u16.wrapping_add((line as u16).wrapping_mul(0x2983));
            for &word in words.iter() {
                data.extend_from_slice(&(word ^ key).to_le_bytes());
                key = key.rotate_left(3);
            }
        }
        let length = data.len() as u32 - 16;
        data[4..8].copy_from_slice(&length.to_le_bytes());
        data[16..20].copy_from_slice(&length.to_le_bytes());
        data
    }

    #[test]
    fn name_text_handles_unicode_and_preserves_variables_with_zero_arguments() {
        let bytes = text_file(&[
            &[65, 0xE08E, 0],
            &[0x10, 2, 0x1234, 0, 66, 0],
            &[0xD83D, 0xDE00, 0],
        ]);
        assert_eq!(
            parse_text(&bytes).unwrap(),
            ["A♂", "[VAR 1234 0000]B", "😀"]
        );
        for len in 0..bytes.len() {
            assert!(parse_text(&bytes[..len]).is_err());
        }
        let mut bad = bytes;
        bad[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_text(&bad).is_err());
        assert!(parse_text(&text_file(&[&[65]])).is_err());
        assert!(parse_text(&text_file(&[&[0xD800, 0]])).is_err());
    }

    #[test]
    fn trainer_records_preserve_forms_and_reorder_speed_without_inventing_moves() {
        let mut header = [0u8; 20];
        header[3] = 1;
        let mut party = [0u8; 32];
        party[0] = 0x21;
        party[1] = 3;
        party[2..8].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        let ivs = 1u32 << 5 | 2 << 10 | 3 << 15 | 4 << 20 | 5 << 25;
        party[8..12].copy_from_slice(&ivs.to_le_bytes());
        party[14] = 5;
        party[16..18].copy_from_slice(&26u16.to_le_bytes());
        party[18] = 1;
        let trainer = parse_trainer(&header, &party, 1).unwrap();
        let creature = &trainer.party[0];
        assert_eq!(
            creature.key,
            SpeciesKey {
                species: 26,
                form: 1
            }
        );
        assert_eq!(
            (creature.ability_selector, creature.gender_selector),
            (2, 1)
        );
        assert_eq!(creature.ivs, [0, 1, 2, 5, 3, 4]);
        assert_eq!(creature.evs, [1, 2, 3, 6, 4, 5]);
        assert_eq!(creature.moves, [0; 4]);
        assert!(parse_trainer(&header, &party[..31], 1).is_err());
        party[14] = 0;
        assert!(parse_trainer(&header, &party, 1).is_err());
    }

    #[test]
    fn encounters_keep_day_night_forms_and_require_bounded_zero_padding() {
        let mut day = [0u8; 0x164];
        day[0] = 2;
        day[1] = 3;
        day[2] = 100;
        day[12..16].copy_from_slice(&19u32.to_le_bytes());
        let mut night = day;
        night[12..16].copy_from_slice(&(19u32 | 1 << 11).to_le_bytes());
        let mut archive = vec![0u8; 128 + 768];
        archive[..4].copy_from_slice(b"EA\x01\x00");
        archive[4..8].copy_from_slice(&128u32.to_le_bytes());
        archive[8..12].copy_from_slice(&896u32.to_le_bytes());
        archive[132..488].copy_from_slice(&day);
        archive[488..844].copy_from_slice(&night);
        let tables = parse_encounter_archive(&archive).unwrap();
        assert_eq!(
            tables[0].day.slots[0][0],
            Some(SpeciesKey {
                species: 19,
                form: 0
            })
        );
        assert_eq!(
            tables[0].night.slots[0][0],
            Some(SpeciesKey {
                species: 19,
                form: 1
            })
        );
        archive[895] = 1;
        assert!(parse_encounter_archive(&archive).is_err());
        assert!(parse_encounter_table(&day[..355]).is_err());
        day[12..16].copy_from_slice(&(1u32 << 16).to_le_bytes());
        assert!(parse_encounter_table(&day).is_err());
    }
}

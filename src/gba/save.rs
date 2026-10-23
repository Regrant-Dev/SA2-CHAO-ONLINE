//! Gen 3 GBA save (128 KiB flash) parser: party and storage boxes.
//!
//! Layout: two slots at 0x0000 and 0xE000, each 14 sections of 0x1000 bytes stored in rotated
//! order. Section footer @0xFF4: u16 id, u16 checksum, u32 signature, u32 save counter.

use super::{rom, text};

const SECTION_SIZE: usize = 0x1000;
const SECTIONS: usize = 14;
const SLOT_SIZE: usize = SECTION_SIZE * SECTIONS;
/// Smallest accepted file: both slots.
const MIN_LEN: usize = SLOT_SIZE * 2;
const FOOTER_ID: usize = 0xFF4;
const FOOTER_CHECKSUM: usize = 0xFF6;
const FOOTER_SIGNATURE: usize = 0xFF8;
const FOOTER_COUNTER: usize = 0xFFC;
const SIGNATURE: u32 = 0x0801_2025;
/// Full section payload.
const SECTION_DATA: usize = 3968;
/// Block sizes: block 2 (id 0), block 1 (ids 1..=4), storage (ids 5..=13).
const BLOCK2_SIZE: usize = 0xF2C;
const BLOCK1_SIZE: usize = 0x3D88;
const STORAGE_SIZE: usize = 0x83D0;

/// Block 2: u32 trainer id (public id | secret id << 16).
const TRAINER_ID: usize = 0x0A;
/// Block 1: u8 party count, then 6 party records of 100 bytes.
const PARTY_COUNT: usize = 0x234;
const PARTY: usize = 0x238;
const PARTY_SIZE: usize = 6;
const PARTY_MON_SIZE: usize = 100;
/// Party record: u8 level @84.
const PARTY_LEVEL: usize = 84;
/// Storage: u8 current box @0, 14 × 30 box records of 80 bytes @4, 14 × 9-byte names @0x8344.
const BOXES: usize = 4;
const BOX_COUNT: usize = 14;
const BOX_SLOTS: usize = 30;
const BOX_MON_SIZE: usize = 80;
const BOX_NAMES: usize = 0x8344;
const BOX_NAME_LEN: usize = 9;

/// Box record: u32 personality @0, u32 original trainer id @4, 10-byte nickname @8, language @18,
/// flags @19 (bit0 bad egg, bit1 has species, bit2 egg), u16 checksum @28, then 4 encrypted
/// 12-byte substructures @32.
const NICKNAME: usize = 8;
const NICKNAME_LEN: usize = 10;
const FLAGS: usize = 19;
const CHECKSUM: usize = 28;
const SECURE: usize = 32;
const SECURE_LEN: usize = 48;
const SUB_LEN: usize = 12;

/// Payload bytes in a section with this id (blocks split into 3968-byte pieces).
fn section_data_size(id: usize) -> usize {
    let piece =
        |total: usize, index: usize| total.saturating_sub(index * SECTION_DATA).min(SECTION_DATA);
    match id {
        0 => BLOCK2_SIZE,
        1..=4 => piece(BLOCK1_SIZE, id - 1),
        5..=13 => piece(STORAGE_SIZE, id - 5),
        _ => 0,
    }
}

/// Sum of LE u32 words, folded to 16 bits.
fn section_checksum(data: &[u8]) -> u16 {
    let sum = data
        .as_chunks::<4>()
        .0
        .iter()
        .fold(0u32, |a, w| a.wrapping_add(u32::from_le_bytes(*w)));
    ((sum >> 16) as u16).wrapping_add(sum as u16)
}

/// Sum of LE u16 words: the creature record checksum over the decrypted substructures.
fn record_checksum(sec: &[u8]) -> u16 {
    sec.as_chunks::<2>()
        .0
        .iter()
        .fold(0u16, |a, w| a.wrapping_add(u16::from_le_bytes(*w)))
}

/// XORs each LE u32 word with `key` (substructure encryption is its own inverse).
fn xor_words(src: &[u8], key: u32) -> [u8; SECURE_LEN] {
    let mut out = [0u8; SECURE_LEN];
    for (o, w) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.as_chunks::<4>().0)
    {
        *o = (u32::from_le_bytes(*w) ^ key).to_le_bytes();
    }
    out
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Where a creature sits in the save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Party(u8),
    Box { index: u8, slot: u8 },
}

/// A decoded creature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mon {
    pub place: Place,
    pub personality: u32,
    pub ot_id: u32,
    pub nickname: String,
    pub species: u16,
    pub held_item: u16,
    pub exp: u32,
    pub moves: [u16; 4],
    /// HP, attack, defense, speed, sp. attack, sp. defense.
    pub ivs: [u8; 6],
    /// Same order as `ivs`.
    pub evs: [u8; 6],
    /// Level stored with party records; `None` in boxes.
    pub party_level: Option<u8>,
    pub is_egg: bool,
    /// Which of its species' two abilities it has (0 or 1).
    pub ability_slot: u8,
}

impl Mon {
    /// Nature id: personality % 25.
    pub fn nature(&self) -> u8 {
        (self.personality % 25) as u8
    }

    /// Stable identity: personality in the high 32 bits, original trainer id in the low 32.
    pub fn key(&self) -> u64 {
        u64::from(self.personality) << 32 | u64::from(self.ot_id)
    }
}

/// The newest valid save slot's creatures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Save {
    pub party: Vec<Mon>,
    pub boxes: Vec<Mon>,
    /// 14 storage box names.
    pub box_names: Vec<String>,
    /// Save counter of the slot read.
    pub counter: u32,
    /// Player's trainer id (public id | secret id << 16).
    pub trainer_id: u32,
    /// Occupied records left out: bad eggs, checksum failures, unknown species ids.
    pub skipped: u32,
}

/// Position of each substructure (growth, attacks, EVs, misc) for `personality % 24`. The 24 layouts
/// are the permutations of the four in lexicographic order; this inverts layout `n`.
fn substruct_positions(personality: u32) -> [usize; 4] {
    let mut n = (personality % 24) as usize;
    let mut left = vec![0usize, 1, 2, 3];
    let mut pos = [0; 4];
    for (slot, fact) in [6, 2, 1, 1].into_iter().enumerate() {
        let kind = left.remove(n / fact);
        n %= fact;
        pos[kind] = slot;
    }
    pos
}

enum Record {
    Empty,
    Bad,
    Mon(Box<Mon>),
}

/// Decodes one 80-byte box record (the first 80 bytes of a party record).
fn decode_record(raw: &[u8], place: Place) -> Record {
    let personality = u32_at(raw, 0);
    let ot_id = u32_at(raw, 4);
    let flags = raw[FLAGS];
    if (personality == 0 && ot_id == 0) || flags & 0b10 == 0 {
        return Record::Empty;
    }
    if flags & 0b1 != 0 {
        return Record::Bad;
    }
    let sec = xor_words(&raw[SECURE..SECURE + SECURE_LEN], personality ^ ot_id);
    if record_checksum(&sec) != u16_at(raw, CHECKSUM) {
        return Record::Bad;
    }
    let pos = substruct_positions(personality);
    let sub = |kind: usize| &sec[pos[kind] * SUB_LEN..(pos[kind] + 1) * SUB_LEN];
    let (growth, attacks, evs, misc) = (sub(0), sub(1), sub(2), sub(3));
    let species = u16_at(growth, 0);
    if species == 0 || species >= rom::SPECIES_COUNT {
        return Record::Bad;
    }
    let iv_word = u32_at(misc, 4);
    let mut ivs = [0; 6];
    for (i, iv) in ivs.iter_mut().enumerate() {
        *iv = (iv_word >> (i * 5) & 31) as u8;
    }
    let mut ev = [0; 6];
    ev.copy_from_slice(&evs[..6]);
    Record::Mon(Box::new(Mon {
        place,
        personality,
        ot_id,
        nickname: text::decode(&raw[NICKNAME..NICKNAME + NICKNAME_LEN]),
        species,
        held_item: u16_at(growth, 2),
        exp: u32_at(growth, 4),
        moves: std::array::from_fn(|i| u16_at(attacks, i * 2)),
        ivs,
        evs: ev,
        party_level: None,
        is_egg: iv_word >> 30 & 1 != 0 || flags & 0b100 != 0,
        ability_slot: (iv_word >> 31) as u8,
    }))
}

/// A complete slot: section payloads indexed by id, and the save counter.
struct Slot<'a> {
    sections: [&'a [u8]; SECTIONS],
    counter: u32,
}

/// Reads a slot; `None` unless all 14 ids appear once with the signature and good checksums.
fn read_slot(slot: &[u8]) -> Option<Slot<'_>> {
    let mut sections: [Option<&[u8]>; SECTIONS] = [None; SECTIONS];
    let mut counter = 0;
    for sec in slot.as_chunks::<SECTION_SIZE>().0 {
        let id = usize::from(u16_at(sec, FOOTER_ID));
        if id >= SECTIONS || u32_at(sec, FOOTER_SIGNATURE) != SIGNATURE || sections[id].is_some() {
            return None;
        }
        let data = &sec[..section_data_size(id)];
        if section_checksum(data) != u16_at(sec, FOOTER_CHECKSUM) {
            return None;
        }
        if id == 0 {
            counter = u32_at(sec, FOOTER_COUNTER);
        }
        sections[id] = Some(data);
    }
    let sections = sections.map(|s| s.unwrap_or_default());
    Some(Slot { sections, counter })
}

/// True when slot b is newer; a counter of 0 beats u32::MAX (wrap-around).
fn newer(a: u32, b: u32) -> bool {
    if (a == u32::MAX && b == 0) || (a == 0 && b == u32::MAX) {
        a.wrapping_add(1) < b.wrapping_add(1)
    } else {
        a < b
    }
}

/// Parses a save file (at least 0x1C000 bytes; trailing bytes are ignored).
pub fn parse(bytes: &[u8]) -> Result<Save, String> {
    if bytes.len() < MIN_LEN {
        return Err("not a GBA save file (too small)".into());
    }
    let a = read_slot(&bytes[..SLOT_SIZE]);
    let b = read_slot(&bytes[SLOT_SIZE..MIN_LEN]);
    let slot = match (a, b) {
        (Some(a), Some(b)) => {
            if newer(a.counter, b.counter) {
                b
            } else {
                a
            }
        }
        (Some(s), None) | (None, Some(s)) => s,
        (None, None) => return Err("the GBA save is empty or damaged".into()),
    };
    let join = |ids: std::ops::RangeInclusive<usize>| -> Vec<u8> {
        ids.flat_map(|id| slot.sections[id].iter().copied())
            .collect()
    };
    let block1 = join(1..=4);
    let storage = join(5..=13);
    let block2 = slot.sections[0];

    let mut skipped = 0;
    let mut party = Vec::new();
    let count = usize::from(block1[PARTY_COUNT]).min(PARTY_SIZE);
    for i in 0..count {
        let raw = &block1[PARTY + i * PARTY_MON_SIZE..PARTY + (i + 1) * PARTY_MON_SIZE];
        match decode_record(raw, Place::Party(i as u8)) {
            Record::Empty => {}
            Record::Bad => skipped += 1,
            Record::Mon(mut m) => {
                m.party_level = Some(raw[PARTY_LEVEL]);
                party.push(*m);
            }
        }
    }
    let mut boxes = Vec::new();
    for index in 0..BOX_COUNT {
        for slot_no in 0..BOX_SLOTS {
            let off = BOXES + (index * BOX_SLOTS + slot_no) * BOX_MON_SIZE;
            let place = Place::Box {
                index: index as u8,
                slot: slot_no as u8,
            };
            match decode_record(&storage[off..off + BOX_MON_SIZE], place) {
                Record::Empty => {}
                Record::Bad => skipped += 1,
                Record::Mon(m) => boxes.push(*m),
            }
        }
    }
    let box_names = (0..BOX_COUNT)
        .map(|i| {
            let off = BOX_NAMES + i * BOX_NAME_LEN;
            text::decode(&storage[off..off + BOX_NAME_LEN])
        })
        .collect();
    Ok(Save {
        party,
        boxes,
        box_names,
        counter: slot.counter,
        trainer_id: u32_at(block2, TRAINER_ID),
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes a creature as an 80-byte box record.
    fn encode(m: &Mon) -> [u8; BOX_MON_SIZE] {
        let mut raw = [0u8; BOX_MON_SIZE];
        raw[0..4].copy_from_slice(&m.personality.to_le_bytes());
        raw[4..8].copy_from_slice(&m.ot_id.to_le_bytes());
        raw[NICKNAME..NICKNAME + NICKNAME_LEN].fill(0xFF);
        raw[NICKNAME] = 0xBB; // "A"
        raw[FLAGS] = 0b10 | if m.is_egg { 0b100 } else { 0 };
        let mut subs = [[0u8; SUB_LEN]; 4];
        subs[0][0..2].copy_from_slice(&m.species.to_le_bytes());
        subs[0][2..4].copy_from_slice(&m.held_item.to_le_bytes());
        subs[0][4..8].copy_from_slice(&m.exp.to_le_bytes());
        for (i, mv) in m.moves.iter().enumerate() {
            subs[1][i * 2..i * 2 + 2].copy_from_slice(&mv.to_le_bytes());
        }
        subs[2][..6].copy_from_slice(&m.evs);
        let mut iv = m
            .ivs
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, &v)| a | u32::from(v) << (i * 5));
        if m.is_egg {
            iv |= 1 << 30;
        }
        iv |= u32::from(m.ability_slot) << 31;
        subs[3][4..8].copy_from_slice(&iv.to_le_bytes());
        let pos = substruct_positions(m.personality);
        let mut sec = [0u8; SECURE_LEN];
        for kind in 0..4 {
            sec[pos[kind] * SUB_LEN..(pos[kind] + 1) * SUB_LEN].copy_from_slice(&subs[kind]);
        }
        raw[CHECKSUM..CHECKSUM + 2].copy_from_slice(&record_checksum(&sec).to_le_bytes());
        raw[SECURE..SECURE + SECURE_LEN].copy_from_slice(&xor_words(&sec, m.personality ^ m.ot_id));
        raw
    }

    fn mon(personality: u32, ot_id: u32, species: u16) -> Mon {
        Mon {
            place: Place::Party(0),
            personality,
            ot_id,
            nickname: "A".into(),
            species,
            held_item: 13,
            exp: 4321,
            moves: [1, 2, 3, 4],
            ivs: [31, 0, 15, 7, 1, 30],
            evs: [252, 0, 6, 1, 2, 3],
            party_level: None,
            is_egg: false,
            ability_slot: 0,
        }
    }

    /// Unencrypted save contents.
    struct Blocks {
        block2: Vec<u8>,
        block1: Vec<u8>,
        storage: Vec<u8>,
    }

    impl Blocks {
        fn new() -> Self {
            let mut b = Blocks {
                block2: vec![0; BLOCK2_SIZE],
                block1: vec![0; BLOCK1_SIZE],
                storage: vec![0; STORAGE_SIZE],
            };
            b.block2[TRAINER_ID..TRAINER_ID + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
            for i in 0..BOX_COUNT {
                let off = BOX_NAMES + i * BOX_NAME_LEN;
                b.storage[off] = 0xBC + i as u8;
                b.storage[off + 1] = 0xFF;
            }
            b
        }

        fn set_party(&mut self, mons: &[(Mon, u8)]) {
            self.block1[PARTY_COUNT] = mons.len() as u8;
            for (i, (m, level)) in mons.iter().enumerate() {
                let off = PARTY + i * PARTY_MON_SIZE;
                self.block1[off..off + BOX_MON_SIZE].copy_from_slice(&encode(m));
                self.block1[off + PARTY_LEVEL] = *level;
            }
        }

        fn set_box(&mut self, index: usize, slot: usize, raw: &[u8; BOX_MON_SIZE]) {
            let off = BOXES + (index * BOX_SLOTS + slot) * BOX_MON_SIZE;
            self.storage[off..off + BOX_MON_SIZE].copy_from_slice(raw);
        }

        /// Writes a slot whose sections start rotated by `rotation`.
        fn write_slot(&self, out: &mut [u8], counter: u32, rotation: usize) {
            for id in 0..SECTIONS {
                let (block, start) = match id {
                    0 => (&self.block2, 0),
                    1..=4 => (&self.block1, (id - 1) * SECTION_DATA),
                    _ => (&self.storage, (id - 5) * SECTION_DATA),
                };
                let size = section_data_size(id);
                let sec_off = ((id + rotation) % SECTIONS) * SECTION_SIZE;
                let sec = &mut out[sec_off..sec_off + SECTION_SIZE];
                sec.fill(0);
                sec[..size].copy_from_slice(&block[start..start + size]);
                let sum = section_checksum(&sec[..size]);
                sec[FOOTER_ID..FOOTER_ID + 2].copy_from_slice(&(id as u16).to_le_bytes());
                sec[FOOTER_CHECKSUM..FOOTER_CHECKSUM + 2].copy_from_slice(&sum.to_le_bytes());
                sec[FOOTER_SIGNATURE..FOOTER_SIGNATURE + 4]
                    .copy_from_slice(&SIGNATURE.to_le_bytes());
                sec[FOOTER_COUNTER..FOOTER_COUNTER + 4].copy_from_slice(&counter.to_le_bytes());
            }
        }
    }

    /// Full 128 KiB file with both slots.
    fn file(a: &Blocks, a_counter: u32, b: &Blocks, b_counter: u32) -> Vec<u8> {
        let mut out = vec![0xFFu8; 0x20000];
        a.write_slot(&mut out[..SLOT_SIZE], a_counter, 3);
        b.write_slot(&mut out[SLOT_SIZE..MIN_LEN], b_counter, 9);
        out
    }

    #[test]
    fn section_sizes_cover_blocks() {
        assert_eq!(section_data_size(0), BLOCK2_SIZE);
        assert_eq!((1..=4).map(section_data_size).sum::<usize>(), BLOCK1_SIZE);
        assert_eq!((5..=13).map(section_data_size).sum::<usize>(), STORAGE_SIZE);
        assert_eq!(section_data_size(13), 0x7D0);
    }

    #[test]
    fn substruct_layouts_are_distinct_permutations() {
        let mut seen = std::collections::HashSet::new();
        for n in 0..24 {
            let p = substruct_positions(n);
            let mut s = p;
            s.sort();
            assert_eq!(s, [0, 1, 2, 3]);
            assert!(seen.insert(p));
        }
        assert_eq!(substruct_positions(0), [0, 1, 2, 3]);
        assert_eq!(substruct_positions(2), [0, 2, 1, 3]);
        assert_eq!(substruct_positions(23), [3, 2, 1, 0]);
        assert_eq!(substruct_positions(9), [3, 0, 1, 2]);
    }

    #[test]
    fn all_orders_round_trip() {
        for n in 0..24u32 {
            let personality = 0xFEDC_BA98u32 / 24 * 24 - 24 + n;
            assert_eq!(personality % 24, n);
            let mut m = mon(personality, 0xBEEF_0042, 100 + n as u16);
            // The ability bit sits above the egg bit.
            m.ability_slot = (n % 2) as u8;
            let place = Place::Box { index: 2, slot: 5 };
            let Record::Mon(got) = decode_record(&encode(&m), place) else {
                panic!("order {n}")
            };
            assert_eq!(*got, Mon { place, ..m });
        }
    }

    #[test]
    fn newest_slot_and_layout() {
        let mut old = Blocks::new();
        old.set_party(&[(mon(5, 6, 1), 9)]);
        let mut new = Blocks::new();
        let mut egg = mon(77, 88, 2);
        egg.is_egg = true;
        new.set_party(&[(mon(11, 12, 3), 42), (egg, 5)]);
        new.set_box(0, 0, &encode(&mon(21, 22, 4)));
        new.set_box(13, 29, &encode(&mon(23, 24, 5)));
        let mut bad = encode(&mon(31, 32, 6));
        bad[CHECKSUM] ^= 1;
        new.set_box(4, 7, &bad);
        let mut bad_egg = encode(&mon(33, 34, 7));
        bad_egg[FLAGS] |= 1;
        new.set_box(4, 8, &bad_egg);

        let mut bytes = file(&old, 7, &new, 8);
        bytes.extend_from_slice(&[0u8; 16]);
        assert_eq!(bytes.len(), 0x20010);
        let s = parse(&bytes).unwrap();
        assert_eq!(s.counter, 8);
        assert_eq!(s.trainer_id, 0x1234_5678);
        assert_eq!(s.party.len(), 2);
        assert_eq!(s.party[0].place, Place::Party(0));
        assert_eq!(s.party[0].party_level, Some(42));
        assert_eq!(s.party[0].species, 3);
        assert!(!s.party[0].is_egg);
        assert!(s.party[1].is_egg);
        assert_eq!(s.party[1].place, Place::Party(1));
        assert_eq!(s.boxes.len(), 2);
        assert_eq!(s.boxes[0].place, Place::Box { index: 0, slot: 0 });
        assert_eq!(s.boxes[0].party_level, None);
        assert_eq!(
            s.boxes[1].place,
            Place::Box {
                index: 13,
                slot: 29
            }
        );
        assert_eq!(s.boxes[1].species, 5);
        assert_eq!(s.skipped, 2);
        assert_eq!(s.box_names.len(), BOX_COUNT);
        assert_eq!(s.box_names[1], "C");
        assert_eq!(s.party[0].key(), 11u64 << 32 | 12);
        assert_eq!(s.party[0].nature(), 11);

        // Same with the slots swapped.
        let s = parse(&file(&new, 8, &old, 7)).unwrap();
        assert_eq!(s.counter, 8);
        // Counter wrap-around: 0 is newer than u32::MAX.
        let s = parse(&file(&old, u32::MAX, &new, 0)).unwrap();
        assert_eq!(s.counter, 0);
    }

    #[test]
    fn broken_newest_slot_falls_back() {
        let mut old = Blocks::new();
        old.set_party(&[(mon(5, 6, 1), 9)]);
        let new = Blocks::new();
        let mut bytes = file(&old, 7, &new, 8);
        // Corrupt one section's data in the newer slot.
        bytes[SLOT_SIZE + 5 * SECTION_SIZE + 10] ^= 0x40;
        let s = parse(&bytes).unwrap();
        assert_eq!(s.counter, 7);
        assert_eq!(s.party.len(), 1);
        assert_eq!(s.party[0].party_level, Some(9));
        // Duplicate a section id in the older slot too: nothing valid left.
        let first = bytes[..SECTION_SIZE].to_vec();
        bytes[SECTION_SIZE..2 * SECTION_SIZE].copy_from_slice(&first);
        assert!(parse(&bytes).is_err());
    }

    #[test]
    fn empty_slots_and_short_files() {
        let mut b = Blocks::new();
        // Occupied-looking slot without the has-species flag, and a zeroed slot.
        let mut raw = encode(&mon(1, 2, 3));
        raw[FLAGS] = 0;
        b.set_box(1, 1, &raw);
        b.set_party(&[]);
        let s = parse(&file(&b, 1, &b, 2)).unwrap();
        assert!(s.party.is_empty() && s.boxes.is_empty());
        assert_eq!(s.skipped, 0);
        assert!(parse(&[]).is_err());
        assert!(parse(&vec![0; MIN_LEN - 1]).is_err());
        assert!(parse(&vec![0; MIN_LEN]).is_err());
        assert!(parse(&vec![0xFF; 0x20000]).is_err());
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..20 {
            let len = MIN_LEN + (next() % 0x5000) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = parse(&bytes);
        }
        // Valid footers around random payloads, including random party counts and records.
        for _ in 0..50 {
            let mut b = Blocks::new();
            for v in b.block1.iter_mut().chain(b.storage.iter_mut()) {
                *v = next() as u8;
            }
            let _ = parse(&file(&b, 1, &b, 2)).unwrap();
        }
    }
}

//! GBA link: creatures from the player's own GBA save (game code BPEE) come to a Chao garden as
//! partners Chao can pick up and absorb, like the game's animals. Everything shown of them
//! (names, types, pictures) is read from the player's own ROM (`crate::gba`); the GBA save is
//! only read.
//!
//! - F9 "GBA link" lists the save's party and boxes and the ones the mod keeps (without a save,
//!   a one-time gift of one of the ROM's first partners); Enter brings one into the garden we're
//!   in (`store`: each once a day per garden, 4 waiting per garden at most).
//! - In the garden each waiting partner is an animal object drawn as its picture (`scene`).
//! - A Chao that absorbs one keeps it as its partner, which teaches it the partner's moves (it
//!   keeps four), type ids and ability, in `<mod>/gba/partners.txt` by the Chao's identity; its
//!   stats and save don't change. `battle_profile` gives what a battle uses (`profile`). The
//!   status balloon shows the partner's types (balloon.rs).
//! - Other players' Chao: what theirs have from partners comes online (`Body::Partners`), and
//!   exporting Chao files writes `partners.txt` next to them, which is read from `chao/import`.

// Off the game (Linux builds) only the tests use the garden side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

mod billboard;
pub mod profile;
mod scene;
pub mod store;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::gba::rom::Rom;
use crate::gba::save::{self, Mon, Place, Save};

use crate::log;
use crate::net::protocol::SharedPartner;
pub use profile::Profile;
use store::{Learned, Partner, Store};

/// The folder in the mod where the ROM and save are looked for when no path is set, and where
/// `partners.txt` lives.
const FOLDER: &str = "gba";
const STORE_FILE: &str = "partners.txt";

/// Where the ROM and save are (config.ini `GbaRom`, `GbaSave`; empty = the first `.gba` /
/// `.sav` in `<mod>/gba/`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paths {
    pub rom: String,
    pub save: String,
}

struct Link {
    paths: Paths,
    /// The ROM and the path it came from.
    rom: Option<(PathBuf, Arc<Rom>)>,
    store: Option<Store>,
}

static LINK: Mutex<Link> = Mutex::new(Link {
    paths: Paths {
        rom: String::new(),
        save: String::new(),
    },
    rom: None,
    store: None,
});

/// What other players' Chao have from their partners (`Body::Partners`), by Chao identity.
static OTHERS: Mutex<BTreeMap<u64, Learned>> = Mutex::new(BTreeMap::new());

fn with<R>(f: impl FnOnce(&mut Link) -> R) -> R {
    f(&mut LINK.lock().unwrap_or_else(|e| e.into_inner()))
}

fn folder() -> PathBuf {
    Path::new(crate::online::mod_path()).join(FOLDER)
}

/// The days since 1970 (UTC): imports are counted per day.
fn today() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as u32)
}

/// `set` as configured, else the first file with `extension` in `<mod>/gba/`. A relative path
/// is from the mod folder.
fn resolve(set: &str, extension: &str) -> Option<PathBuf> {
    let set = set.trim().trim_matches('"');
    if !set.is_empty() {
        let p = PathBuf::from(set);
        return Some(if p.is_absolute() {
            p
        } else {
            Path::new(crate::online::mod_path()).join(p)
        });
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Settings from config.ini (menu.rs reads the file).
pub fn set_paths(paths: Paths) {
    with(|l| l.paths = paths);
}

/// The ROM, loaded the first time it's needed (16 MiB, read once).
pub fn rom() -> Result<Arc<Rom>, String> {
    let paths = with(|l| l.paths.clone());
    let path = resolve(&paths.rom, "gba").ok_or_else(|| {
        "No GBA ROM: put your own .gba in the mod's gba folder or set GbaRom".to_string()
    })?;
    if let Some(rom) = with(|l| {
        l.rom
            .as_ref()
            .filter(|(p, _)| *p == path)
            .map(|(_, r)| r.clone())
    }) {
        return Ok(rom);
    }
    let rom = Arc::new(Rom::open(&path)?);
    log!("gba link: ROM {}", path.display());
    with(|l| l.rom = Some((path, rom.clone())));
    Ok(rom)
}

/// The ROM if it's loaded already (never reads the file).
fn loaded_rom() -> Option<Arc<Rom>> {
    with(|l| l.rom.as_ref().map(|(_, r)| r.clone()))
}

/// The GBA save, read fresh (it may have changed since).
pub fn read_save() -> Result<Save, String> {
    let paths = with(|l| l.paths.clone());
    let path = resolve(&paths.save, "sav").ok_or_else(|| {
        "No GBA save: put your .sav in the mod's gba folder or set GbaSave".to_string()
    })?;
    let bytes = std::fs::read(&path).map_err(|e| format!("Can't read {}: {e}", path.display()))?;
    save::parse(&bytes)
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    with(|l| {
        let store = l.store.get_or_insert_with(|| {
            let mut s = std::fs::read_to_string(folder().join(STORE_FILE))
                .map(|t| Store::parse(&t))
                .unwrap_or_default();
            s.forget_old(today());
            s
        });
        f(store)
    })
}

fn write_store() {
    let text = with_store(|s| s.to_text());
    let dir = folder();
    let result =
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(STORE_FILE), text));
    if let Err(e) = result {
        log!("gba link: can't write {STORE_FILE}: {e}");
    }
}

/// Where a creature on the F9 list comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Party(u8),
    Box {
        index: u8,
        slot: u8,
    },
    /// The mod's own list (`store`), not a GBA save.
    Kept,
}

/// One creature that can come to the garden, whatever its source: its key (one creature, for
/// the once-a-day rule), species id, level, move ids and which of its species' abilities it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub source: Source,
    pub key: u64,
    pub species: u16,
    pub level: u8,
    pub moves: [u16; 4],
    pub ability_slot: u8,
}

impl Candidate {
    /// From a GBA save entry (`None` for an egg).
    pub fn from_mon(rom: &Rom, mon: &Mon) -> Option<Self> {
        if mon.is_egg {
            return None;
        }
        Some(Self {
            source: match mon.place {
                Place::Party(i) => Source::Party(i),
                Place::Box { index, slot } => Source::Box { index, slot },
            },
            key: mon.key(),
            species: mon.species,
            level: level(rom, mon),
            moves: mon.moves,
            ability_slot: mon.ability_slot,
        })
    }
}

/// A creature's line in the F9 list: where it is, its name (the ROM's species name), level
/// and type names.
pub fn label(rom: &Rom, c: &Candidate) -> String {
    let place = match c.source {
        Source::Party(i) => format!("Party {}", i + 1),
        Source::Box { index, slot } => format!("Box {} #{}", index + 1, slot + 1),
        Source::Kept => "Kept".to_string(),
    };
    let name = rom
        .species_name(c.species)
        .map_or_else(|| format!("#{}", c.species), |n| ascii(&n));
    let types = rom.species(c.species).map_or_else(String::new, |s| {
        type_names(rom, s.types)
            .iter()
            .map(|t| ascii(t))
            .collect::<Vec<_>>()
            .join("/")
    });
    format!("{place}: {name}  Lv{}  {types}", c.level)
}

/// Its level: the party's stored one, else from its experience and growth rate.
pub fn level(rom: &Rom, mon: &Mon) -> u8 {
    mon.party_level
        .filter(|l| (1..=100).contains(l))
        .unwrap_or_else(|| {
            rom.species(mon.species)
                .map_or(1, |s| rom.level_for_exp(s.growth, mon.exp))
        })
}

/// The type names of `types` (one when both are the same).
fn type_names(rom: &Rom, types: [u8; 2]) -> Vec<String> {
    let mut ids = vec![types[0]];
    if types[1] != types[0] {
        ids.push(types[1]);
    }
    ids.iter().filter_map(|&t| rom.type_name(t)).collect()
}

/// Only ASCII for the debug and game fonts: accented letters lose their accent, the rest is
/// left out.
pub fn ascii(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            ' '..='~' => Some(c),
            'à'..='å' => Some('a'),
            'À'..='Å' => Some('A'),
            'è'..='ë' => Some('e'),
            'È'..='Ë' => Some('E'),
            'ì'..='ï' => Some('i'),
            'ò'..='ö' => Some('o'),
            'ù'..='ü' => Some('u'),
            'ç' => Some('c'),
            'ñ' => Some('n'),
            '♂' => Some('m'),
            '♀' => Some('f'),
            _ => None,
        })
        .collect()
}

/// Brings `c` into the garden we're in. The message to show.
pub fn import(c: &Candidate) -> String {
    match import_checked(c) {
        Ok(name) => format!("{name} came to the garden"),
        Err(e) => e,
    }
}

fn import_checked(mon: &Candidate) -> Result<String, String> {
    if !crate::game::in_garden() {
        return Err("Go to a Chao garden first".into());
    }
    if crate::garden::server_runs() {
        return Err("GBA link works in your own garden, not a shared online one".into());
    }
    let rom = rom()?;
    let species = rom
        .species(mon.species)
        .ok_or("The ROM has no data for this one")?;
    let garden = crate::game::current_chao_area() as u8;
    let partner = Partner {
        species: mon.species,
        types: species.types,
        moves: mon.moves,
        level: mon.level.clamp(1, 100),
        ability: ability_of(&species, mon.ability_slot),
    };
    with_store(|s| s.import(mon.key, garden, today(), partner))
        .map_err(|r| r.message().to_string())?;
    write_store();
    let name = rom.species_name(mon.species).unwrap_or_default();
    log!(
        "gba link: species {} (level {}) came to garden {garden}",
        mon.species,
        partner.level
    );
    Ok(ascii(&name))
}

/// The ability in `slot` of a species' two, or its first when it has no second.
fn ability_of(species: &crate::gba::rom::Species, slot: u8) -> u8 {
    match species.abilities[usize::from(slot.min(1))] {
        0 => species.abilities[0],
        a => a,
    }
}

/// The partner types of the Chao with `identity`, by the ROM's type names, for the status
/// balloon (`None` when it has none or the ROM isn't loaded).
pub fn partner_types(identity: u64) -> Option<Vec<String>> {
    let learned = with_store(|s| s.partners.get(&identity).copied())?;
    let rom = loaded_rom()?;
    let names: Vec<String> = type_names(&rom, learned.partner.types)
        .iter()
        .map(|t| ascii(t))
        .collect();
    (!names.is_empty()).then_some(names)
}

/// The battle profile of the Chao with `identity`: its partner's types and ability and the
/// moves it was taught, or Normal type with basic moves without a partner. Ours come from
/// `partners.txt`, other players' from what they sent, then from `chao/import/partners.txt`.
/// Loads the ROM if needed.
pub fn battle_profile(identity: u64) -> Result<Profile, String> {
    let rom = rom()?;
    let learned = with_store(|s| s.partners.get(&identity).copied())
        .or_else(|| {
            OTHERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&identity)
                .copied()
        })
        .or_else(|| {
            let path = Path::new(crate::online::mod_path()).join(IMPORTED_PARTNERS);
            let text = std::fs::read_to_string(path).ok()?;
            Store::parse(&text).partners.get(&identity).copied()
        });
    Ok(profile::profile(&rom, learned.as_ref()))
}

/// What friends' Chao files carry from partners, under the mod folder.
const IMPORTED_PARTNERS: &str = "chao/import/partners.txt";

/// What our Chao have from their partners, to send to the others.
pub fn shared_partners() -> Vec<SharedPartner> {
    with_store(|s| {
        s.partners
            .iter()
            .map(|(&identity, l)| SharedPartner {
                identity,
                species: l.partner.species,
                types: l.partner.types,
                ability: l.partner.ability,
                moves: l.moves,
            })
            .collect()
    })
}

/// What another player's Chao have from their partners.
pub fn others_learned(list: &[SharedPartner]) {
    let mut others = OTHERS.lock().unwrap_or_else(|e| e.into_inner());
    for p in list {
        others.insert(
            p.identity,
            Learned {
                partner: Partner {
                    species: p.species,
                    types: p.types,
                    moves: p.moves,
                    level: 0,
                    ability: p.ability,
                },
                moves: p.moves,
            },
        );
    }
}

/// Writes `partners.txt` into `dir` with what the Chao with these identities have from their
/// partners, next to their exported Chao files (none: no file).
pub fn export_partners(dir: &Path, identities: &[u64]) {
    let partners: BTreeMap<u64, Learned> = with_store(|s| {
        identities
            .iter()
            .filter_map(|id| s.partners.get(id).map(|l| (*id, *l)))
            .collect()
    });
    let path = dir.join(STORE_FILE);
    let result = if partners.is_empty() {
        std::fs::remove_file(&path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })
    } else {
        let text = Store {
            partners,
            ..Store::default()
        }
        .to_text();
        std::fs::write(&path, text)
    };
    if let Err(e) = result {
        log!("gba link: can't write {}: {e}", path.display());
    }
}

/// A Chao (`identity`) absorbed the partner `key` waiting in `garden`: it keeps it as its
/// partner, which teaches it its moves, types and ability (the Chao record isn't changed).
fn absorbed(garden: u8, key: u64, identity: u64) {
    let Some(learned) = with_store(|s| s.absorbed(garden, key, identity)) else {
        return;
    };
    write_store();
    crate::online::resend_partners();
    log!(
        "gba link: a Chao absorbed species {} (types {:?}, ability {}, moves {:?})",
        learned.partner.species,
        learned.partner.types,
        learned.partner.ability,
        learned.moves
    );
}

/// Each frame: the garden's waiting partners (scene.rs).
pub fn on_frame() {
    scene::on_frame();
}

pub fn install() {
    scene::install();
    // The ROM in the background, so the status balloon has the type names from the start.
    std::thread::spawn(|| {
        if let Err(e) = rom() {
            log!("gba link: {e}");
        }
    });
}

/// A garden object that is one of our partners (items.rs leaves them out: they're never shared
/// or saved).
pub fn is_partner(object: usize) -> bool {
    scene::is_partner_object(object)
}

/// What Enter does on a row of the F9 page.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Nothing (a message).
    None,
    /// Bring this one to the garden.
    Bring(Candidate),
    /// Take this first-partner species as the gift.
    Gift(u16),
}

/// The F9 page: the save's creatures and the mod's kept ones, one row each; without a save,
/// the first-partner gift until it's taken.
pub struct Page {
    pub rows: Vec<(String, Action)>,
    pub row: usize,
    pub scroll: usize,
}

/// The gift's level.
const GIFT_LEVEL: u8 = 5;

impl Page {
    /// Reads the ROM and save now.
    pub fn open() -> Self {
        let mut rows = Vec::new();
        match rom() {
            Err(e) => {
                log!("gba link: {e}");
                rows.push((e, Action::None));
            }
            Ok(rom) => {
                let save = read_save();
                if let Ok(save) = &save {
                    for c in save
                        .party
                        .iter()
                        .chain(&save.boxes)
                        .filter_map(|m| Candidate::from_mon(&rom, m))
                    {
                        rows.push((label(&rom, &c), Action::Bring(c)));
                    }
                    if save.skipped > 0 {
                        rows.push((
                            format!("({} damaged entries skipped)", save.skipped),
                            Action::None,
                        ));
                    }
                }
                let (kept, gift) = with_store(|s| (s.kept.clone(), s.gift));
                for (key, species, level, moves) in kept {
                    let c = Candidate {
                        source: Source::Kept,
                        key,
                        species,
                        level,
                        moves,
                        ability_slot: 0,
                    };
                    rows.push((label(&rom, &c), Action::Bring(c)));
                }
                if let Err(e) = &save {
                    log!("gba link: {e}");
                    rows.push((e.clone(), Action::None));
                    if gift.is_none()
                        && let Some(starters) = rom.starters()
                    {
                        rows.push(("Or take one gift partner (only once):".into(), Action::None));
                        for species in starters {
                            let c = Candidate {
                                source: Source::Kept,
                                key: 0,
                                species,
                                level: GIFT_LEVEL,
                                moves: [0; 4],
                                ability_slot: 0,
                            };
                            let text = label(&rom, &c).replacen("Kept", "Gift", 1);
                            rows.push((text, Action::Gift(species)));
                        }
                    }
                }
                if rows.is_empty() {
                    rows.push(("The save has no one to bring".into(), Action::None));
                }
            }
        }
        Self {
            rows,
            row: 0,
            scroll: 0,
        }
    }
}

/// Takes the gift `species` (one of the ROM's first partners, once only). The message to show.
pub fn take_gift(species: u16) -> String {
    let Ok(rom) = rom() else {
        return "No GBA ROM".into();
    };
    if !rom.starters().is_some_and(|s| s.contains(&species)) {
        return "Not a gift partner".into();
    }
    let moves = rom.moves_at_level(species, GIFT_LEVEL);
    let key = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ (u64::from(species) << 48)
        | 1 << 63;
    if !with_store(|s| s.take_gift(key, species, GIFT_LEVEL, moves)) {
        return "The gift was taken already".into();
    }
    write_store();
    let name = ascii(&rom.species_name(species).unwrap_or_default());
    log!("gba link: took gift species {species}");
    format!("{name} is yours: F9 > GBA link brings it to a garden")
}

#[cfg(test)]
mod tests {
    use super::{ability_of, ascii};
    use crate::gba::rom::Species;

    #[test]
    fn ascii_keeps_letters_and_drops_the_rest() {
        assert_eq!(ascii("Abc-1 é♀"), "Abc-1 ef");
        assert_eq!(ascii("日本"), "");
    }

    #[test]
    fn the_ability_follows_the_slot() {
        let mut s = Species {
            types: [0, 0],
            base: [0; 6],
            growth: 0,
            abilities: [5, 9],
        };
        assert_eq!((ability_of(&s, 0), ability_of(&s, 1)), (5, 9));
        s.abilities[1] = 0;
        assert_eq!(ability_of(&s, 1), 5);
    }
}

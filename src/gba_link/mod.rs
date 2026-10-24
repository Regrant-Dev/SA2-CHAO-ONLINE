//! GBA link: creatures from the player's own GBA save (game code BPEE) come to a Chao garden as
//! partners Chao can pick up and absorb, like the game's animals. Everything shown of them
//! (names, types, pictures) is read from the player's own ROM (`crate::gba`); the GBA save is
//! only read.
//!
//! - The link machine in each garden (`machine`; F9 "GBA link" when it's switched off) lists the
//!   save's party and boxes and the ones the mod keeps (without a save, a one-time gift of one
//!   of the ROM's first partners); the one picked comes into the garden we're in (`store`: each
//!   once a day per garden, 4 waiting per garden at most).
//! - In the garden each waiting partner is an animal object drawn as its picture (`scene`).
//! - A Chao that absorbs one keeps it as its partner, which gives it the partner's type ids and
//!   ability and teaches it the partner's moves, in `<mod>/gba/partners.txt` by the Chao's
//!   identity; its stats and save don't change. A Chao remembers every move, type and ability
//!   it got and battles with four of the moves, one or two of the types and one of the
//!   abilities (or none), chosen at the link machine or the battle team screen. Its battle level
//!   and XP are kept there too. `battle_profile` gives what a battle uses (`profile`). The status
//!   balloon shows the types it battles as and the battle level (balloon.rs).
//! - Other players' Chao: the types, ability and four moves theirs battle with and their battle
//!   levels come online (`Body::Partners`), and exporting Chao files writes `partners.txt` next
//!   to them, which is read from `chao/import`.
//! - Wild visitors: each garden has an area of the ROM's wild tables every 10 minutes
//!   (`rotation`), whose creatures come by now and then and are absorbed like partners (`wild`).
//!   The link machine lists who can come to each garden now and next (`Page::who_comes`).
//! - In a garden the server runs, its visitors and the partners players bring are the server's,
//!   shown in every game there from its own ROM (`shared`); the link machine is there too, each
//!   player using it for themselves.

// Off the game (Linux builds) only the tests use the garden side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

pub(crate) mod billboard;
mod card;
pub(crate) mod machine;
mod machine_model;
pub mod profile;
pub mod rotation;
mod scene;
mod shared;
pub mod store;
mod wild;
pub mod wild_battle;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::gba::rom::Rom;
use crate::gba::save::{self, Mon, Place, Save};

use crate::battle::levels::Level;
use crate::log;
use crate::net::protocol::SharedPartner;
pub use profile::Profile;
use store::{Learned, Moves, Partner, Store, Traits};

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
    /// `partners.txt` was there but couldn't be read: it's left as it is (never written over).
    store_unreadable: bool,
}

static LINK: Mutex<Link> = Mutex::new(Link {
    paths: Paths {
        rom: String::new(),
        save: String::new(),
    },
    rom: None,
    store: None,
    store_unreadable: false,
});

/// What other players' Chao have from their partners (`Body::Partners`), with the types and
/// ability they battle with, by Chao identity.
static OTHERS: Mutex<BTreeMap<u64, (Learned, Traits)>> = Mutex::new(BTreeMap::new());
/// Other players' Chao's battle levels (`Body::Partners`), by Chao identity.
static OTHER_LEVELS: Mutex<BTreeMap<u64, Level>> = Mutex::new(BTreeMap::new());

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
        "No GBA ROM: put your own Emerald .gba (US) in the mod's gba folder or set GbaRom"
            .to_string()
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

/// The text of `path` (`None` when there's no file yet). A file that can't be read, or isn't
/// text, is an error.
fn read_text(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The store from `partners.txt`, loaded the first time. When the file is there but can't be
/// read, the store starts empty and the file is never written over (`write_store`).
fn loaded_store(l: &mut Link) -> &mut Store {
    if l.store.is_none() {
        let text = match read_text(&folder().join(STORE_FILE)) {
            Ok(text) => text.unwrap_or_default(),
            Err(e) => {
                log!("gba link: can't read {STORE_FILE} ({e}); it's left as it is");
                l.store_unreadable = true;
                String::new()
            }
        };
        // TM moves an older trainer progress file taught join the Chao's move lists.
        let taught = crate::battle::ladder::load().taught;
        let mut s = Store::parse_with_taught(&text, &taught);
        s.forget_old(today());
        l.store = Some(s);
    }
    l.store.get_or_insert_with(Store::default)
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    with(|l| f(loaded_store(l)))
}

/// Writes the store to `partners.txt` (through a temporary file), unless the file couldn't be
/// read.
fn write_store() {
    let text = with(|l| {
        let text = loaded_store(l).to_text();
        (!l.store_unreadable).then_some(text)
    });
    let Some(text) = text else {
        log!("gba link: {STORE_FILE} couldn't be read, so it isn't written");
        return;
    };
    if let Err(e) = crate::battle::ladder::write(&folder().join(STORE_FILE), &text) {
        log!("gba link: can't write {STORE_FILE}: {e}");
    }
}

/// Where a creature on the GBA link list comes from.
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

/// A creature's line in the GBA link list: where it is, its name (the ROM's species name), level
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
        from_save: matches!(mon.source, Source::Party(_) | Source::Box { .. }),
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

/// The Chao with `identity` has absorbed a partner (known without the ROM).
pub fn has_partner(identity: u64) -> bool {
    with_store(|s| {
        s.partners
            .get(&identity)
            .is_some_and(|l| l.partner.species != 0)
    })
}

/// The types the Chao with `identity` battles as, by the ROM's type names, for the status
/// balloon (`None` when it has no partner or the ROM isn't loaded).
pub fn partner_types(identity: u64) -> Option<Vec<String>> {
    if !has_partner(identity) {
        return None;
    }
    let rom = loaded_rom()?;
    let names: Vec<String> = traits_of(identity)
        .active_type_list()
        .into_iter()
        .filter_map(|t| rom.type_name(t))
        .map(|t| ascii(&t))
        .collect();
    (!names.is_empty()).then_some(names)
}

/// What is known of a Chao: its partner, moves, types and abilities and battle level, and
/// whether that level is from our own file (its XP is known).
#[derive(Default)]
struct Known {
    partner: Option<Partner>,
    moves: Moves,
    traits: Traits,
    level: Level,
    ours: bool,
}

/// What is known of the Chao with `identity`. Ours come from `partners.txt`, and a Chao in our
/// save (`own`) only from there, so other players' data never changes it; another player's
/// Chao from `partners.txt` (a copy we have too), then from what they sent, then from
/// `chao/import/partners.txt`.
fn record(identity: u64, own: bool) -> Known {
    // Abilities older files didn't record come from the ROM.
    let known = |r: store::Record, ours| Known {
        traits: match loaded_rom() {
            Some(rom) => profile::recorded(&rom, r.partner.as_ref(), r.traits),
            None => r.traits,
        },
        partner: r.partner,
        moves: r.moves,
        level: r.level,
        ours,
    };
    if let Some(r) = with_store(|s| s.record(identity)) {
        return known(r, true);
    }
    if own {
        return Known {
            ours: true,
            ..Known::default()
        };
    }
    let other = lock(&OTHERS).get(&identity).cloned();
    let level = lock(&OTHER_LEVELS).get(&identity).copied();
    if other.is_some() || level.is_some() {
        return Known {
            partner: other.as_ref().map(|(l, _)| l.partner),
            moves: other
                .as_ref()
                .map_or_else(Moves::default, |(l, _)| Moves::from_learned(l)),
            traits: other.map(|(_, t)| t).unwrap_or_default(),
            level: level.unwrap_or_default(),
            ours: false,
        };
    }
    let path = Path::new(crate::online::mod_path()).join(IMPORTED_PARTNERS);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| Store::parse(&text).record(identity))
        .map_or_else(Known::default, |r| known(r, false))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The battle profile of the Chao with `identity`: the types, ability and four moves it
/// battles with and its battle level, or Normal type with basic moves without a partner
/// (`record` says where it comes from). `moves` and `traits` are given for a Chao in our save
/// (what it battles with), and only our own file counts for it. Without `save_partners`, what
/// partners from a GBA save gave is left out (`profile`). Loads the ROM if needed.
pub fn battle_profile(
    identity: u64,
    moves: Option<&Moves>,
    traits: Option<&Traits>,
    save_partners: bool,
) -> Result<Profile, String> {
    let rom = rom()?;
    let k = record(identity, moves.is_some() || traits.is_some());
    Ok(profile::profile(
        &rom,
        k.partner.as_ref(),
        traits.unwrap_or(&k.traits),
        moves.unwrap_or(&k.moves),
        save_partners,
        k.level.level,
    ))
}

/// The battle level of the Chao with `identity` (a new Chao's when nothing has one), found as
/// `battle_profile` finds it (`own`: a Chao in our save), and whether its XP is known (ours;
/// other players send only the level).
pub fn battle_level(identity: u64, own: bool) -> (Level, bool) {
    let k = record(identity, own);
    (k.level, k.ours)
}

/// Our Chao's battle levels (the ones not here are at a new Chao's).
pub fn levels() -> BTreeMap<u64, Level> {
    with_store(|s| s.levels.clone())
}

/// Keeps these battle levels of our Chao (after a battle) and tells the others.
pub fn set_levels(levels: &BTreeMap<u64, Level>) {
    with_store(|s| s.levels.extend(levels.iter().map(|(&id, &l)| (id, l))));
    write_store();
    crate::online::resend_partners();
}

/// Every move our Chao with `identity` learned, and the four it battles with.
pub fn moves_of(identity: u64) -> Moves {
    with_store(|s| s.moves_of(identity))
}

/// Keeps `moves` as our Chao's with `identity` (the four it battles with chosen, or a TM move
/// taught) and tells the others.
pub fn set_moves(identity: u64, moves: Moves) {
    with_store(|s| s.set_moves(identity, moves));
    write_store();
    crate::online::resend_partners();
}

/// Every type and ability our Chao with `identity` got, and the ones it battles with (an ability
/// older files didn't record from the ROM, once it's loaded).
pub fn traits_of(identity: u64) -> Traits {
    let (partner, traits) = with_store(|s| {
        (
            s.partners.get(&identity).map(|l| l.partner),
            s.traits_of(identity),
        )
    });
    match loaded_rom() {
        Some(rom) => profile::recorded(&rom, partner.as_ref(), traits),
        None => traits,
    }
}

/// Keeps `traits` as our Chao's with `identity` (its types or ability chosen) and tells the
/// others.
pub fn set_traits(identity: u64, traits: Traits) {
    with_store(|s| s.set_traits(identity, traits));
    write_store();
    crate::online::resend_partners();
}

/// What friends' Chao files carry from partners, under the mod folder.
const IMPORTED_PARTNERS: &str = "chao/import/partners.txt";

/// What our Chao have from partners, the types, ability and four moves they battle with and
/// their battle levels, to send to the others.
pub fn shared_partners() -> Vec<SharedPartner> {
    let rom = loaded_rom();
    with_store(|s| shared_of(s, rom.as_deref()))
}

/// Each Chao `store` has anything of: its partner's species (0 without one), the types it
/// battles as (Normal without), its active ability (0: none; one older files didn't record from
/// `rom`) and moves, and its battle level.
fn shared_of(store: &Store, rom: Option<&Rom>) -> Vec<SharedPartner> {
    store
        .identities()
        .into_iter()
        .map(|identity| {
            let partner = store.partners.get(&identity).map(|l| l.partner);
            let traits = store.traits_of(identity);
            let traits = match rom {
                Some(rom) => profile::recorded(rom, partner.as_ref(), traits),
                None => traits,
            };
            let types = match traits.active_type_list()[..] {
                [] => [profile::NORMAL; 2],
                [a] => [a; 2],
                [a, b, ..] => [a, b],
            };
            SharedPartner {
                identity,
                species: partner.map_or(0, |p| p.species),
                types,
                ability: traits.ability,
                moves: store.moves_of(identity).active,
                level: store.level(identity).level,
            }
        })
        .collect()
}

/// What another player's Chao have from their partners, and their battle levels.
pub fn others_learned(list: &[SharedPartner]) {
    let mut others = lock(&OTHERS);
    let mut levels = lock(&OTHER_LEVELS);
    for p in list {
        levels.insert(p.identity, Level::new(p.level, 0));
        if p.species == 0 && p.moves.iter().all(|&m| m == 0) {
            continue;
        }
        // The types and ability are the ones it battles with (0: no ability).
        let traits = if p.species == 0 {
            Traits::default()
        } else {
            Traits::chosen(p.types, p.ability)
        };
        others.insert(
            p.identity,
            (
                Learned {
                    partner: Partner {
                        species: p.species,
                        types: p.types,
                        moves: p.moves,
                        level: 0,
                        ability: p.ability,
                        from_save: false,
                    },
                    moves: p.moves,
                    save_moves: 0,
                },
                traits,
            ),
        );
    }
}

/// The `partners.txt` that goes with exported Chao files: for each of `identities` our store
/// has anything of, its partner (species 0 without one), its types and abilities, the four moves
/// it battles with (not its whole move list) and its battle level. `None` when there's nothing
/// to write.
fn export_of(store: &Store, identities: &[u64]) -> Option<Store> {
    let mut out = Store::default();
    for &identity in identities {
        let Some(store::Record {
            partner,
            moves,
            traits,
            ..
        }) = store.record(identity)
        else {
            continue;
        };
        if let Some(&l) = store.levels.get(&identity) {
            out.levels.insert(identity, l);
        }
        if partner.is_none() && moves.active.iter().all(|&m| m == 0) {
            continue;
        }
        let partner = partner.unwrap_or(Partner {
            species: 0,
            types: [profile::NORMAL; 2],
            moves: [0; 4],
            level: 0,
            ability: 0,
            from_save: false,
        });
        out.partners.insert(
            identity,
            Learned {
                partner,
                moves: moves.active,
                save_moves: moves.active_marks(),
            },
        );
        if partner.species != 0 {
            out.traits.insert(identity, traits);
        }
    }
    (!out.partners.is_empty() || !out.levels.is_empty()).then_some(out)
}

/// Writes `partners.txt` into `dir` for the Chao with these identities (`export_of`), next to
/// their exported Chao files (none: no file).
pub fn export_partners(dir: &Path, identities: &[u64]) {
    let export = with_store(|s| export_of(s, identities));
    let path = dir.join(STORE_FILE);
    let result = match export {
        None => std::fs::remove_file(&path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        }),
        Some(store) => std::fs::write(&path, store.to_text()),
    };
    if let Err(e) = result {
        log!("gba link: can't write {}: {e}", path.display());
    }
}

/// "<name> learned <move>!" for the moves new to a Chao (the ROM's names), as one message.
fn learned_message(name: &str, new: &[u16]) -> Option<String> {
    let rom = loaded_rom()?;
    let names: Vec<String> = new
        .iter()
        .filter_map(|&id| rom.move_name(id))
        .map(|n| ascii(&n))
        .collect();
    let list = match names.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!("{name} learned {list}!"))
}

/// A Chao (`identity`, `name`) absorbed the partner `key` waiting in `garden`: it keeps it as
/// its partner, which teaches it its moves, types and ability (the Chao record isn't changed).
/// The moves new to it are shown.
fn absorbed(garden: u8, key: u64, identity: u64, name: &str) {
    let Some((learned, new)) = with_store(|s| s.absorbed(garden, key, identity)) else {
        return;
    };
    write_store();
    crate::online::resend_partners();
    if let Some(text) = learned_message(name, &new) {
        crate::hud::toast(text);
    }
    log!(
        "gba link: a Chao absorbed species {} (types {:?}, ability {}, moves {:?})",
        learned.partner.species,
        learned.partner.types,
        learned.partner.ability,
        learned.moves
    );
}

/// Each frame: the garden's waiting partners (scene.rs) and wild visitors (wild.rs).
pub fn on_frame() {
    scene::on_frame();
    // Before `wild`: a shared visitor carried out of the garden becomes its to carry on.
    shared::on_frame();
    wild::on_frame();
    machine::on_frame();
    wild_battle::on_frame();
    card::on_frame();
}

/// The note left in a new `gba` folder, saying what goes in it.
const FOLDER_NOTE: &str = "PUT YOUR ROM HERE.txt";
const FOLDER_NOTE_TEXT: &str = "Put your own ROM of Emerald for the Game Boy Advance here \
(the US release, game code BPEE, 16 MB), dumped from a cartridge you own. Its file name must \
end in .gba (check that Windows isn't hiding a second extension). A save file from it (.sav) \
is optional. Then start the game again.\r\n";

/// Makes the mod's `gba` folder when there's none, and while it has no `.gba` file leaves the
/// note saying what goes in it and logs what it does hold (a ROM still zipped or named
/// `.gba.gba` shows up there).
fn make_folder() {
    let dir = folder();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log!("gba link: can't make {}: {e}", dir.display());
        return;
    }
    if with(|l| !l.paths.rom.trim().is_empty()) {
        // The ROM is set elsewhere (`GbaRom`).
        return;
    }
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n != FOLDER_NOTE)
                .collect()
        })
        .unwrap_or_default();
    if names.iter().any(|n| has_extension(n, "gba")) {
        return;
    }
    let note = dir.join(FOLDER_NOTE);
    if !note.exists()
        && let Err(e) = std::fs::write(&note, FOLDER_NOTE_TEXT)
    {
        log!("gba link: can't write {}: {e}", note.display());
    }
    log!(
        "gba link: no ROM (.gba) in {}{}",
        dir.display(),
        if names.is_empty() {
            String::new()
        } else {
            format!("; it holds: {}", names.join(", "))
        }
    );
}

/// `name` ends in `.extension` (any case).
fn has_extension(name: &str, extension: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

pub fn install() {
    make_folder();
    crate::registry::raise_animal_limit();
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
    scene::is_partner_object(object) || wild::is_visitor(object) || shared::is_copy(object)
}

/// Settings from config.ini (`SharedVisitors`): the gardens the server runs show their shared
/// wild visitors and link machine.
pub fn set_shared_visitors(on: bool) {
    shared::set_enabled(on);
}

/// Network: one of the host's messages about the shared visitors (`Body::WildList`,
/// `WildAsk`, `WildAnswer`, `WildTaught`, `WildUsed`); anyone else's is dropped.
pub fn received_wild(from: u8, body: crate::net::protocol::Body) {
    shared::received(from, body);
}

/// Network: our messages about the shared visitors for the host, (body, reliable).
pub fn take_wild_outgoing() -> Vec<(crate::net::protocol::Body, bool)> {
    shared::take_outgoing()
}

/// Network: connected or disconnected (`WildHello` goes again).
pub fn wild_connected() {
    shared::connected();
}

/// Chao `chao` keeps off the shared visitor the player holds (its owner can't learn from it).
pub fn keeps_off_visitor(chao: usize) -> bool {
    shared::keeps_off_visitor(chao)
}

/// Settings from config.ini (`WildVisitors`): wild visitors come to the gardens.
pub fn set_wild_visitors(on: bool) {
    wild::set_enabled(on);
}

/// A Chao (`identity`, `name`) absorbed a wild visitor: it keeps it as its partner, as with
/// `absorbed`.
fn learned_wild(identity: u64, partner: Partner, name: &str) {
    let (learned, new) = with_store(|s| s.learn(identity, partner));
    write_store();
    crate::online::resend_partners();
    if let Some(text) = learned_message(name, &new) {
        crate::hud::toast(text);
    }
    log!(
        "gba link: a Chao absorbed a wild species {} (types {:?}, ability {}, moves {:?})",
        learned.partner.species,
        learned.partner.types,
        learned.partner.ability,
        learned.moves
    );
}

/// Brings `c` into the garden we're in as `import` does, coming out at `at` with a hop along
/// `angle` (BAMS) instead of walking in ahead of the player (the link machine). The message to
/// show, or why it can't come.
pub fn bring_out(c: &Candidate, at: [f32; 3], angle: i32) -> Result<String, String> {
    if crate::game::in_garden() && crate::garden::server_runs() {
        return bring_shared(c, at, angle);
    }
    if crate::game::in_garden() {
        let garden = crate::game::current_chao_area() as u8;
        with_store(|s| s.can_import(c.key, garden, today()))
            .map_err(|r| r.message().to_string())?;
        if !wild::room_for_partner(garden) {
            return Err("The garden is full of animals right now".into());
        }
    }
    let name = import_checked(c)?;
    scene::arrive_at(c.key, at, angle);
    Ok(format!("{name} came out of the link machine"))
}

/// `bring_out` in a garden the server runs: the server is asked to take it as a shared visitor;
/// it's recorded as brought only once it does (`shared`).
fn bring_shared(c: &Candidate, at: [f32; 3], angle: i32) -> Result<String, String> {
    let garden = crate::game::current_chao_area() as u8;
    with_store(|s| s.can_import(c.key, garden, today())).map_err(|r| r.message().to_string())?;
    let rom = rom()?;
    let species = rom
        .species(c.species)
        .ok_or("The ROM has no data for this one")?;
    let partner = Partner {
        species: c.species,
        types: species.types,
        moves: c.moves,
        level: c.level.clamp(1, 100),
        ability: ability_of(&species, c.ability_slot),
        from_save: matches!(c.source, Source::Party(_) | Source::Box { .. }),
    };
    let name = ascii(&rom.species_name(c.species).unwrap_or_default());
    shared::bring(shared::Bring {
        key: c.key,
        garden,
        partner,
        name: name.clone(),
        at: Some((at, angle)),
    })?;
    Ok(format!("{name} is coming out of the link machine"))
}

/// Settings from config.ini (`LinkMachine`): each garden has its link machine, which replaces
/// the F9 GBA link page.
pub fn set_link_machine(on: bool) {
    machine::set_enabled(on);
}

/// The gardens have their link machine (the F9 GBA link page only points to it).
pub fn link_machine() -> bool {
    machine::enabled()
}

/// What picking a row of the GBA link list (the link machine's or the F9 page) does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Nothing (a message).
    None,
    /// Bring this one to the garden.
    Bring(Candidate),
    /// Take this first-partner species as the gift.
    Gift(u16),
    /// The link machine's list of our Chao, to choose the moves, types and ability one battles
    /// with (`Page::chao_list`).
    ChaoList,
    /// What our Chao with this identity battles with: its moves, types and ability, each to
    /// choose (`Page::chao_choices`).
    Chao(u64),
    /// The moves our Chao with this identity learned (`Page::chao_moves`).
    ChaoMoves(u64),
    /// The types our Chao with this identity got (`Page::chao_types`).
    ChaoTypes(u64),
    /// The abilities our Chao with this identity got (`Page::chao_abilities`).
    ChaoAbilities(u64),
    /// Switch move `id` of our Chao `identity` on or off for battles (`toggle_move`).
    Toggle { identity: u64, id: u16 },
    /// Switch type `id` of our Chao `identity` on or off for battles (`toggle_type`).
    ToggleType { identity: u64, id: u8 },
    /// Our Chao `identity` battles with ability `id` (0: none) (`choose_ability`).
    Ability { identity: u64, id: u8 },
    /// The link machine's choice of garden for who can come there (`Page::who_gardens`).
    WhoComes,
    /// Who can come to garden `garden` (1..=3) from its wild area now (`Page::who_comes`).
    WhoNow(u8),
    /// The same for its next window's area.
    WhoNext(u8),
    /// A species that can come (picking it does nothing; the machine's screen shows it).
    Look(u16),
    /// Back to the list before.
    Back,
}

impl Action {
    /// It opens another page of the link machine's list.
    pub fn opens_page(&self) -> bool {
        matches!(
            self,
            Self::ChaoList
                | Self::Chao(_)
                | Self::ChaoMoves(_)
                | Self::ChaoTypes(_)
                | Self::ChaoAbilities(_)
                | Self::WhoComes
                | Self::WhoNow(_)
                | Self::WhoNext(_)
        )
    }
}

/// The GBA link list: the save's creatures and the mod's kept ones, one row each; without a save,
/// the first-partner gift until it's taken. The link machine's Chao, move, type and ability
/// lists are pages too.
pub struct Page {
    pub title: String,
    pub rows: Vec<(String, Action)>,
    /// Each row's name for the keyboard search (`crate::search`); rows without one (past the
    /// end too) always show.
    pub names: Vec<Option<String>>,
    /// A line about each row, shown under the list while it's highlighted (none past the end).
    pub notes: Vec<Option<String>>,
    pub row: usize,
    pub scroll: usize,
    /// What the keys do here (the link machine's own when empty).
    pub hints: Vec<String>,
}

/// The gift's level.
const GIFT_LEVEL: u8 = 5;

/// The last hint lines of the link machine's pages, with what B does (`back`: "back" or
/// "close").
pub fn nav_hints(back: &str) -> [String; 2] {
    [
        "Up/Down pick, Left/Right jump 10, PgUp/PgDn a page.".into(),
        format!("Type to search. B / Esc / Backspace {back}."),
    ]
}

impl Page {
    fn new(title: String, rows: Vec<(String, Action)>, hints: Vec<String>) -> Self {
        Self {
            title,
            rows,
            names: Vec::new(),
            notes: Vec::new(),
            row: 0,
            scroll: 0,
            hints,
        }
    }

    /// Row `i`'s search name.
    pub fn name(&self, i: usize) -> Option<&str> {
        self.names.get(i)?.as_deref()
    }

    /// Row `i`'s note.
    pub fn note(&self, i: usize) -> Option<&str> {
        self.notes.get(i)?.as_deref()
    }

    /// Adds a row with its search name.
    fn push(&mut self, text: String, action: Action, name: Option<String>) {
        self.names.resize(self.rows.len(), None);
        self.rows.push((text, action));
        self.names.push(name);
    }

    /// Puts a row without a search name or note first.
    pub fn push_front(&mut self, text: String, action: Action) {
        self.rows.insert(0, (text, action));
        if !self.names.is_empty() {
            self.names.insert(0, None);
        }
        if !self.notes.is_empty() {
            self.notes.insert(0, None);
        }
    }

    /// Reads the ROM and save now.
    pub fn open() -> Self {
        let mut page = Self::new("Link machine".into(), Vec::new(), Vec::new());
        match rom() {
            Err(e) => {
                log!("gba link: {e}");
                page.push(e, Action::None, None);
            }
            Ok(rom) => {
                let name = |species: u16| rom.species_name(species).map(|n| ascii(&n));
                let save = read_save();
                if let Ok(save) = &save {
                    for c in save
                        .party
                        .iter()
                        .chain(&save.boxes)
                        .filter_map(|m| Candidate::from_mon(&rom, m))
                    {
                        let n = name(c.species);
                        page.push(label(&rom, &c), Action::Bring(c), n);
                    }
                    if save.skipped > 0 {
                        page.push(
                            format!("({} damaged entries skipped)", save.skipped),
                            Action::None,
                            None,
                        );
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
                    page.push(label(&rom, &c), Action::Bring(c), name(species));
                }
                if let Err(e) = &save {
                    log!("gba link: {e}");
                    page.push(e.clone(), Action::None, None);
                    if gift.is_none()
                        && let Some(starters) = rom.starters()
                    {
                        page.push(
                            "Or take one gift partner (only once):".into(),
                            Action::None,
                            None,
                        );
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
                            page.push(text, Action::Gift(species), name(species));
                        }
                    }
                }
                if page.rows.is_empty() {
                    page.push("The save has no one to bring".into(), Action::None, None);
                }
            }
        }
        page
    }

    /// Our Chao (hatched, in any garden), to choose what one battles with.
    pub fn chao_list() -> Self {
        let mut page = Self::new(
            "Moves, type & ability: which Chao?".into(),
            Vec::new(),
            [
                "A / Enter: what it battles with. Each Chao remembers".into(),
                "every move, type and ability its partners gave it.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        for (identity, name) in own_chao() {
            let row = chao_row(&name, &moves_of(identity));
            page.push(row, Action::Chao(identity), Some(name));
        }
        if page.rows.is_empty() {
            page.push("You have no Chao to choose for".into(), Action::None, None);
        }
        page
    }

    /// What our Chao `identity` battles with: its moves, types and ability, each a page.
    pub fn chao_choices(identity: u64) -> Self {
        let rom = loaded_rom();
        let rows = choice_rows(
            identity,
            &moves_of(identity),
            &traits_of(identity),
            |t| type_name(rom.as_deref(), t),
            |a| ability_name(rom.as_deref(), a),
        );
        Self::new(
            format!("{}: moves, type & ability", chao_name(identity)),
            rows,
            [
                "A / Enter: choose. Changes are kept at once.".into(),
                "Up/Down pick. B / Esc / Backspace back.".into(),
            ]
            .into(),
        )
    }

    /// Every move our Chao `identity` learned, each switched on or off for battles.
    pub fn chao_moves(identity: u64) -> Self {
        let rom = loaded_rom();
        let describe = |id: u16| match &rom {
            Some(rom) => move_label(rom, id),
            None => format!("Move #{id}"),
        };
        let moves = moves_of(identity);
        let mut page = Self::new(
            format!("{}'s moves (4 at most)", chao_name(identity)),
            move_rows(identity, &moves, describe),
            [
                "A / Enter: use or drop a move (kept at once). [x]: it".into(),
                "battles with it. GBA save: only a save's partner taught it,".into(),
                "so live battles without GBA save partners leave it out.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        page.names = moves
            .known
            .iter()
            .map(|&(id, _)| {
                rom.as_ref()
                    .and_then(|r| r.move_name(id))
                    .map(|n| ascii(&n))
            })
            .collect();
        page
    }

    /// Every type our Chao `identity` got, each switched on or off for battles (two at most).
    pub fn chao_types(identity: u64) -> Self {
        let rom = loaded_rom();
        let traits = traits_of(identity);
        let mut page = Self::new(
            format!("{}'s type (2 at most)", chao_name(identity)),
            type_rows(identity, &traits, |t| type_name(rom.as_deref(), t)),
            [
                "A / Enter: use or drop a type (kept at once). [x]: it".into(),
                "battles as it. GBA save: only a save's partner gave it,".into(),
                "so live battles without GBA save partners leave it out.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        page.names = traits
            .types
            .iter()
            .map(|&(t, _)| Some(type_name(rom.as_deref(), t)))
            .collect();
        page
    }

    /// Every ability our Chao `identity` got, one (or none) chosen for battles, each with the
    /// ROM's description.
    pub fn chao_abilities(identity: u64) -> Self {
        let rom = loaded_rom();
        let traits = traits_of(identity);
        let mut page = Self::new(
            format!("{}'s ability", chao_name(identity)),
            ability_rows(identity, &traits, |a| ability_name(rom.as_deref(), a)),
            [
                "A / Enter: battle with it (kept at once). (o): its ability.".into(),
                "GBA save: only a save's partner gave it, so live battles".into(),
                "without GBA save partners leave it out.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        // Row 0 is "None".
        page.names = std::iter::once(None)
            .chain(
                traits
                    .abilities
                    .iter()
                    .map(|&(a, _)| Some(ability_name(rom.as_deref(), a))),
            )
            .collect();
        page.notes = std::iter::once(Some("It battles without an ability.".to_string()))
            .chain(traits.abilities.iter().map(|&(a, _)| {
                rom.as_ref()
                    .and_then(|r| r.ability_description(a))
                    .map(|d| ascii(&d.replace('\n', " ")))
            }))
            .collect();
        page
    }
}

/// The gardens' names by `CurrentChaoArea` 1..=3.
const GARDEN_NAMES: [&str; 3] = ["Neutral Garden", "Hero Garden", "Dark Garden"];

fn garden_name(garden: u8) -> &'static str {
    GARDEN_NAMES
        .get(usize::from(garden).wrapping_sub(1))
        .copied()
        .unwrap_or("Garden")
}

/// "1 minute", "N minutes".
fn minutes(n: u64) -> String {
    if n == 1 {
        "1 minute".into()
    } else {
        format!("{n} minutes")
    }
}

/// A visitor's odds as a percentage: whole from 10%, else to a tenth.
fn percent(odds: f64) -> String {
    let p = odds * 100.0;
    if p >= 9.95 {
        format!("{p:.0}%")
    } else {
        format!("{p:.1}%")
    }
}

/// A level range ("Lv5", "Lv3-5").
fn level_range(min: u8, max: u8) -> String {
    if min == max {
        format!("Lv{min}")
    } else {
        format!("Lv{min}-{max}")
    }
}

/// The ROM's wild areas (read once in the game, `wild::areas`).
fn wild_areas(rom: &Rom) -> Option<Arc<Vec<rotation::Area>>> {
    #[cfg(target_arch = "x86")]
    {
        let _ = rom;
        wild::areas()
    }
    #[cfg(not(target_arch = "x86"))]
    {
        rotation::areas(rom).map(Arc::new)
    }
}

impl Page {
    /// The link machine's choice of garden for who can come there: the one we're in (`here`)
    /// first.
    pub fn who_gardens(here: u8) -> Self {
        let mut page = Self::new(
            "Who can come here: which garden?".into(),
            Vec::new(),
            [
                "A / Enter: its wild area now, how long it stays, and".into(),
                "every kind that can come from it.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        let mut gardens = vec![here];
        gardens.extend((1..=3).filter(|&g| g != here));
        for garden in gardens.into_iter().filter(|g| (1..=3).contains(g)) {
            let name = garden_name(garden);
            let text = if garden == here {
                format!("{name} (here)")
            } else {
                name.to_string()
            };
            page.push(text, Action::WhoNow(garden), Some(name.to_string()));
        }
        page
    }

    /// Who can come to garden `garden` at `unix` (`next`: in the window after): its area (the
    /// ROM's name), how long it lasts, now also the next window's area to see, then every
    /// species that can come with its level range, where from and its odds (`wild::who_comes`,
    /// what the picking uses), its types and abilities in its note.
    pub fn who_comes(
        rom: &Rom,
        areas: &[rotation::Area],
        garden: u8,
        next: bool,
        unix: u64,
    ) -> Self {
        let slot = rotation::slot_of(unix) + u32::from(next);
        let left = minutes(rotation::minutes_left(unix));
        let mut page = Self::new(
            format!(
                "Who can come to the {}{}",
                garden_name(garden),
                if next { " next" } else { "" }
            ),
            Vec::new(),
            [
                "%: of the visitors to the grass (grass, rocks) or to the".into(),
                "pond (pond, fishing). Kinds that just came come less often.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        let area_name = |a: &rotation::Area| {
            rom.map_section_name(a.section)
                .map(|n| ascii(&n))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Area {}", a.section))
        };
        let Some(area) = rotation::area_at(areas, garden, slot) else {
            page.push(
                "No wild area of the ROM for this garden".into(),
                Action::None,
                None,
            );
            return page;
        };
        if next {
            page.push(
                format!("{}: in {left}, for 10 minutes", area_name(area)),
                Action::None,
                None,
            );
        } else {
            page.push(
                format!("{}: for the next {left}", area_name(area)),
                Action::None,
                None,
            );
            if let Some(after) = rotation::area_at(areas, garden, slot + 1) {
                page.push(
                    format!("Next: {} in {left} (A: who comes)", area_name(after)),
                    Action::WhoNext(garden),
                    None,
                );
            }
        }
        let types = |s: u16| rom.species(s).map(|i| i.types);
        for c in wild::who_comes(area, types) {
            let name = rom
                .species_name(c.species)
                .map_or_else(|| format!("#{}", c.species), |n| ascii(&n));
            let text = format!(
                "{name:<10}  {:<8}  {:<7}  {:>5}",
                level_range(c.min_level, c.max_level),
                c.from.label(),
                percent(c.odds)
            );
            page.notes.resize(page.rows.len(), None);
            page.notes.push(species_note(rom, c.species));
            page.push(text, Action::Look(c.species), Some(name));
        }
        page
    }

    /// `who_comes` from the player's ROM by the clock now.
    pub fn who_comes_now(garden: u8, next: bool) -> Self {
        let rom = match rom() {
            Ok(rom) => rom,
            Err(e) => {
                let mut page = Self::new("Who can come here".into(), Vec::new(), Vec::new());
                page.push(e, Action::None, None);
                return page;
            }
        };
        match wild_areas(&rom) {
            Some(areas) => Self::who_comes(&rom, &areas, garden, next, rotation::unix_now()),
            None => {
                let mut page = Self::new("Who can come here".into(), Vec::new(), Vec::new());
                page.push(
                    "The ROM's wild tables can't be read".into(),
                    Action::None,
                    None,
                );
                page
            }
        }
    }
}

/// A species' types and abilities from the ROM, for the line under the list.
fn species_note(rom: &Rom, species: u16) -> Option<String> {
    let info = rom.species(species)?;
    let types: Vec<String> = type_names(rom, info.types)
        .iter()
        .map(|t| ascii(t))
        .collect();
    let mut abilities: Vec<String> = Vec::new();
    for a in info.abilities.into_iter().filter(|&a| a != 0) {
        let name = ability_name(Some(rom), a);
        if !abilities.contains(&name) {
            abilities.push(name);
        }
    }
    Some(format!(
        "{}  |  {}",
        types.join("/"),
        if abilities.is_empty() {
            "No ability".to_string()
        } else {
            abilities.join(" or ")
        }
    ))
}

/// Our hatched Chao in the save, any garden: (identity, name).
fn own_chao() -> Vec<(u64, String)> {
    (0..crate::game::GARDEN_SLOTS)
        .filter_map(crate::game::read_chao_slot)
        .filter(|c| c.chao_type() > crate::chao::TYPE_EGG && c.validate().is_ok())
        .map(|c| (c.identity(), crate::karate::chao_name(&c)))
        .collect()
}

/// The name of our Chao with `identity` ("Chao" when it's not in the save).
fn chao_name(identity: u64) -> String {
    own_chao()
        .into_iter()
        .find(|(id, _)| *id == identity)
        .map_or_else(|| "Chao".to_string(), |(_, name)| name)
}

/// The ROM's name of type `t` ("Type #t" without the ROM).
fn type_name(rom: Option<&Rom>, t: u8) -> String {
    rom.and_then(|r| r.type_name(t))
        .map_or_else(|| format!("Type #{t}"), |n| ascii(&n))
}

/// The ROM's name of ability `a` ("Ability #a" without the ROM).
fn ability_name(rom: Option<&Rom>, a: u8) -> String {
    rom.and_then(|r| r.ability_name(a))
        .map_or_else(|| format!("Ability #{a}"), |n| ascii(&n))
}

/// A Chao's row in the link machine's list: its name and how many moves it uses and knows.
fn chao_row(name: &str, m: &Moves) -> String {
    let on = m.active.iter().filter(|&&id| id != 0).count();
    format!("{name:<7}  {on} of {} moves", m.known.len())
}

/// The rows of what a Chao battles with (`type_name` and `ability_name` name them): its moves,
/// types and ability, each opening its page, then "Done".
fn choice_rows(
    identity: u64,
    m: &Moves,
    t: &Traits,
    type_name: impl Fn(u8) -> String,
    ability_name: impl Fn(u8) -> String,
) -> Vec<(String, Action)> {
    let on = m.active.iter().filter(|&&id| id != 0).count();
    let types: Vec<String> = t.active_type_list().into_iter().map(type_name).collect();
    vec![
        (
            format!("Moves: {on} of {} on", m.known.len()),
            Action::ChaoMoves(identity),
        ),
        (
            if types.is_empty() {
                "Type: Normal (none from partners yet)".into()
            } else {
                format!("Type: {}", types.join("/"))
            },
            Action::ChaoTypes(identity),
        ),
        (
            match t.ability {
                0 => "Ability: none".into(),
                a => format!("Ability: {}", ability_name(a)),
            },
            Action::ChaoAbilities(identity),
        ),
        ("Done".into(), Action::Back),
    ]
}

/// A move's name, type and power from the ROM ("Name  Type 40").
fn move_label(rom: &Rom, id: u16) -> String {
    let name = rom
        .move_name(id)
        .map_or_else(|| format!("Move #{id}"), |n| ascii(&n));
    match rom.move_info(id) {
        Some(m) => {
            let kind = rom
                .type_name(m.type_id)
                .map(|t| ascii(&t))
                .unwrap_or_default();
            if m.power > 1 {
                format!("{name}  {kind} {}", m.power)
            } else {
                format!("{name}  {kind}")
            }
        }
        None => name,
    }
}

/// The rows of a Chao's moves (`describe` names one): "[x]" on the ones it battles with,
/// "(GBA save)" on the ones only partners from a GBA save taught it, then "Done".
fn move_rows(identity: u64, m: &Moves, describe: impl Fn(u16) -> String) -> Vec<(String, Action)> {
    let mut rows: Vec<(String, Action)> = m
        .known
        .iter()
        .map(|&(id, marked)| {
            let on = if m.is_active(id) { "[x]" } else { "[ ]" };
            let save = if marked { "  (GBA save)" } else { "" };
            (
                format!("{on} {}{save}", describe(id)),
                Action::Toggle { identity, id },
            )
        })
        .collect();
    if rows.is_empty() {
        rows.push((
            "No moves learned yet: it battles with basic moves".into(),
            Action::None,
        ));
    }
    rows.push(("Done".into(), Action::Back));
    rows
}

/// The rows of a Chao's types (`name` names one): "[x]" on the ones it battles as, "(GBA save)"
/// on the ones only partners from a GBA save gave it, then "Done".
fn type_rows(identity: u64, t: &Traits, name: impl Fn(u8) -> String) -> Vec<(String, Action)> {
    let mut rows: Vec<(String, Action)> = t
        .types
        .iter()
        .map(|&(id, marked)| {
            let on = if t.is_type_active(id) { "[x]" } else { "[ ]" };
            let save = if marked { "  (GBA save)" } else { "" };
            (
                format!("{on} {}{save}", name(id)),
                Action::ToggleType { identity, id },
            )
        })
        .collect();
    if rows.is_empty() {
        rows.push((
            "No types from partners yet: it battles as Normal".into(),
            Action::None,
        ));
    }
    rows.push(("Done".into(), Action::Back));
    rows
}

/// The rows of a Chao's abilities (`name` names one): "None", then each, "(o)" on the one it
/// battles with and "(GBA save)" on the ones only partners from a GBA save gave it, then "Done".
fn ability_rows(identity: u64, t: &Traits, name: impl Fn(u8) -> String) -> Vec<(String, Action)> {
    let mark = |on: bool| if on { "(o)" } else { "( )" };
    let mut rows = vec![(
        format!("{} None", mark(t.ability == 0)),
        Action::Ability { identity, id: 0 },
    )];
    rows.extend(t.abilities.iter().map(|&(id, marked)| {
        let save = if marked { "  (GBA save)" } else { "" };
        (
            format!("{} {}{save}", mark(t.ability == id), name(id)),
            Action::Ability { identity, id },
        )
    }));
    rows.push(("Done".into(), Action::Back));
    rows
}

/// Switches move `id` of our Chao `identity` on or off for battles and keeps it (`set_moves`);
/// whether it's on now.
pub fn toggle_move(identity: u64, id: u16) -> Result<bool, store::MoveRefused> {
    let mut m = moves_of(identity);
    let on = m.toggle(id)?;
    set_moves(identity, m);
    Ok(on)
}

/// Switches type `id` of our Chao `identity` on or off for battles and keeps it (`set_traits`);
/// whether it's on now.
pub fn toggle_type(identity: u64, id: u8) -> Result<bool, store::TraitRefused> {
    let mut t = traits_of(identity);
    let on = t.toggle_type(id)?;
    set_traits(identity, t);
    Ok(on)
}

/// Our Chao `identity` battles with ability `id` (0: none) from now on (`set_traits`).
pub fn choose_ability(identity: u64, id: u8) -> Result<(), store::TraitRefused> {
    let mut t = traits_of(identity);
    t.choose_ability(id)?;
    set_traits(identity, t);
    Ok(())
}

/// Takes the gift `species` (one of the ROM's first partners, once only). The message to show.
pub fn take_gift(species: u16) -> String {
    match take_gift_candidate(species) {
        Ok(c) => {
            let name = loaded_rom()
                .and_then(|r| r.species_name(c.species))
                .map(|n| ascii(&n))
                .unwrap_or_default();
            format!("{name} is yours: F9 > GBA link brings it to a garden")
        }
        Err(e) => e,
    }
}

/// Takes the gift `species` (once only): it's kept by the mod from now on. The kept one, to
/// bring into a garden.
pub fn take_gift_candidate(species: u16) -> Result<Candidate, String> {
    let rom = rom().map_err(|_| "No GBA ROM".to_string())?;
    if !rom.starters().is_some_and(|s| s.contains(&species)) {
        return Err("Not a gift partner".into());
    }
    let moves = rom.moves_at_level(species, GIFT_LEVEL);
    let key = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ (u64::from(species) << 48)
        | 1 << 63;
    if !with_store(|s| s.take_gift(key, species, GIFT_LEVEL, moves)) {
        return Err("The gift was taken already".into());
    }
    write_store();
    log!("gba link: took gift species {species}");
    Ok(Candidate {
        source: Source::Kept,
        key,
        species,
        level: GIFT_LEVEL,
        moves,
        ability_slot: 0,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_real_gba_name_counts_as_a_rom() {
        use super::has_extension;
        assert!(has_extension("emerald.gba", "gba"));
        assert!(has_extension("Emerald.GBA", "gba"));
        assert!(!has_extension("emerald.gba.zip", "gba"));
        assert!(!has_extension("emerald.gba.7z", "gba"));
        assert!(!has_extension("gba", "gba"));
    }

    use super::*;
    use crate::gba::rom::Species;

    fn store() -> Store {
        let mut s = Store::default();
        let partner = Partner {
            species: 25,
            types: [13, 13],
            moves: [84, 45, 86, 98],
            level: 12,
            ability: 9,
            from_save: true,
        };
        s.learn(1, partner);
        s.learn(
            1,
            Partner {
                moves: [33, 0, 0, 0],
                from_save: false,
                ..partner
            },
        );
        // TM moves only, and a level only.
        let mut tm = Moves::default();
        tm.teach_tm(15, 0);
        s.set_moves(2, tm);
        s.levels.insert(1, Level::new(40, 7));
        s.levels.insert(3, Level::new(9, 0));
        s
    }

    #[test]
    fn friends_get_the_active_moves_and_levels_of_every_chao() {
        let mut s = store();
        // Chao 1 battles as its first partner's type and a later one's, with no ability.
        let mut t = s.traits_of(1);
        t.learn(&Partner {
            species: 30,
            types: [2, 2],
            moves: [0; 4],
            level: 5,
            ability: 20,
            from_save: false,
        });
        t.toggle_type(2).unwrap();
        t.choose_ability(0).unwrap();
        s.set_traits(1, t);
        let shared = shared_of(&s, None);
        assert_eq!(shared.len(), 3);
        assert_eq!(
            (shared[0].species, shared[0].moves, shared[0].level),
            (25, [84, 45, 86, 98], 40)
        );
        assert_eq!((shared[0].types, shared[0].ability), ([13, 2], 0));
        assert_eq!(shared_of(&store(), None)[0].ability, 9);
        assert_eq!(
            (
                shared[1].species,
                shared[1].types,
                shared[1].moves,
                shared[1].level
            ),
            (0, [profile::NORMAL; 2], [15, 0, 0, 0], 5)
        );
        assert_eq!((shared[2].moves, shared[2].level), ([0; 4], 9));
        // Received: the level always, a partner record only with something in it.
        let mut theirs = shared.clone();
        for p in &mut theirs {
            p.identity |= 0xF00D_0000_0000_0000;
        }
        others_learned(&theirs);
        let others = lock(&OTHERS);
        assert_eq!(others[&theirs[0].identity].0.moves, [84, 45, 86, 98]);
        assert!(others.contains_key(&theirs[1].identity));
        assert!(!others.contains_key(&theirs[2].identity));
        // What it battles with, as sent: no ability is none, not the species' first.
        let t = &others[&theirs[0].identity].1;
        assert_eq!((t.active_types, t.ability), ([13, 2], 0));
        assert_eq!(others[&theirs[1].identity].1, Traits::default());
        drop(others);
        assert_eq!(lock(&OTHER_LEVELS)[&theirs[2].identity].level, 9);
        assert_eq!(record(theirs[0].identity, false).traits.ability, 0);
    }

    #[test]
    fn our_chao_take_nothing_from_other_players() {
        with(|l| {
            l.store.get_or_insert_with(Store::default);
        });
        let id = 0xBEEF_0000_0000_0001;
        others_learned(&[SharedPartner {
            identity: id,
            species: 25,
            types: [13, 13],
            ability: 9,
            moves: [1, 0, 0, 0],
            level: 100,
        }]);
        // Another player's: their partner and level, without the XP.
        let (level, ours) = battle_level(id, false);
        assert_eq!((level.level, ours), (100, false));
        assert_eq!(record(id, false).partner.map(|p| p.species), Some(25));
        // A Chao in our save with the same identity: only our own file counts.
        assert_eq!(battle_level(id, true), (Level::default(), true));
        assert!(record(id, true).partner.is_none());
    }

    #[test]
    fn a_file_that_cant_be_read_is_an_error_and_a_missing_one_none() {
        let dir = std::env::temp_dir().join(format!("sa2co-gba-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(STORE_FILE);
        assert!(read_text(&path).unwrap().is_none());
        std::fs::write(&path, [b'l', b'e', 0xFF, 0xFE]).unwrap();
        assert!(read_text(&path).is_err());
        std::fs::write(&path, "level 1 5 0\n").unwrap();
        assert_eq!(read_text(&path).unwrap().as_deref(), Some("level 1 5 0\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exports_carry_the_active_moves_and_levels() {
        let s = store();
        assert!(export_of(&s, &[99]).is_none());
        let export = export_of(&s, &[1, 2, 3, 99]).unwrap();
        let back = Store::parse(&export.to_text());
        // Not the whole list: a friend's copy knows the four it battles with.
        let m = back.moves_of(1);
        assert_eq!(m.active, [84, 45, 86, 98]);
        assert!(!m.knows(33));
        assert_eq!(back.partners[&1].save_moves, 0b1111);
        assert_eq!(back.level(1), Level::new(40, 7));
        assert_eq!(back.partners[&2].partner.species, 0);
        assert_eq!(back.moves_of(2).active, [15, 0, 0, 0]);
        assert_eq!(back.level(3).level, 9);
        assert!(!back.partners.contains_key(&3));
        // Its types and abilities, with the ones it battles with.
        assert_eq!(back.traits_of(1), s.traits_of(1));
        assert_eq!(back.traits_of(1).active_types, [13, 13]);
        assert!(!back.traits.contains_key(&2));
    }

    #[test]
    fn the_machine_lists_what_a_chao_battles_with() {
        let mut m = Moves::default();
        m.learn([10, 20, 0, 0], false);
        let mut t = Traits::default();
        let rows = choice_rows(7, &m, &t, |id| format!("T{id}"), |id| format!("A{id}"));
        let texts: Vec<&str> = rows.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Moves: 2 of 2 on",
                "Type: Normal (none from partners yet)",
                "Ability: none",
                "Done"
            ]
        );
        assert_eq!(rows[1].1, Action::ChaoTypes(7));
        assert_eq!(rows[2].1, Action::ChaoAbilities(7));
        assert!(rows[..3].iter().all(|(_, a)| a.opens_page()));
        assert_eq!(rows[3].1, Action::Back);
        let type_none = type_rows(7, &t, |id| format!("T{id}"));
        assert_eq!(type_none[0].1, Action::None);
        let ability_none = ability_rows(7, &t, |id| format!("A{id}"));
        assert_eq!(ability_none[0].0, "(o) None");
        assert_eq!(ability_none.len(), 2);
        for (types, ability, from_save) in [([3, 3], 9, true), ([5, 6], 4, false)] {
            t.learn(&Partner {
                species: 1,
                types,
                moves: [0; 4],
                level: 5,
                ability,
                from_save,
            });
        }
        t.toggle_type(6).unwrap();
        let rows = choice_rows(7, &m, &t, |id| format!("T{id}"), |id| format!("A{id}"));
        assert_eq!(rows[1].0, "Type: T3/T6");
        assert_eq!(rows[2].0, "Ability: A9");
        let rows = type_rows(7, &t, |id| format!("T{id}"));
        let texts: Vec<&str> = rows.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(texts, ["[x] T3  (GBA save)", "[ ] T5", "[x] T6", "Done"]);
        assert_eq!(rows[1].1, Action::ToggleType { identity: 7, id: 5 });
        let rows = ability_rows(7, &t, |id| format!("A{id}"));
        let texts: Vec<&str> = rows.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(texts, ["( ) None", "(o) A9  (GBA save)", "( ) A4", "Done"]);
        assert_eq!(rows[0].1, Action::Ability { identity: 7, id: 0 });
        assert_eq!(rows[2].1, Action::Ability { identity: 7, id: 4 });
    }

    #[test]
    fn the_machine_lists_a_chaos_moves_to_switch() {
        let mut m = Moves::default();
        m.learn([10, 20, 0, 0], true);
        m.teach_tm(30, 0);
        m.learn([40, 0, 0, 0], false);
        assert_eq!(chao_row("Pip", &m), "Pip      4 of 4 moves");
        let rows = move_rows(7, &m, |id| format!("M{id}"));
        let texts: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            texts,
            [
                "[x] M10  (GBA save)",
                "[x] M20  (GBA save)",
                "[x] M30",
                "[x] M40",
                "Done"
            ]
        );
        assert_eq!(
            rows[2].1,
            Action::Toggle {
                identity: 7,
                id: 30
            }
        );
        assert_eq!(rows[4].1, Action::Back);
        let none = move_rows(7, &Moves::default(), |id| format!("M{id}"));
        assert_eq!(none[0].1, Action::None);
        assert_eq!(none[1].1, Action::Back);
    }

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
            exp_yield: 0,
        };
        assert_eq!((ability_of(&s, 0), ability_of(&s, 1)), (5, 9));
        s.abilities[1] = 0;
        assert_eq!(ability_of(&s, 1), 5);
    }
}

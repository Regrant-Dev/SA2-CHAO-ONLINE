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
//!   identity; its stats and save don't change. A Chao remembers every move it learned and
//!   battles with four of them, chosen at the link machine or the battle team screen. Its battle
//!   level and XP are kept there too. `battle_profile` gives what a battle uses (`profile`). The
//!   status balloon shows the partner's types and the battle level (balloon.rs).
//! - Other players' Chao: what theirs have from partners, the four moves they battle with and
//!   their battle levels come online (`Body::Partners`), and exporting Chao files writes
//!   `partners.txt` next to them, which is read from `chao/import`.
//! - Wild visitors: each garden has an area of the ROM's wild tables each day (`rotation`), whose
//!   creatures come by now and then and are absorbed like partners (`wild`).

// Off the game (Linux builds) only the tests use the garden side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

pub(crate) mod billboard;
mod card;
mod machine;
mod machine_model;
pub mod profile;
pub mod rotation;
mod scene;
pub mod store;
mod wild;

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
use store::{Learned, Moves, Partner, Store};

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

/// What other players' Chao have from their partners (`Body::Partners`), by Chao identity.
static OTHERS: Mutex<BTreeMap<u64, Learned>> = Mutex::new(BTreeMap::new());
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

/// The partner types of the Chao with `identity`, by the ROM's type names, for the status
/// balloon (`None` when it has none or the ROM isn't loaded).
pub fn partner_types(identity: u64) -> Option<Vec<String>> {
    let learned =
        with_store(|s| s.partners.get(&identity).copied()).filter(|l| l.partner.species != 0)?;
    let rom = loaded_rom()?;
    let names: Vec<String> = type_names(&rom, learned.partner.types)
        .iter()
        .map(|t| ascii(t))
        .collect();
    (!names.is_empty()).then_some(names)
}

/// What is known of a Chao: its partner, moves and battle level, and whether that level is
/// from our own file (its XP is known).
#[derive(Default)]
struct Known {
    partner: Option<Partner>,
    moves: Moves,
    level: Level,
    ours: bool,
}

/// What is known of the Chao with `identity`. Ours come from `partners.txt`, and a Chao in our
/// save (`own`) only from there, so other players' data never changes it; another player's
/// Chao from `partners.txt` (a copy we have too), then from what they sent, then from
/// `chao/import/partners.txt`.
fn record(identity: u64, own: bool) -> Known {
    let known = |(partner, moves, level): (Option<Partner>, Moves, Level), ours| Known {
        partner,
        moves,
        level,
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
    let other = lock(&OTHERS).get(&identity).copied();
    let level = lock(&OTHER_LEVELS).get(&identity).copied();
    if other.is_some() || level.is_some() {
        return Known {
            partner: other.map(|l| l.partner),
            moves: other.map_or_else(Moves::default, |l| Moves::from_learned(&l)),
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

/// The battle profile of the Chao with `identity`: its partner's types and ability, the four
/// moves it battles with and its battle level, or Normal type with basic moves without a
/// partner (`record` says where it comes from). `chosen` is given for a Chao in our save: the
/// moves it battles with, and only our own file counts for it. Without `save_partners`, what
/// partners from a GBA save gave is left out (`profile`). Loads the ROM if needed.
pub fn battle_profile(
    identity: u64,
    chosen: Option<&Moves>,
    save_partners: bool,
) -> Result<Profile, String> {
    let rom = rom()?;
    let k = record(identity, chosen.is_some());
    let moves = chosen.unwrap_or(&k.moves);
    Ok(profile::profile(
        &rom,
        k.partner.as_ref(),
        moves,
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

/// What friends' Chao files carry from partners, under the mod folder.
const IMPORTED_PARTNERS: &str = "chao/import/partners.txt";

/// What our Chao have from partners, the four moves they battle with and their battle levels,
/// to send to the others.
pub fn shared_partners() -> Vec<SharedPartner> {
    with_store(|s| shared_of(s))
}

/// Each Chao `store` has anything of: its partner (species 0, Normal and no ability without
/// one), its active moves and its battle level.
fn shared_of(store: &Store) -> Vec<SharedPartner> {
    store
        .identities()
        .into_iter()
        .map(|identity| {
            let partner = store.partners.get(&identity).map(|l| l.partner);
            SharedPartner {
                identity,
                species: partner.map_or(0, |p| p.species),
                types: partner.map_or([profile::NORMAL; 2], |p| p.types),
                ability: partner.map_or(0, |p| p.ability),
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
        others.insert(
            p.identity,
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
        );
    }
}

/// The `partners.txt` that goes with exported Chao files: for each of `identities` our store
/// has anything of, its partner (species 0 without one) with the four moves it battles with,
/// and its battle level; not its whole move list. `None` when there's nothing to write.
fn export_of(store: &Store, identities: &[u64]) -> Option<Store> {
    let mut out = Store::default();
    for &identity in identities {
        let Some((partner, moves, _)) = store.record(identity) else {
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
    wild::on_frame();
    machine::on_frame();
    card::on_frame();
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
    scene::is_partner_object(object) || wild::is_visitor(object)
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
    /// The link machine's list of our Chao, to choose the moves one battles with
    /// (`Page::chao_list`).
    Moves,
    /// The moves our Chao with this identity learned (`Page::chao_moves`).
    ChaoMoves(u64),
    /// Switch move `id` of our Chao `identity` on or off for battles (`toggle_move`).
    Toggle { identity: u64, id: u16 },
    /// Back to the list before.
    Back,
}

/// The GBA link list: the save's creatures and the mod's kept ones, one row each; without a save,
/// the first-partner gift until it's taken. The link machine's move lists are pages too.
pub struct Page {
    pub title: String,
    pub rows: Vec<(String, Action)>,
    pub row: usize,
    pub scroll: usize,
    /// What the keys do here (the link machine's own when empty).
    pub hints: Vec<String>,
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
            title: "Link machine".into(),
            rows,
            row: 0,
            scroll: 0,
            hints: Vec::new(),
        }
    }

    /// Our Chao (hatched, in any garden), to choose the moves one battles with.
    pub fn chao_list() -> Self {
        let mut rows: Vec<(String, Action)> = own_chao()
            .into_iter()
            .map(|(identity, name)| {
                let m = moves_of(identity);
                (chao_row(&name, &m), Action::ChaoMoves(identity))
            })
            .collect();
        if rows.is_empty() {
            rows.push(("You have no Chao to choose moves for".into(), Action::None));
        }
        Self {
            title: "Moves: which Chao?".into(),
            rows,
            row: 0,
            scroll: 0,
            hints: vec![
                "A / Enter: its moves. Each Chao remembers every move".into(),
                "it learned and battles with up to four of them.".into(),
                "Up/Down (PgUp/PgDn) pick. B / Esc / Backspace back.".into(),
            ],
        }
    }

    /// Every move our Chao `identity` learned, each switched on or off for battles.
    pub fn chao_moves(identity: u64) -> Self {
        let name = own_chao()
            .into_iter()
            .find(|(id, _)| *id == identity)
            .map_or_else(|| "Chao".to_string(), |(_, name)| name);
        let rom = loaded_rom();
        let describe = |id: u16| match &rom {
            Some(rom) => move_label(rom, id),
            None => format!("Move #{id}"),
        };
        Self {
            title: format!("{name}'s moves (4 at most)"),
            rows: move_rows(identity, &moves_of(identity), describe),
            row: 0,
            scroll: 0,
            hints: vec![
                "A / Enter: use or drop a move (kept at once). [x]: it".into(),
                "battles with it. GBA save: only a save's partner taught it,".into(),
                "so live battles without GBA save partners leave it out.".into(),
                "Up/Down (PgUp/PgDn) pick. B / Esc / Backspace back.".into(),
            ],
        }
    }
}

/// Our hatched Chao in the save, any garden: (identity, name).
fn own_chao() -> Vec<(u64, String)> {
    (0..crate::game::GARDEN_SLOTS)
        .filter_map(crate::game::read_chao_slot)
        .filter(|c| c.chao_type() > crate::chao::TYPE_EGG && c.validate().is_ok())
        .map(|c| (c.identity(), crate::karate::chao_name(&c)))
        .collect()
}

/// A Chao's row in the link machine's list: its name and how many moves it uses and knows.
fn chao_row(name: &str, m: &Moves) -> String {
    let on = m.active.iter().filter(|&&id| id != 0).count();
    format!("{name:<7}  {on} of {} moves", m.known.len())
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

/// Switches move `id` of our Chao `identity` on or off for battles and keeps it (`set_moves`);
/// whether it's on now.
pub fn toggle_move(identity: u64, id: u16) -> Result<bool, store::MoveRefused> {
    let mut m = moves_of(identity);
    let on = m.toggle(id)?;
    set_moves(identity, m);
    Ok(on)
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
        let shared = shared_of(&store());
        assert_eq!(shared.len(), 3);
        assert_eq!(
            (shared[0].species, shared[0].moves, shared[0].level),
            (25, [84, 45, 86, 98], 40)
        );
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
        assert_eq!(others[&theirs[0].identity].moves, [84, 45, 86, 98]);
        assert!(others.contains_key(&theirs[1].identity));
        assert!(!others.contains_key(&theirs[2].identity));
        drop(others);
        assert_eq!(lock(&OTHER_LEVELS)[&theirs[2].identity].level, 9);
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

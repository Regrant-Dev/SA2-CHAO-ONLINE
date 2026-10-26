//! Generic creature link: a private local catalog supplies names, gameplay data and 3D
//! actors. The starter partner and wild visitors teach Chao types, abilities and moves.
//! Progress lives in the mod's versioned partner store; native Chao saves stay independent.

// Off the game (Linux builds) only the tests use the garden side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

pub(crate) mod billboard;
mod card;
pub(crate) mod content_paths;
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::creature::rom::Rom;

use crate::battle::levels::Level;
use crate::log;
use crate::net::protocol::SharedPartner;
pub use profile::Profile;
use store::{Learned, Moves, Partner, Store, Traits};

/// Local source/cache content and partner progress, separate from native Chao saves.
const FOLDER: &str = "creature";
const STORE_FILE: &str = "partners.txt";

/// Local source image and private cache directory. Source assets never enter network sharing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paths {
    pub source: String,
    pub cache: String,
}

struct Link {
    paths: Paths,
    /// The catalog and the path it came from.
    rom: Option<(PathBuf, Arc<Rom>)>,
    store: Option<Store>,
    /// `partners.txt` was there but couldn't be read: it's left as it is (never written over).
    store_unreadable: bool,
    importing: Option<ImportJob>,
    /// Where the player last asked for the catalog (`rom_from`), for the message once an
    /// import is done.
    asked_from: Option<&'static str>,
    /// The store's active moves were checked against the catalog (`drop_unusable_moves`).
    moves_checked: bool,
}

struct ImportJob {
    token: u64,
    source: PathBuf,
    cache: PathBuf,
    message: String,
    running: bool,
    /// A page the player opened (`rom_from`) has shown the finished job's message.
    shown: bool,
    /// The game file's size and time when the job started.
    stamp: Option<(u64, std::time::SystemTime)>,
}
fn file_stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}
fn finish_import(job: &mut Option<ImportJob>, token: u64, message: String) {
    if let Some(job) = job.as_mut().filter(|job| job.token == token) {
        job.running = false;
        job.message = message;
    }
}
/// A finished job is dropped (so the next ask imports again) once its message has been shown
/// on a page, or when the game file has changed since: the first open after a failure says
/// why, the next one (or the first one after replacing the file) tries again.
fn retry_finished_import(job: &mut Option<ImportJob>) {
    if job
        .as_ref()
        .is_some_and(|job| !job.running && (job.shown || file_stamp(&job.source) != job.stamp))
    {
        *job = None;
    }
}
/// The page about to show `message` from a finished job shows it.
fn show_finished_import(job: &mut Option<ImportJob>, message: &str) {
    if let Some(job) = job
        .as_mut()
        .filter(|job| !job.running && job.message == message)
    {
        job.shown = true;
    }
}
static IMPORT_TOKEN: AtomicU64 = AtomicU64::new(1);

static LINK: Mutex<Link> = Mutex::new(Link {
    paths: Paths {
        source: String::new(),
        cache: String::new(),
    },
    rom: None,
    store: None,
    store_unreadable: false,
    importing: None,
    asked_from: None,
    moves_checked: false,
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

/// `set` as configured, else the first file with `extension` in `<mod>/creature/`. A relative path
/// is from the mod folder.
fn resolve(set: &str, extension: &str) -> Option<PathBuf> {
    let set = set.trim().trim_matches('"');
    if !set.is_empty() {
        return content_paths::resolve_configured(Path::new(crate::online::mod_path()), set).ok();
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
    with(|l| {
        if l.paths != paths {
            l.rom = None;
            l.importing = None;
        }
        l.paths = paths;
    });
}

/// What to do when there's no game file to prepare the catalog from.
const CHOOSE_SOURCE: &str = "Choose your game file in SA Mod Manager > Source game image, or put it in the mod's creature folder.";

/// The bounded runtime catalog, prepared by a separate helper when needed.
pub fn rom() -> Result<Arc<Rom>, String> {
    let paths = with(|l| l.paths.clone());
    let path = if paths.cache.trim().is_empty() {
        folder()
            .join("cache")
            .join(crate::creature::catalog::PROFILE)
    } else {
        content_paths::resolve_configured(Path::new(crate::online::mod_path()), &paths.cache)?
    };
    if let Some(rom) = with(|l| {
        l.rom
            .as_ref()
            .filter(|(p, _)| *p == path)
            .map(|(_, r)| r.clone())
    }) {
        return Ok(rom);
    }
    let rom = match Rom::open_ready(&path) {
        Ok(rom) => Arc::new(rom),
        Err(error) => {
            log!(
                "creature link: no usable cache at {}: {error}",
                path.display()
            );
            let Some(source) = resolve(&paths.source, "3ds").filter(|s| s.is_file()) else {
                return Err(CHOOSE_SOURCE.into());
            };
            return Err(prepare_source(source, path));
        }
    };
    crate::creature::assets::set_root(Some(rom.asset_root()));
    log!(
        "creature link: local cache {} ({})",
        path.display(),
        rom.fingerprint()
    );
    with(|l| l.rom = Some((path, rom.clone())));
    check_moves(&rom);
    Ok(rom)
}

/// `rom`, asked for from `place` ("the link machine"): when it has to prepare the catalog
/// first, the message once that's done says to open `place` again.
pub fn rom_from(place: &'static str) -> Result<Arc<Rom>, String> {
    with(|l| l.asked_from = Some(place));
    rom().inspect_err(|message| with(|l| show_finished_import(&mut l.importing, message)))
}

/// Once a session, with the catalog loaded: active moves creature battles can't use are
/// switched off (they stay known), with a notice when there were any.
fn check_moves(rom: &Rom) {
    let dropped = with(|l| {
        if l.moves_checked {
            return 0;
        }
        l.moves_checked = true;
        loaded_store(l).drop_unusable_moves(|id| rom.move_info(id).is_some())
    });
    if dropped > 0 {
        write_store();
        log!("creature link: switched off {dropped} active moves battles can't use");
        crate::hud::toast(
            "Some of your Chao's moves can't be used in battles any more, so they were switched off",
        );
    }
}

/// The catalog if it's loaded already (never reads the file).
fn loaded_rom() -> Option<Arc<Rom>> {
    with(|l| l.rom.as_ref().map(|(_, r)| r.clone()))
}

pub(crate) fn visual_size(species: u16, form: u8) -> f32 {
    if form == 0 {
        return 1.;
    }
    loaded_rom().map_or(1., |rom| {
        rom.size_ratio(crate::creature::data::SpeciesKey { species, form })
    })
}

fn prepare_source(source: PathBuf, cache: PathBuf) -> String {
    let previous = with(|l| {
        l.importing
            .as_ref()
            .filter(|job| job.source == source && job.cache == cache)
            .map(|job| job.message.clone())
    });
    if let Some(status) = previous {
        return status;
    }
    let helper = Path::new(crate::online::mod_path()).join("sa2co-import.exe");
    if !helper.is_file() {
        return "The creature link's importer is missing. Reinstall the mod.".into();
    }
    let status =
        "Preparing the creature link from your game file. This takes a few minutes.".to_string();
    let token = IMPORT_TOKEN.fetch_add(1, Ordering::Relaxed);
    with(|l| {
        l.importing = Some(ImportJob {
            token,
            source: source.clone(),
            cache: cache.clone(),
            message: status.clone(),
            running: true,
            shown: false,
            stamp: file_stamp(&source),
        })
    });
    let spawned = std::thread::Builder::new()
        .name("local-creature-import".into())
        .spawn(move || {
            let result = (|| -> Result<(), String> {
                let mut command = std::process::Command::new(helper);
                command
                    .arg("build")
                    .arg(&source)
                    .arg("--output")
                    .arg(&cache);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS: no console, and the game
                    // keeps running smoothly.
                    command.creation_flags(0x0800_0000 | 0x0000_4000);
                }
                let child = command
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| {
                        log!("creature link: can't start the importer: {e}");
                        "The creature link's importer couldn't start. Reinstall the mod."
                            .to_string()
                    })?;
                #[cfg(windows)]
                tie_to_game(&child);
                let output = child.wait_with_output().map_err(|e| {
                    log!("creature link: lost the importer: {e}");
                    "The creature link couldn't be prepared. Try again.".to_string()
                })?;
                if !output.status.success() {
                    let error = String::from_utf8_lossy(&output.stderr);
                    log!("local importer failed: {error}");
                    return Err(import_message(&error));
                }
                Rom::open_ready(&cache).map(|_| ()).map_err(|e| {
                    log!("creature link: the new cache can't be opened: {e}");
                    "The creature link couldn't be prepared. Try again.".to_string()
                })
            })();
            let message = match result {
                Ok(()) => {
                    let message = match with(|l| l.asked_from) {
                        Some(place) => format!("Creature link is ready. Open {place} again."),
                        None => "Creature link is ready".to_string(),
                    };
                    crate::hud::toast(message.clone());
                    message
                }
                Err(error) => {
                    crate::hud::toast(error.clone());
                    error
                }
            };
            with(|l| finish_import(&mut l.importing, token, message));
        });
    if let Err(error) = spawned {
        log!("creature link: can't start the import worker: {error}");
        let message = "The creature link couldn't be prepared. Try again.".to_string();
        with(|l| {
            finish_import(&mut l.importing, token, message.clone());
        });
        return message;
    }
    status
}

/// Puts the importer in a job that ends it when the game's handle to the job closes, so quitting
/// while the creature link is prepared stops the import too (the next import clears what it
/// left). The job is made once and kept open for the game's life.
#[cfg(windows)]
fn tie_to_game(child: &std::process::Child) {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(
            job: *mut c_void,
            class: u32,
            info: *const c_void,
            size: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    }
    /// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`.
    #[repr(C)]
    #[derive(Default)]
    struct Limits {
        per_process_time: i64,
        per_job_time: i64,
        flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_processes: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
        io: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }
    const EXTENDED_LIMITS: u32 = 9;
    const KILL_ON_JOB_CLOSE: u32 = 0x2000;

    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        let limits = Limits {
            flags: KILL_ON_JOB_CLOSE,
            ..Limits::default()
        };
        // SAFETY: a new unnamed job and its limits, from a struct of the size given.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let set = SetInformationJobObject(
                job,
                EXTENDED_LIMITS,
                (&limits as *const Limits).cast(),
                size_of::<Limits>() as u32,
            );
            (set != 0).then_some(job as usize)
        }
    });
    let Some(job) = *job else {
        log!("creature link: the importer isn't tied to the game");
        return;
    };
    // SAFETY: the job made above and the child's own process handle.
    if unsafe { AssignProcessToJobObject(job as *mut c_void, child.as_raw_handle()) } == 0 {
        log!("creature link: the importer isn't tied to the game");
    }
}

/// A deliberate menu action retries failed/completed jobs once their message has been shown;
/// background garden polling never starts an endless sequence of failed imports at the same
/// path.
pub fn retry_import() {
    with(|link| retry_finished_import(&mut link.importing));
}

/// What the player sees for the importer's error output: its first `error[<code>]: ...` line.
fn import_message(stderr: &str) -> String {
    use crate::creature::import_error::{ErrorKind, parse};
    const UNREADABLE: &str = "Your game file couldn't be read. Use a decrypted copy of your own cartridge, without the update.";
    const FAILED: &str = "The creature link couldn't be prepared. Try again; if it keeps failing, include sa2-chao-online.log in a report.";
    match parse(stderr) {
        Some(ErrorKind::Encrypted) => {
            "This file is encrypted. Use a decrypted copy of your own cartridge."
        }
        Some(ErrorKind::WrongTitle) => "This isn't the supported game.",
        Some(ErrorKind::Update | ErrorKind::Format) => {
            "Use the original cartridge dump, without the update."
        }
        Some(ErrorKind::Missing) => CHOOSE_SOURCE,
        Some(ErrorKind::Changed) => "Your game file changed while it was being read. Try again.",
        Some(ErrorKind::Space) => {
            "There isn't enough free disk space to prepare the creature link."
        }
        Some(ErrorKind::Cache) => {
            "The creature link's cache folder can't be used. Check Private asset cache in SA Mod Manager."
        }
        Some(ErrorKind::Invalid) => UNREADABLE,
        // A conversion or write after the file read fine, the mod's own command line, or a
        // crash (which leaves no error line).
        Some(ErrorKind::Failed | ErrorKind::Usage) | None => FAILED,
    }
    .into()
}

/// The text of `path` (`None` when there's no file yet). A file that can't be read, or isn't
/// text, is an error.
fn read_text(path: &Path) -> std::io::Result<Option<String>> {
    use std::io::Read;
    const LIMIT: usize = 8 << 20;
    let read = (|| -> std::io::Result<Vec<u8>> {
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > LIMIT as u64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Partner/progress text exceeds its regular-file bound",
            ));
        }
        let mut bytes = Vec::new();
        file.take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > LIMIT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Partner/progress text changed beyond its size bound",
            ));
        }
        Ok(bytes)
    })();
    match read {
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
                log!("creature link: can't read {STORE_FILE} ({e}); it's left as it is");
                l.store_unreadable = true;
                String::new()
            }
        };
        let mut s = Store::parse(&text).unwrap_or_else(|e| {
            log!("creature link: {STORE_FILE} is {e}; it's left as it is");
            l.store_unreadable = true;
            Store::default()
        });
        if l.store_unreadable {
            crate::hud::toast(format!(
                "Creature link: {STORE_FILE} in the creature folder can't be read, so new \
                 partners aren't saved. Move it out of that folder to start fresh."
            ));
        }
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
        log!("creature link: {STORE_FILE} couldn't be read, so it isn't written");
        return;
    };
    if let Err(e) = crate::battle::ladder::write(&folder().join(STORE_FILE), &text) {
        log!("creature link: can't write {STORE_FILE}: {e}");
    }
}

/// Where a creature on the creature link list comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The mod's own list (`store`).
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

/// A creature's line in the creature link list: where it is, its name (the catalog's species
/// name), level and type names.
pub fn label(rom: &Rom, c: &Candidate) -> String {
    let place = match c.source {
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
        return Err(if machine::enabled() {
            "Creature link works in your own garden; use the shared link machine online"
        } else {
            "Bring partners in your own garden, or turn on Creature link machine in the gardens to use the shared machine"
        }
        .into());
    }
    let rom = rom()?;
    let species = rom
        .species(mon.species)
        .ok_or("This one isn't in your game's data")?;
    let garden = crate::game::current_chao_area() as u8;
    let partner = Partner {
        form: 0,
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
        "creature link: species {} (level {}) came to garden {garden}",
        mon.species,
        partner.level
    );
    Ok(ascii(&name))
}

/// The ability in `slot` of a species' two, or its first when it has no second.
fn ability_of(species: &crate::creature::rom::Species, slot: u8) -> u8 {
    match species.abilities[usize::from(slot.min(2))] {
        0 => species.abilities[0],
        a => a,
    }
}

/// The Chao with `identity` has absorbed a partner (known without the catalog).
pub fn has_partner(identity: u64) -> bool {
    with_store(|s| {
        s.partners
            .get(&identity)
            .is_some_and(|l| l.partner.species != 0)
    })
}

/// The types the Chao with `identity` battles as, by the catalog's type names, for the status
/// balloon (`None` when it has no partner or the catalog isn't loaded).
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
    // Abilities not recorded come from the catalog.
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
        .and_then(|text| Store::parse(&text).ok()?.record(identity))
        .map_or_else(Known::default, |r| known(r, false))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The battle profile of the Chao with `identity`: the types, ability and four moves it
/// battles with and its battle level, or Normal type with basic moves without a partner
/// (`record` says where it comes from). `moves` and `traits` are given for a Chao in our save
/// (what it battles with), and only our own file counts for it. Loads the catalog if needed.
pub fn battle_profile(
    identity: u64,
    moves: Option<&Moves>,
    traits: Option<&Traits>,
) -> Result<Profile, String> {
    let rom = rom()?;
    let k = record(identity, moves.is_some() || traits.is_some());
    Ok(profile::profile(
        &rom,
        k.partner.as_ref(),
        traits.unwrap_or(&k.traits),
        moves.unwrap_or(&k.moves),
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

/// Keeps `moves` as our Chao's with `identity` (the four it battles with chosen, or a milestone move
/// taught) and tells the others.
pub fn set_moves(identity: u64, moves: Moves) {
    with_store(|s| s.set_moves(identity, moves));
    write_store();
    crate::online::resend_partners();
}

/// Every type and ability our Chao with `identity` got, and the ones it battles with (an ability
/// not recorded from the catalog, once it's loaded).
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

/// Each Chao `store` has anything of: its partner's species (0 without one), the types and
/// ability it battles with (`profile::traits_ruled`: Normal without types, 0 for no ability or
/// none battles can use; one not recorded from `rom`), its active moves and its battle level.
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
            let (types, ability) = profile::traits_ruled(&traits);
            SharedPartner {
                form: partner.map_or(0, |p| p.form),
                identity,
                species: partner.map_or(0, |p| p.species),
                types,
                ability,
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
                        form: p.form,
                        species: p.species,
                        types: p.types,
                        moves: p.moves,
                        level: 0,
                        ability: p.ability,
                    },
                    moves: p.moves,
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
            form: 0,
            species: 0,
            types: [profile::NORMAL; 2],
            moves: [0; 4],
            level: 0,
            ability: 0,
        });
        out.partners.insert(
            identity,
            Learned {
                partner,
                moves: moves.active,
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
        log!("creature link: can't write {}: {e}", path.display());
    }
}

/// "<name> learned <move>!" for the moves new to a Chao (the catalog's names), as one message.
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
        "creature link: a Chao absorbed species {} (types {:?}, ability {}, moves {:?})",
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

/// A note in the creature folder until a game file is there or chosen.
const FOLDER_NOTE: &str = "PUT YOUR GAME FILE HERE.txt";
const FOLDER_NOTE_TEXT: &str = "Put a decrypted copy of your own cartridge (.3ds, without the update) in this folder, or choose it in SA Mod Manager > Source game image. When the game starts with the file in place (or when you open the creature link), the mod prepares the creature link from it in the background, which takes a few minutes. Your file stays on your computer. how-to-play.md says which game it is.\r\n";

fn make_folder() {
    let dir = folder();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log!("creature link: cannot create {}: {e}", dir.display());
        return;
    }
    if with(|l| !l.paths.source.trim().is_empty()) {
        return;
    }
    let present = std::fs::read_dir(&dir).ok().is_some_and(|entries| {
        entries
            .flatten()
            .any(|e| has_extension(&e.file_name().to_string_lossy(), "3ds"))
    });
    if present {
        return;
    }
    let note = dir.join(FOLDER_NOTE);
    if !note.exists()
        && let Err(e) = std::fs::write(&note, FOLDER_NOTE_TEXT)
    {
        log!("creature link: cannot write source setup note: {e}");
    }
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
    // The catalog in the background, so the status balloon has the type names from the start.
    std::thread::spawn(|| {
        if let Err(e) = rom() {
            log!("creature link: {e}");
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
        "creature link: a Chao absorbed a wild species {} (types {:?}, ability {}, moves {:?})",
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
        .ok_or("This one isn't in your game's data")?;
    let partner = Partner {
        form: 0,
        species: c.species,
        types: species.types,
        moves: c.moves,
        level: c.level.clamp(1, 100),
        ability: ability_of(&species, c.ability_slot),
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
/// the F9 creature link page.
pub fn set_link_machine(on: bool) {
    machine::set_enabled(on);
}

/// The gardens have their link machine (the F9 creature link page only points to it).
pub fn link_machine() -> bool {
    machine::enabled()
}

/// What picking a row of the creature link list (the link machine's or the F9 page) does.
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

/// The creature link list: the save's creatures and the mod's kept ones, one row each; without a save,
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
        format!("Type to search. Esc clears / {back}; B {back}."),
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

    /// Lists the starter partner taken, or the starter gift to choose from. `from`: where it's
    /// opened (`rom_from`).
    pub fn open(from: &'static str) -> Self {
        retry_import();
        let mut page = Self::new("Creature link".into(), Vec::new(), Vec::new());
        match rom_from(from) {
            Err(error) => page.push(error, Action::None, None),
            Ok(source) => {
                let name = |species: u16| source.species_name(species).map(|n| ascii(&n));
                let (kept, gift) = with_store(|store| (store.kept.clone(), store.gift));
                for (key, species, level, moves) in kept {
                    let candidate = Candidate {
                        source: Source::Kept,
                        key,
                        species,
                        level,
                        moves,
                        ability_slot: 0,
                    };
                    page.push(
                        label(&source, &candidate),
                        Action::Bring(candidate),
                        name(species),
                    );
                }
                if gift.is_none()
                    && let Some(starters) = source.starters()
                {
                    page.push(
                        "Choose one starter partner (kept once):".into(),
                        Action::None,
                        None,
                    );
                    for species in starters {
                        let candidate = Candidate {
                            source: Source::Kept,
                            key: 0,
                            species,
                            level: GIFT_LEVEL,
                            moves: [0; 4],
                            ability_slot: 0,
                        };
                        let text = label(&source, &candidate).replacen("Kept", "Starter", 1);
                        page.push(text, Action::Gift(species), name(species));
                    }
                }
                if page.rows.is_empty() {
                    page.push(
                        "No starter partner in this creature link".into(),
                        Action::None,
                        None,
                    );
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
                "battles with it.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        page.names = moves
            .known
            .iter()
            .map(|&id| {
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
                "battles as it.".into(),
            ]
            .into_iter()
            .chain(nav_hints("back"))
            .collect(),
        );
        page.names = traits
            .types
            .iter()
            .map(|&t| Some(type_name(rom.as_deref(), t)))
            .collect();
        page
    }

    /// Every ability our Chao `identity` got, one (or none) chosen for battles, each with the
    /// catalog's description.
    pub fn chao_abilities(identity: u64) -> Self {
        let rom = loaded_rom();
        let traits = traits_of(identity);
        let mut page = Self::new(
            format!("{}'s ability", chao_name(identity)),
            ability_rows(identity, &traits, |a| ability_name(rom.as_deref(), a)),
            ["A / Enter: battle with it (kept at once). (o): its ability.".into()]
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
                    .map(|&a| Some(ability_name(rom.as_deref(), a))),
            )
            .collect();
        page.notes = std::iter::once(Some("It battles without an ability.".to_string()))
            .chain(traits.abilities.iter().map(|&a| {
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

/// The catalog's wild areas (read once in the game, `wild::areas`).
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
    /// catalog's name), how long it lasts, now also the next window's area to see, then every
    /// species that can come with its level range, where from and its odds (`wild::who_comes`,
    /// what the picking uses), its types and abilities in its note.
    pub fn who_comes(
        rom: &Rom,
        areas: &[rotation::Area],
        garden: u8,
        next: bool,
        unix: u64,
    ) -> Self {
        let slot = rotation::slot_of(unix).wrapping_add(u32::from(next));
        let left = minutes(rotation::minutes_left(unix));
        let mut page = Self::new(
            format!(
                "Who can come to the {}{}",
                garden_name(garden),
                if next { " next" } else { "" }
            ),
            Vec::new(),
            [
                "%: of the visitors to the grass, or of those to the pond.".into(),
                "Kinds that just came come less often.".into(),
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
            page.push("No wild area for this garden".into(), Action::None, None);
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
            if let Some(after) = rotation::area_at(areas, garden, slot.wrapping_add(1)) {
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

    /// `who_comes` from the player's catalog by the clock now.
    pub fn who_comes_now(garden: u8, next: bool) -> Self {
        let rom = match rom_from("the link machine") {
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
                    "The wild areas can't be read from your game's data".into(),
                    Action::None,
                    None,
                );
                page
            }
        }
    }
}

/// A species' types and abilities from the catalog, for the line under the list.
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

/// The catalog's name of type `t` ("Type #t" without the catalog).
fn type_name(rom: Option<&Rom>, t: u8) -> String {
    rom.and_then(|r| r.type_name(t))
        .map_or_else(|| format!("Type #{t}"), |n| ascii(&n))
}

/// The catalog's name of ability `a` ("Ability #a" without the catalog).
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

/// A move's name, type and power from the catalog ("Name  Type 40"); "(unavailable)" for one
/// creature battles can't use.
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
        None => format!("{name} (unavailable)"),
    }
}

/// The rows of a Chao's moves (`describe` names one): "[x]" on the ones it battles with, then
/// "Done".
fn move_rows(identity: u64, m: &Moves, describe: impl Fn(u16) -> String) -> Vec<(String, Action)> {
    let mut rows: Vec<(String, Action)> = m
        .known
        .iter()
        .map(|&id| {
            let on = if m.is_active(id) { "[x]" } else { "[ ]" };
            (
                format!("{on} {}", describe(id)),
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

/// The rows of a Chao's types (`name` names one): "[x]" on the ones it battles as, then "Done".
fn type_rows(identity: u64, t: &Traits, name: impl Fn(u8) -> String) -> Vec<(String, Action)> {
    let mut rows: Vec<(String, Action)> = t
        .types
        .iter()
        .map(|&id| {
            let on = if t.is_type_active(id) { "[x]" } else { "[ ]" };
            (
                format!("{on} {}", name(id)),
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
/// battles with and "(unavailable)" on ones creature battles can't use, then "Done".
fn ability_rows(identity: u64, t: &Traits, name: impl Fn(u8) -> String) -> Vec<(String, Action)> {
    let mark = |on: bool| if on { "(o)" } else { "( )" };
    let mut rows = vec![(
        format!("{} None", mark(t.ability == 0)),
        Action::Ability { identity, id: 0 },
    )];
    rows.extend(t.abilities.iter().map(|&id| {
        let availability = if crate::battle::source_rules::ability_supported(id) {
            ""
        } else {
            " (unavailable)"
        };
        (
            format!("{} {}{availability}", mark(t.ability == id), name(id)),
            Action::Ability { identity, id },
        )
    }));
    rows.push(("Done".into(), Action::Back));
    rows
}

/// Switches move `id` of our Chao `identity` on or off for battles and keeps it (`set_moves`);
/// whether it's on now. One creature battles can't use only switches off; nothing changes while
/// the catalog isn't loaded.
pub fn toggle_move(identity: u64, id: u16) -> Result<bool, store::MoveRefused> {
    let rom = rom().map_err(|_| store::MoveRefused::NotReady)?;
    let mut m = moves_of(identity);
    if rom.move_info(id).is_none() {
        if !m.is_active(id) {
            return Err(store::MoveRefused::Unsupported);
        }
        // Even its last one: empty slots take basic moves.
        m.drop_unusable(|other| other != id);
        set_moves(identity, m);
        return Ok(false);
    }
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
    if !crate::battle::source_rules::ability_supported(id) {
        return Err(store::TraitRefused::Unsupported);
    }
    let mut t = traits_of(identity);
    t.choose_ability(id)?;
    set_traits(identity, t);
    Ok(())
}

/// Takes the gift `species` (one of the catalog's first partners, once only). The message to
/// show.
pub fn take_gift(species: u16) -> String {
    match take_gift_candidate(species) {
        Ok(c) => {
            let name = loaded_rom()
                .and_then(|r| r.species_name(c.species))
                .map(|n| ascii(&n))
                .unwrap_or_default();
            format!("{name} is yours: F9 > Creature link brings it to a garden")
        }
        Err(e) => e,
    }
}

/// Takes the gift `species` (once only): it's kept by the mod from now on. The kept one, to
/// bring into a garden.
pub fn take_gift_candidate(species: u16) -> Result<Candidate, String> {
    let rom = rom()?;
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
    log!("creature link: took gift species {species}");
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
    fn failed_imports_say_why_once_then_retry_but_active_or_newer_jobs_are_preserved() {
        use super::{ImportJob, finish_import, retry_finished_import, show_finished_import};
        let mut job = Some(ImportJob {
            token: 2,
            source: "source.3ds".into(),
            cache: "private".into(),
            message: "preparing".into(),
            running: true,
            shown: false,
            stamp: None,
        });
        retry_finished_import(&mut job);
        assert!(job.as_ref().unwrap().running);
        show_finished_import(&mut job, "preparing");
        assert!(!job.as_ref().unwrap().shown);
        finish_import(&mut job, 1, "stale worker failed".into());
        assert_eq!(job.as_ref().unwrap().message, "preparing");
        finish_import(&mut job, 2, "encrypted".into());
        assert!(!job.as_ref().unwrap().running);
        // The first open after the failure keeps the job, so its page says why.
        retry_finished_import(&mut job);
        assert_eq!(job.as_ref().unwrap().message, "encrypted");
        show_finished_import(&mut job, "another message");
        assert!(!job.as_ref().unwrap().shown);
        show_finished_import(&mut job, "encrypted");
        // The next one tries again.
        retry_finished_import(&mut job);
        assert!(job.is_none());
    }
    #[test]
    fn importer_errors_become_plain_messages() {
        use super::import_message;
        use crate::creature::import_error::{ErrorKind, line};
        // Each kind as the importer prints it, after its progress on another stream.
        let message = |kind| import_message(&format!("\n{}\n", line(kind, "details")));
        let failed = import_message("thread 'main' panicked");
        assert!(failed.contains("couldn't be prepared") && failed.contains(".log"));
        for kind in ErrorKind::ALL {
            let text = message(kind);
            let expect = match kind {
                ErrorKind::Missing => super::CHOOSE_SOURCE,
                ErrorKind::Format | ErrorKind::Update => "without the update",
                ErrorKind::Encrypted => "encrypted",
                ErrorKind::WrongTitle => "isn't the supported game",
                ErrorKind::Changed => "changed while it was being read",
                ErrorKind::Space => "free disk space",
                ErrorKind::Cache => "cache folder",
                ErrorKind::Invalid => "couldn't be read",
                ErrorKind::Failed | ErrorKind::Usage => &failed,
            };
            assert!(text.contains(expect), "{kind:?}: {text}");
            // Only the importer's own details never reach the player.
            assert!(!text.contains("details"), "{kind:?}");
        }
        assert_eq!(import_message("error[unknown]: x"), failed);
    }

    #[test]
    fn only_a_real_source_extension_counts_as_an_image() {
        use super::has_extension;
        assert!(has_extension("source.3ds", "3ds"));
        assert!(has_extension("Source.3DS", "3ds"));
        assert!(!has_extension("source.3ds.zip", "3ds"));
        assert!(!has_extension("source.3ds.7z", "3ds"));
        assert!(!has_extension("creature", "creature"));
    }

    use super::*;
    use crate::creature::rom::Species;

    fn store() -> Store {
        let mut s = Store::default();
        let partner = Partner {
            form: 0,
            species: 25,
            types: [13, 13],
            moves: [84, 45, 86, 98],
            level: 12,
            ability: 9,
        };
        s.learn(1, partner);
        s.learn(
            1,
            Partner {
                moves: [33, 0, 0, 0],
                ..partner
            },
        );
        // Milestone moves only, and a level only.
        let mut taught = Moves::default();
        taught.teach_milestone(15, 0);
        s.set_moves(2, taught);
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
            form: 0,
            species: 30,
            types: [2, 2],
            moves: [0; 4],
            level: 5,
            ability: 20,
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
    fn a_chao_with_an_ability_battles_cant_use_still_reaches_friends() {
        use crate::net::protocol::{Body, Message};
        let Some(unsupported) = (1..crate::creature::rom::ABILITY_COUNT)
            .find(|&a| !crate::battle::source_rules::ability_supported(a))
        else {
            return;
        };
        let partner = Partner {
            form: 0,
            species: 25,
            types: [13, 13],
            moves: [84, 0, 0, 0],
            level: 12,
            ability: unsupported,
        };
        let mut s = Store::default();
        s.learn(1, partner);
        assert_eq!(s.traits_of(1).ability, unsupported);
        let sent = |s: &Store| {
            let list = shared_of(s, None);
            let message = Message::Game {
                from: 1,
                body: Body::Partners { list: list.clone() },
            };
            assert_eq!(Message::decode(&message.encode()).unwrap(), message);
            list[0].ability
        };
        // Sent without it, and the message still reads.
        assert_eq!(sent(&s), 0);
        // With another one it can use, that one.
        s.learn(
            1,
            Partner {
                ability: 9,
                ..partner
            },
        );
        assert_eq!(s.traits_of(1).ability, unsupported);
        assert_eq!(sent(&s), 9);
    }

    #[test]
    fn our_chao_take_nothing_from_other_players() {
        with(|l| {
            l.store.get_or_insert_with(Store::default);
        });
        let id = 0xBEEF_0000_0000_0001;
        others_learned(&[SharedPartner {
            form: 0,
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
        let dir = std::env::temp_dir().join(format!("sa2co-creature-link-{}", std::process::id()));
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
        let back = Store::parse(&export.to_text()).unwrap();
        // Not the whole list: a friend's copy knows the four it battles with.
        let m = back.moves_of(1);
        assert_eq!(m.active, [84, 45, 86, 98]);
        assert!(!m.knows(33));
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
        m.learn([10, 20, 0, 0]);
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
        for (types, ability) in [([3, 3], 9), ([5, 6], 4)] {
            t.learn(&Partner {
                form: 0,
                species: 1,
                types,
                moves: [0; 4],
                level: 5,
                ability,
            });
        }
        t.toggle_type(6).unwrap();
        let rows = choice_rows(7, &m, &t, |id| format!("T{id}"), |id| format!("A{id}"));
        assert_eq!(rows[1].0, "Type: T3/T6");
        assert_eq!(rows[2].0, "Ability: A9");
        let rows = type_rows(7, &t, |id| format!("T{id}"));
        let texts: Vec<&str> = rows.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(texts, ["[x] T3", "[ ] T5", "[x] T6", "Done"]);
        assert_eq!(rows[1].1, Action::ToggleType { identity: 7, id: 5 });
        let rows = ability_rows(7, &t, |id| format!("A{id}"));
        let texts: Vec<&str> = rows.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(texts, ["( ) None", "(o) A9", "( ) A4", "Done"]);
        assert_eq!(rows[0].1, Action::Ability { identity: 7, id: 0 });
        assert_eq!(rows[2].1, Action::Ability { identity: 7, id: 4 });
    }

    #[test]
    fn the_machine_lists_a_chaos_moves_to_switch() {
        let mut m = Moves::default();
        m.learn([10, 20, 0, 0]);
        m.teach_milestone(30, 0);
        m.learn([40, 0, 0, 0]);
        assert_eq!(chao_row("Pip", &m), "Pip      4 of 4 moves");
        let rows = move_rows(7, &m, |id| format!("M{id}"));
        let texts: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["[x] M10", "[x] M20", "[x] M30", "[x] M40", "Done"]);
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
            abilities: [5, 9, 0],
            exp_yield: 0,
        };
        assert_eq!((ability_of(&s, 0), ability_of(&s, 1)), (5, 9));
        s.abilities[1] = 0;
        assert_eq!(ability_of(&s, 1), 5);
    }
}

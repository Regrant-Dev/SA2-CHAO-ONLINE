//! Builds the private local creature cache from the player's own cartridge image: battle data,
//! 3D actors, thumbnails, field motions and the battle venue, published only when complete.
//! The cache stays on the player's computer and is never part of release packages.
//! Developer inspection commands are built only with the `game-import-dev` feature.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sa2_chao_online::creature::asset_import::write_new_file;
use sa2_chao_online::creature::data;
use sa2_chao_online::creature::import_error;
use sa2_chao_online::creature::source::{ErrorKind, GameSource, ImportError, Result};
use serde_json::{Value, json};

/// A complete cache's size on disk, rounded up (about 3.55 GB in 5,300 files).
const CACHE_BYTES: u64 = 3_600_000_000;
/// Free space a build asks for next to the cache folder: twice the cache, for safety.
const NEEDED_BYTES: u64 = 2 * CACHE_BYTES;
/// The folder a build fills before it's published: `.source-cache-<process id>`.
const STAGE_PREFIX: &str = ".source-cache-";
/// A replaced cache, removed after publication: `<cache>.previous-<process id>-<time>`.
const PREVIOUS: &str = ".previous-";

const USAGE: &str = "sa2co-import build IMAGE --output CACHE_DIRECTORY\n\
sa2co-import verify-cache CACHE_DIRECTORY [--actors] [--output REPORT.json]";

fn main() {
    if let Err(error) = run() {
        // The only line on stderr (progress goes to stdout): the mod reads its code to tell
        // the player what to do.
        eprintln!("{}", import_error::line(error.kind, &error.message));
        std::process::exit(1);
    }
}

fn usage() -> ImportError {
    ImportError::with(
        ErrorKind::Usage,
        "unknown command or arguments; see sa2co-import --help",
    )
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("{USAGE}");
        #[cfg(feature = "game-import-dev")]
        println!("{}", dev::USAGE);
        return Ok(());
    }
    let command = args[0].to_str().ok_or_else(usage)?;
    let image = args.get(1).map(PathBuf::from).ok_or_else(usage)?;
    let mut options: Vec<String> = args
        .iter()
        .skip(2)
        .map(|s| s.to_str().map(str::to_owned).ok_or_else(usage))
        .collect::<Result<_>>()?;
    let output = take_option(&mut options, "--output")?.map(PathBuf::from);
    match command {
        "verify-cache" => {
            let actors = take_flag(&mut options, "--actors");
            require_empty(&options)?;
            let report = verify_cache(&image, actors)?;
            publish(report, output.as_deref())
        }
        "build" => {
            require_empty(&options)?;
            let output = output.ok_or_else(|| {
                ImportError::with(
                    ErrorKind::Usage,
                    "build needs --output pointing to a private cache directory",
                )
            })?;
            let source = GameSource::open(&image)?;
            build_cache(&source, &output)
        }
        #[cfg(feature = "game-import-dev")]
        _ => dev::run(command, &image, options, output),
        #[cfg(not(feature = "game-import-dev"))]
        _ => Err(usage()),
    }
}

/// A failure after the image's battle data has read cleanly: converting and writing aren't the
/// dump's fault (a damaged image, a changed file and the cache folder keep their own kinds).
fn after_reading(mut error: ImportError) -> ImportError {
    if error.kind == ErrorKind::Invalid {
        error.kind = ErrorKind::Failed;
    }
    error
}

/// A failure at the cache folder itself (out of space stays `space`).
fn at_cache(error: impl Into<ImportError>) -> ImportError {
    let mut error = error.into();
    if error.kind == ErrorKind::Invalid {
        error.kind = ErrorKind::Cache;
    }
    error
}

/// Publish a complete cache only after every imported model and catalog validates.
fn build_cache(source: &GameSource, output: &Path) -> Result<()> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = output
        .file_name()
        .ok_or_else(|| ImportError::with(ErrorKind::Cache, "Cache destination has no folder name"))?
        .to_string_lossy()
        .into_owned();
    let canonical_output = if output.exists() {
        output.canonicalize().map_err(at_cache)?
    } else {
        fs::create_dir_all(parent).map_err(at_cache)?;
        parent.canonicalize().map_err(at_cache)?.join(&name)
    };
    if source.source_path().starts_with(&canonical_output) {
        return Err(ImportError::with(
            ErrorKind::Cache,
            "The cache destination cannot contain the source image; choose a separate cache folder",
        ));
    }
    let mut replaced = 0;
    if output.exists() {
        let metadata = fs::symlink_metadata(output).map_err(at_cache)?;
        let empty = metadata.is_dir() && fs::read_dir(output).map_err(at_cache)?.next().is_none();
        let owned = metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && (output.join("ready.json").is_file()
                || (output.join("catalog.json").is_file() && output.join("assets").is_dir()));
        if !empty && !owned {
            return Err(ImportError::with(
                ErrorKind::Cache,
                "The cache destination contains unrelated files; choose an empty or existing mod cache folder",
            ));
        }
        if owned {
            replaced = folder_bytes(output);
        }
    }
    fs::create_dir_all(parent).map_err(at_cache)?;
    remove_stale(parent, &name, output.join("ready.json").is_file());
    let needed = needed_space(replaced);
    if let Some(free) = free_space(parent)
        && free < needed
    {
        return Err(ImportError::with(
            ErrorKind::Space,
            format!(
                "Not enough free disk space: building the cache needs about {:.1} GB free on its drive, and {:.1} GB is free",
                needed as f64 / 1e9,
                free as f64 / 1e9
            ),
        ));
    }
    let stage = parent.join(format!("{STAGE_PREFIX}{}", std::process::id()));
    fs::create_dir(&stage).map_err(at_cache)?;
    let result = (|| -> Result<()> {
        println!("Reading battle data and verifying the source...");
        // Reading and parsing the image's own archives: a failure here is the dump's.
        let catalog = sa2_chao_online::creature::catalog::import(source)?;
        convert(source, &catalog, &stage, output).map_err(after_reading)?;
        // The new cache is complete: the one it replaced isn't needed for a rollback.
        remove_stale(parent, &name, true);
        println!("Local cache ready at {}", output.display());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

/// Convert the actors, motions and venue into `stage`, write the catalog and publish the cache.
fn convert(
    source: &GameSource,
    catalog: &sa2_chao_online::creature::catalog::Envelope,
    stage: &Path,
    output: &Path,
) -> Result<()> {
    println!("Converting private 3D actors...");
    let report = sa2_chao_online::creature::asset_import::import_assets(
        source,
        &stage.join("assets"),
        1,
        data::MAX_SPECIES,
    )?;
    if !report.failed.is_empty() || report.models != usize::from(data::MAX_SPECIES) {
        return Err(ImportError::new(format!(
            "Asset conversion incomplete: {} of {} models succeeded; first errors: {:?}",
            report.models,
            data::MAX_SPECIES,
            report.failed.iter().take(5).collect::<Vec<_>>()
        )));
    }
    println!("Reading authored field movement clips...");
    let keys: Vec<_> = data::species_forms(&catalog.data.personal)
        .map_err(ImportError::new)?
        .into_iter()
        .map(|f| f.key)
        .filter(|&key| data::ordinary_form(key))
        .collect();
    let motions = sa2_chao_online::creature::asset_import::import_field_motions(
        source,
        &stage.join("assets"),
        &keys,
    )?;
    println!(
        "Field motions: {} identities, {} without a walk, {} unavailable packages",
        motions.written + motions.reused,
        motions.missing_walk.len(),
        motions.failed.len()
    );
    println!("Converting the local battle venue...");
    sa2_chao_online::creature::asset_import::import_venue(source, &stage.join("assets"), 14)?;
    let bytes = serde_json::to_vec(&catalog).map_err(|e| ImportError::new(e.to_string()))?;
    write_new(
        &stage.join(sa2_chao_online::creature::catalog::FILE),
        &bytes,
    )?;
    write_new(
        &stage.join("ready.json"),
        &serde_json::to_vec(&sa2_chao_online::creature::catalog::Ready::new(
            &catalog.fingerprint,
            &bytes,
        ))
        .map_err(|e| ImportError::new(e.to_string()))?,
    )?;
    source.verify_unchanged()?;
    publish_cache(stage, output).map_err(at_cache)
}

/// Free space a build asks for: `NEEDED_BYTES`, less the size of the cache it replaces (that one
/// is the margin: it's removed once the new one is published), but always room for a whole cache.
fn needed_space(replaced: u64) -> u64 {
    NEEDED_BYTES.saturating_sub(replaced).max(CACHE_BYTES)
}

/// The total size of the files under `folder`, not following links.
fn folder_bytes(folder: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let metadata = fs::symlink_metadata(entry.path()).ok()?;
            Some(if metadata.is_dir() {
                folder_bytes(&entry.path())
            } else if metadata.is_file() {
                metadata.len()
            } else {
                0
            })
        })
        .sum()
}

/// Preserve the previous cache until the replacement is complete, and roll back publication
/// failures. Never delete an existing player folder or its source image.
fn publish_cache(stage: &Path, output: &Path) -> Result<()> {
    if !output.exists() {
        fs::rename(stage, output)?;
        return Ok(());
    }
    let name = output
        .file_name()
        .ok_or_else(|| ImportError::new("Cache destination has no folder name"))?
        .to_string_lossy();
    let backup = output.with_file_name(format!(
        "{name}{PREVIOUS}{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| ImportError::new(e.to_string()))?
            .as_nanos()
    ));
    fs::rename(output, &backup)?;
    if let Err(error) = fs::rename(stage, output) {
        let restored = fs::rename(&backup, output);
        if let Err(rollback) = restored {
            return Err(ImportError::new(format!(
                "Cache publication failed ({error}); previous cache is preserved at {} ({rollback})",
                backup.display()
            )));
        }
        return Err(error.into());
    }
    Ok(())
}

/// The process id of an unfinished build's folder name.
fn stage_process(file_name: &str) -> Option<u32> {
    let id = file_name.strip_prefix(STAGE_PREFIX)?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        .then(|| id.parse().ok())
        .flatten()
}

/// Whether `file_name` is a replaced copy of cache folder `name`.
fn is_previous(file_name: &str, name: &str) -> bool {
    let Some(rest) = file_name
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(PREVIOUS))
    else {
        return false;
    };
    let mut parts = rest.split('-');
    let digits = |part: Option<&str>| {
        part.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    };
    digits(parts.next()) && digits(parts.next()) && parts.next().is_none()
}

/// Removes what earlier builds left in the cache's parent folder: unfinished build folders
/// whose process is gone and, once the cache is complete (`complete`), replaced copies of it.
/// Only folders directly in `parent` whose names match those patterns are removed; never a
/// link, a file or anything else of the player's.
fn remove_stale(parent: &Path, name: &str, complete: bool) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let stale = match stage_process(file_name) {
            Some(id) => id == std::process::id() || !process_running(id),
            None => complete && is_previous(file_name, name),
        };
        if !stale {
            continue;
        }
        let path = entry.path();
        let folder =
            fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
        if folder && let Err(error) = fs::remove_dir_all(&path) {
            println!("Couldn't remove the old build folder {file_name}: {error}");
        }
    }
}

/// Whether process `id` is still running. When that can't be told, it counts as running, so
/// its folder is kept.
#[cfg(windows)]
fn process_running(id: u32) -> bool {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut c_void;
        fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }
    const QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    const ERROR_INVALID_PARAMETER: u32 = 87;
    // SAFETY: plain process queries; the handle is closed before returning.
    unsafe {
        let process = OpenProcess(QUERY_LIMITED_INFORMATION, 0, id);
        if process.is_null() {
            // No such process; any other failure (access denied) means it exists.
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        let mut code = 0;
        let running = GetExitCodeProcess(process, &mut code) == 0 || code == STILL_ACTIVE;
        CloseHandle(process);
        running
    }
}

#[cfg(target_os = "linux")]
fn process_running(id: u32) -> bool {
    !Path::new("/proc/self").exists() || Path::new("/proc").join(id.to_string()).exists()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn process_running(_id: u32) -> bool {
    true
}

/// Bytes free to this user on the drive holding `folder` (`None` when that can't be told).
#[cfg(windows)]
fn free_space(folder: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            folder: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = folder.as_os_str().encode_wide().chain([0]).collect();
    let mut available = 0;
    // SAFETY: a terminated path and one output; the others may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn free_space(folder: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn statvfs(path: *const std::ffi::c_char, buffer: *mut [u64; 16]) -> i32;
    }
    let path = std::ffi::CString::new(folder.as_os_str().as_bytes()).ok()?;
    // struct statvfs on 64-bit Linux: f_bsize, f_frsize, f_blocks, f_bfree, f_bavail, ...
    // all 64-bit, 112 bytes in all.
    let mut buffer = [0u64; 16];
    // SAFETY: a terminated path and a buffer larger than the structure.
    let ok = unsafe { statvfs(path.as_ptr(), &mut buffer) } == 0;
    ok.then(|| buffer[1].saturating_mul(buffer[4]))
}

#[cfg(not(any(windows, all(target_os = "linux", target_pointer_width = "64"))))]
fn free_space(_folder: &Path) -> Option<u64> {
    None
}

fn verify_cache(path: &Path, actors: bool) -> Result<Value> {
    use sa2_chao_online::{battle, creature};
    let rom = creature::rom::Rom::open(path).map_err(ImportError::new)?;
    let dex = battle::team::Dex::from_rom(&rom).map_err(ImportError::new)?;
    let roster = battle::trainers::Roster::from_rom(&rom, &dex).ok_or_else(|| {
        ImportError::new("No complete eligible trainer ladder in the local cache")
    })?;
    let mut supported = 0;
    let mut excluded = 0;
    let mut unavailable = Vec::new();
    for id in 1..creature::rom::MOVE_COUNT {
        let record = rom
            .modern_move(id)
            .ok_or_else(|| ImportError::new("Incomplete move catalog"))?;
        match battle::source_rules::support_status(record) {
            battle::source_rules::SupportStatus::Supported => supported += 1,
            battle::source_rules::SupportStatus::ExcludedPowerUp => excluded += 1,
            battle::source_rules::SupportStatus::Unsupported(reason) => {
                unavailable.push(json!({"id":id,"reason":reason}))
            }
        }
    }
    let mut checked = 0;
    let mut animated = 0;
    if actors {
        for key in rom.species_keys() {
            for shiny in [false, true] {
                let actor = creature::assets::load_actor(
                    &rom.asset_root(),
                    key.species,
                    u16::from(key.form),
                    shiny,
                )
                .map_err(ImportError::new)?;
                if actor.idle.is_some() {
                    animated += 1;
                }
                for seconds in [0., 0.5, 1.25] {
                    let pose = actor.skinned(seconds);
                    if pose
                        .iter()
                        .flatten()
                        .any(|(v, n)| v.iter().chain(n).any(|x| !x.is_finite() || x.abs() > 1e6))
                    {
                        return Err(ImportError::new(format!(
                            "Nonfinite posed actor {} form {}",
                            key.species, key.form
                        )));
                    }
                }
                creature::assets::load_thumbnail(
                    &rom.asset_root(),
                    key.species,
                    u16::from(key.form),
                    shiny,
                )
                .ok_or_else(|| {
                    ImportError::new(format!(
                        "Missing or invalid actor thumbnail {} form {}",
                        key.species, key.form
                    ))
                })?;
                checked += 1;
            }
        }
    }
    Ok(
        json!({"schema":"sa2co-cache-verification-v1","profile":creature::catalog::PROFILE,
        "fingerprint":rom.fingerprint(),"rules":battle::source_rules::RULES_VERSION,
        "base_species":creature::rom::SPECIES_COUNT-1,"supported_moves":supported,"excluded_power_up_moves":excluded,
        "unsupported_ordinary_moves":unavailable,"eligible_trainers":roster.foes.len(),
        "checked_actor_variants":checked,"animated_variants":animated,"live_game_verified":false}),
    )
}

fn take_option(options: &mut Vec<String>, flag: &str) -> Result<Option<String>> {
    let Some(at) = options.iter().position(|s| s == flag) else {
        return Ok(None);
    };
    if at + 1 >= options.len() || options[at + 1].starts_with("--") {
        return Err(ImportError::with(
            ErrorKind::Usage,
            format!("{flag} needs a value"),
        ));
    }
    options.remove(at);
    Ok(Some(options.remove(at)))
}

fn take_flag(options: &mut Vec<String>, flag: &str) -> bool {
    options
        .iter()
        .position(|s| s == flag)
        .map(|at| options.remove(at))
        .is_some()
}

fn require_empty(options: &[String]) -> Result<()> {
    if options.is_empty() {
        Ok(())
    } else {
        Err(ImportError::with(
            ErrorKind::Usage,
            format!("unexpected arguments: {}", options.join(" ")),
        ))
    }
}

fn publish(report: Value, output: Option<&Path>) -> Result<()> {
    let mut bytes =
        serde_json::to_vec_pretty(&report).map_err(|e| ImportError::new(e.to_string()))?;
    bytes.push(b'\n');
    if let Some(path) = output {
        write_new(path, &bytes)?;
        println!("Wrote private inspection report to {}", path.display());
    } else {
        std::io::stdout().write_all(&bytes)?;
    }
    Ok(())
}

/// Explicit outputs never replace an existing source, report or user file. Temporary data
/// stays in the destination directory and is removed on success and failure.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    write_new_file(path, bytes, true)
}

/// Developer commands for inspecting a source's formats and making private samples.
#[cfg(feature = "game-import-dev")]
mod dev {
    use super::*;
    use sa2_chao_online::creature::archive::{Garc, decompress_lz11};
    use sa2_chao_online::creature::data::{MovePolicy, SpeciesKey};

    const LIMIT: usize = 32 << 20;
    pub const USAGE: &str = "Developer commands:\n\
sa2co-import inspect IMAGE [--full-hash] [--output REPORT.json]\n\
sa2co-import list IMAGE [ROMFS_PREFIX]\n\
sa2co-import catalog IMAGE --output CATALOG.json\n\
sa2co-import build-data IMAGE --output CATALOG.json\n\
sa2co-import assets IMAGE --output ASSET_DIRECTORY [--first N] [--last N]\n\
sa2co-import field-motions IMAGE --output ASSET_DIRECTORY [--keys SPECIES:FORM,...]\n\
sa2co-import venue IMAGE --output ASSET_DIRECTORY\n\
sa2co-import extract IMAGE ROMFS_PATH ENTRY [--subfile N] [--lz11] --output FILE";

    fn needs_output(output: Option<PathBuf>, message: &str) -> Result<PathBuf> {
        output.ok_or_else(|| ImportError::with(ErrorKind::Usage, message))
    }

    fn number<T: std::str::FromStr>(value: &str, what: &str) -> Result<T> {
        value
            .parse()
            .map_err(|_| ImportError::with(ErrorKind::Usage, format!("invalid {what}")))
    }

    fn number_option<T: std::str::FromStr>(
        options: &mut Vec<String>,
        flag: &str,
        what: &str,
    ) -> Result<Option<T>> {
        take_option(options, flag)?
            .map(|value| number(&value, what))
            .transpose()
    }

    pub fn run(
        command: &str,
        image: &Path,
        mut options: Vec<String>,
        output: Option<PathBuf>,
    ) -> Result<()> {
        if !matches!(
            command,
            "inspect"
                | "list"
                | "catalog"
                | "build-data"
                | "field-motions"
                | "venue"
                | "assets"
                | "extract"
        ) {
            return Err(usage());
        }
        let source = GameSource::open(image)?;
        match command {
            "inspect" => {
                let full = take_flag(&mut options, "--full-hash");
                require_empty(&options)?;
                let mut report = source_report(&source);
                if full {
                    report["source_sha256"] = json!(hex(&source.sha256()?));
                }
                source.verify_unchanged()?;
                publish(report, output.as_deref())?;
            }
            "list" => {
                if output.is_some() || options.len() > 1 {
                    return Err(usage());
                }
                let prefix = options.first().map(String::as_str).unwrap_or("");
                source.verify_unchanged()?;
                for (name, region) in source
                    .files()
                    .iter()
                    .filter(|(name, _)| name.starts_with(prefix))
                {
                    println!("{}\t{}\t{}", name, region.offset, region.size);
                }
            }
            "catalog" => {
                require_empty(&options)?;
                let output = needs_output(
                    output,
                    "catalog needs --output pointing to a private local file",
                )?;
                let report = catalog(&source)?;
                source.verify_unchanged()?;
                publish(report, Some(&output))?;
            }
            "build-data" => {
                require_empty(&options)?;
                let output = needs_output(
                    output,
                    "build-data needs --output pointing to a private catalog file",
                )?;
                let report = sa2_chao_online::creature::catalog::import(&source)?;
                let bytes =
                    serde_json::to_vec(&report).map_err(|e| ImportError::new(e.to_string()))?;
                write_new(&output, &bytes)?;
                println!(
                    "Built runtime data catalog ({} bytes), fingerprint {}",
                    bytes.len(),
                    report.fingerprint
                );
            }
            "field-motions" => {
                let selected = take_option(&mut options, "--keys")?;
                require_empty(&options)?;
                let output = needs_output(
                    output,
                    "field-motions needs --output pointing to a private asset directory",
                )?;
                let personal = Garc::open(&source, source.file("a/0/1/7")?)?;
                let records = data::parse_personal_table(
                    &personal.read_entry(personal.entry_count() - 1, LIMIT)?,
                )
                .map_err(ImportError::new)?;
                let keys = if let Some(selected) = selected {
                    selected
                        .split(',')
                        .map(|key| {
                            let (species, form) = key.split_once(':').unwrap_or((key, "0"));
                            Ok(SpeciesKey {
                                species: number(species, "motion species")?,
                                form: number(form, "motion form")?,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?
                } else {
                    data::species_forms(&records)
                        .map_err(ImportError::new)?
                        .into_iter()
                        .map(|f| f.key)
                        .filter(|&key| data::ordinary_form(key))
                        .collect()
                };
                let report = sa2_chao_online::creature::asset_import::import_field_motions(
                    &source, &output, &keys,
                )?;
                println!(
                    "Field motions: {} written, {} reused, {} without an authored walk, {} errors",
                    report.written,
                    report.reused,
                    report.missing_walk.len(),
                    report.failed.len()
                );
                if !report.failed.is_empty() {
                    return Err(ImportError::new(format!(
                        "field motion failures: {:?}",
                        report.failed
                    )));
                }
            }
            "venue" => {
                require_empty(&options)?;
                let output = needs_output(
                    output,
                    "venue needs --output pointing to a private asset directory",
                )?;
                let actor =
                    sa2_chao_online::creature::asset_import::import_venue(&source, &output, 14)?;
                println!(
                    "Imported battle venue: {} meshes, {} textures",
                    actor.meshes.len(),
                    actor.textures.len()
                );
            }
            "assets" => {
                let first = number_option(&mut options, "--first", "first species")?.unwrap_or(1);
                let last = number_option(&mut options, "--last", "last species")?
                    .unwrap_or(data::MAX_SPECIES);
                require_empty(&options)?;
                let output = needs_output(
                    output,
                    "assets needs --output pointing to a private cache directory",
                )?;
                let report = sa2_chao_online::creature::asset_import::import_assets(
                    &source, &output, first, last,
                )?;
                source.verify_unchanged()?;
                println!(
                    "Imported {} models / {} variants / {} animated, {} bytes",
                    report.models, report.variants, report.animated, report.bytes
                );
                if !report.failed.is_empty() {
                    return Err(ImportError::new(format!(
                        "asset failures: {:?}",
                        report.failed
                    )));
                }
            }
            _ => {
                let subfile =
                    number_option(&mut options, "--subfile", "subfile number")?.unwrap_or(0u8);
                let decode = take_flag(&mut options, "--lz11");
                if options.len() != 2 {
                    return Err(usage());
                }
                let index: usize = number(&options[1], "archive member index")?;
                let output = needs_output(
                    output,
                    "extract needs --output pointing to a private local file",
                )?;
                let archive = Garc::open(&source, source.file(&options[0])?)?;
                let bytes = archive.read_member(index, subfile, LIMIT)?;
                let bytes = if decode {
                    decompress_lz11(&bytes, LIMIT)?
                } else {
                    bytes
                };
                source.verify_unchanged()?;
                write_new(&output, &bytes)?;
                println!("Extracted {} bytes to {}", bytes.len(), output.display());
            }
        }
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn source_report(source: &GameSource) -> Value {
        let info = source.info();
        json!({
            "schema": "sa2co-source-inspection-v1",
            "status": "supported-base-readable",
            "signature_authenticity_verified": false,
            "file_size": info.file_size,
            "program_id": format!("{:016x}", info.program_id),
            "product_code": info.product_code,
            "ncch_format_version": info.ncch_format_version,
            "remaster_version": info.remaster_version,
            "smdh_region_mask": format!("{:08x}", info.smdh_region_mask),
            "ncch_flags": hex(&info.ncch_flags),
            "plaintext_with_unfixed_flag": info.plaintext_with_unfixed_flag,
            "exefs_header_sha256": hex(&info.exefs_header_sha256),
            "exefs_header_digest_verified": true,
            "exheader_digest_verified": true,
            "smdh_icon_digest_verified": true,
            "romfs_superblock_digest_verified": true,
            "full_romfs_hash_tree_verified": false,
            "romfs_file_count": source.files().len(),
            "romfs_offset": info.romfs.offset,
            "romfs_size": info.romfs.size,
            "update_overlay_applied": false,
        })
    }

    fn catalog(source: &GameSource) -> Result<Value> {
        let personal = Garc::open(source, source.file("a/0/1/7")?)?;
        let records =
            data::parse_personal_table(&personal.read_entry(personal.entry_count() - 1, LIMIT)?)
                .map_err(ImportError::new)?;
        let forms = data::species_forms(&records).map_err(ImportError::new)?;
        let moves = Garc::open(source, source.file("a/0/1/1")?)?;
        let moves =
            data::parse_move_archive(&moves.read_entry(0, LIMIT)?).map_err(ImportError::new)?;
        let learned = Garc::open(source, source.file("a/0/1/3")?)?;
        let text = Garc::open(source, source.file("a/0/3/2")?)?;
        let species_names =
            data::parse_text(&text.read_entry(60, LIMIT)?).map_err(ImportError::new)?;
        let ability_names =
            data::parse_text(&text.read_entry(101, LIMIT)?).map_err(ImportError::new)?;
        let type_names =
            data::parse_text(&text.read_entry(112, LIMIT)?).map_err(ImportError::new)?;
        let move_names =
            data::parse_text(&text.read_entry(118, LIMIT)?).map_err(ImportError::new)?;
        if species_names.len() <= usize::from(data::MAX_SPECIES)
            || ability_names.len() != usize::from(data::MAX_ABILITY) + 1
            || type_names.len() != usize::from(data::TYPE_COUNT)
            || move_names.len() != moves.len()
        {
            return Err(ImportError::new(
                "the source's text and data catalogs don't match in size",
            ));
        }
        let species: Vec<_> = forms.iter().map(|form| {
            let record = &records[usize::from(form.personal_index)];
            json!({"species": form.key.species, "base_species_name": species_names[usize::from(form.key.species)],
                "form": form.key.form, "personal_index": form.personal_index,
                "base_stats_hp_atk_def_speed_spatk_spdef": record.base_stats,
                "types": record.types, "abilities": record.abilities,
                "base_exp": record.base_exp, "height_cm": record.height_cm, "weight_hg": record.weight_hg,
                "gender_ratio": record.gender_ratio, "regional_variant": record.regional_variant,
                "form_count": record.form_count, "source_form_sprite_index": record.form_sprite_index})
        }).collect();
        let move_records: Vec<_> = moves.iter().map(|record| json!({
            "id": record.id, "name": move_names[usize::from(record.id)],
            "type": record.type_id, "category": format!("{:?}", record.category),
            "power": record.power, "accuracy_source": record.accuracy, "pp": record.pp,
            "accuracy_kind": if record.accuracy == 101 { "always" } else { "percent" },
            "priority": record.priority, "raw_effect_sequence": record.effect_sequence,
            "flags": record.flags, "policy": format!("{:?}", record.policy),
            "ordinary_power_up_metadata": {"move_id": record.power_up.move_id, "power": record.power_up.power, "effect": record.power_up.effect},
        })).collect();
        let mut samples = Vec::new();
        for species in [25, 722, 807] {
            let key = SpeciesKey { species, form: 0 };
            let form = forms
                .iter()
                .find(|f| f.key == key)
                .ok_or_else(|| ImportError::new("sample species missing"))?;
            let learnset =
                data::parse_learnset(&learned.read_entry(usize::from(form.personal_index), LIMIT)?)
                    .map_err(ImportError::new)?;
            samples.push(json!({"species": species, "learnset": learnset.iter().map(|m| json!({"move": m.move_id, "level": m.level})).collect::<Vec<_>>()}));
        }
        let trainer_headers = Garc::open(source, source.file("a/1/0/6")?)?;
        let trainer_parties = Garc::open(source, source.file("a/1/0/7")?)?;
        let trainer = data::parse_trainer(
            &trainer_headers.read_entry(1, LIMIT)?,
            &trainer_parties.read_entry(1, LIMIT)?,
            1,
        )
        .map_err(ImportError::new)?;
        let trainer_names =
            data::parse_text(&text.read_entry(110, LIMIT)?).map_err(ImportError::new)?;
        let trainer_classes =
            data::parse_text(&text.read_entry(111, LIMIT)?).map_err(ImportError::new)?;
        let encounters = Garc::open(source, source.file("a/0/8/2")?)?;
        let encounter_bytes = encounters.read_entry(9, LIMIT)?;
        let encounter_bytes = decompress_lz11(&encounter_bytes, LIMIT)?;
        let encounter_tables =
            data::parse_encounter_archive(&encounter_bytes).map_err(ImportError::new)?;
        let table = |table: &data::EncounterTable| {
            json!({
                "min_level": table.min_level, "max_level": table.max_level, "rates": table.rates,
                "slot_groups": table.slots.iter().map(|group| group.iter().map(|slot| slot.map(|key|
                    json!({"species": key.species, "form": key.form}))).collect::<Vec<_>>()).collect::<Vec<_>>()
            })
        };
        let mut report = source_report(source);
        report["catalog"] = json!({
            "language": "English", "text_archive": "a/0/3/2",
            "base_species": data::MAX_SPECIES, "personal_records": records.len(), "declared_forms": forms.len(),
            "species": species,
            "type_names": type_names, "ability_names": ability_names,
            "move_records_including_zero": moves.len(),
            "ordinary_moves": moves.iter().filter(|m| m.id != 0 && m.policy == MovePolicy::Ordinary).count(),
            "excluded_power_up_moves": moves.iter().filter(|m| m.policy == MovePolicy::ExcludedPowerUpMove).count(),
            "moves": move_records, "learnset_samples": samples,
            "trainer_sample": { "id": trainer.id, "name": trainer_names.get(usize::from(trainer.id)),
                "class_id": trainer.class_id, "class_name": trainer_classes.get(usize::from(trainer.class_id)),
                "battle_mode": trainer.mode, "raw_ai": trainer.ai, "items": trainer.items,
                "party": trainer.party.iter().map(|m| json!({"species": m.key.species, "form": m.key.form,
                    "level": m.level, "moves": m.moves, "held_item": m.held_item,
                    "ability_selector": m.ability_selector, "gender_selector": m.gender_selector,
                    "nature": m.nature, "ivs_hp_atk_def_speed_spatk_spdef": m.ivs,
                    "evs_hp_atk_def_speed_spatk_spdef": m.evs, "shiny": m.shiny})).collect::<Vec<_>>() },
            "encounter_sample": {"romfs_archive": "a/0/8/2", "garc_entry": 9,
                "tables": encounter_tables.iter().map(|entry| json!({"source_table": entry.source_table,
                    "day": table(&entry.day), "night": table(&entry.night)})).collect::<Vec<_>>()},
            "note": "Source data inspection only. Importing records does not implement their battle effects or allow special forms."
        });
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_publication_preserves_the_previous_complete_directory() {
        let root = std::env::temp_dir().join(format!("sa2co-cache-swap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let output = root.join("cache");
        let stage = root.join("stage");
        fs::create_dir_all(&output).unwrap();
        fs::create_dir_all(&stage).unwrap();
        fs::write(output.join("catalog.json"), b"old cache").unwrap();
        fs::write(stage.join("catalog.json"), b"new cache").unwrap();
        publish_cache(&stage, &output).unwrap();
        assert_eq!(fs::read(output.join("catalog.json")).unwrap(), b"new cache");
        let previous = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| is_previous(&p.file_name().unwrap().to_string_lossy(), "cache"))
            .unwrap();
        assert_eq!(
            fs::read(previous.join("catalog.json")).unwrap(),
            b"old cache"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_build_folders_are_removed_only_by_their_exact_names() {
        assert_eq!(stage_process(".source-cache-123"), Some(123));
        for name in [
            ".source-cache-",
            ".source-cache-12a",
            ".source-cache-1/2",
            "source-cache-1",
        ] {
            assert_eq!(stage_process(name), None, "{name}");
        }
        assert!(is_previous("cache.previous-12-345", "cache"));
        for name in [
            "cache.previous-12",
            "cache.previous-12-345-6",
            "cache.previous--345",
            "other.previous-12-345",
            "cache",
            "cache.previous-1x-2",
        ] {
            assert!(!is_previous(name, "cache"), "{name}");
        }

        let root = std::env::temp_dir().join(format!("sa2co-cache-sweep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        // Ours (this process's own leftover) and one of a process that isn't running.
        let gone = format!("{STAGE_PREFIX}{}", u32::MAX - 1);
        let own = format!("{STAGE_PREFIX}{}", std::process::id());
        let keep = [
            "cache",
            "other.previous-1-2",
            "cache.previous-1-2-3",
            ".source-cache-x",
            "notes.txt",
        ];
        for name in [&gone, &own, "cache.previous-1-2"].into_iter().chain(keep) {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        // A file with a matching name is never the importer's folder.
        fs::write(root.join("cache.previous-9-9"), b"player file").unwrap();
        remove_stale(&root, "cache", false);
        assert!(!root.join(&gone).exists());
        assert!(!root.join(&own).exists());
        assert!(root.join("cache.previous-1-2").exists());
        remove_stale(&root, "cache", true);
        assert!(!root.join("cache.previous-1-2").exists());
        assert!(root.join("cache.previous-9-9").is_file());
        for name in keep {
            assert!(root.join(name).exists(), "{name}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_replaced_cache_counts_toward_the_space_a_build_needs() {
        assert_eq!(needed_space(0), NEEDED_BYTES);
        assert_eq!(needed_space(CACHE_BYTES), CACHE_BYTES);
        assert_eq!(needed_space(NEEDED_BYTES * 2), CACHE_BYTES);
        let root = std::env::temp_dir().join(format!("sa2co-cache-size-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("assets/a")).unwrap();
        fs::write(root.join("catalog.json"), [0; 10]).unwrap();
        fs::write(root.join("assets/a/model"), [0; 32]).unwrap();
        assert_eq!(folder_bytes(&root), 42);
        assert_eq!(folder_bytes(&root.join("missing")), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn free_space_is_known_for_an_existing_folder() {
        if cfg!(any(
            windows,
            all(target_os = "linux", target_pointer_width = "64")
        )) {
            assert!(free_space(&std::env::temp_dir()).is_some_and(|free| free > 0));
        }
    }

    #[test]
    fn output_publication_never_replaces_existing_files() {
        let directory =
            std::env::temp_dir().join(format!("sa2co-import-output-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join("private.json");
        write_new(&path, b"first").unwrap();
        assert!(write_new(&path, b"replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}

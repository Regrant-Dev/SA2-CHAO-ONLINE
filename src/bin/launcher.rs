//! Check/install the published mod before starting Steam's original game command.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const API: &str = "https://api.github.com/repos/Regrant-Dev/SA2-CHAO-ONLINE/releases/latest";
const DOWNLOAD: &str = "https://github.com/Regrant-Dev/SA2-CHAO-ONLINE/releases/download/";
const ASSET: &str = "sa2-chao-online.zip";
const FILES: &[&str] = &[
    "sa2-chao-online.dll",
    "mod.ini",
    "configschema.xml",
    "how-to-play.md",
    "LICENSE.txt",
    "mod.build",
    "mod.sha256",
    "sa2co-launcher.exe",
    "sa2co-launcher",
];
const MAX_DOWNLOAD: u64 = 32 << 20;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Release {
    build: String,
    url: String,
    date: String,
    digest: Option<String>,
}

impl Release {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let build = value["target_commitish"].as_str().ok_or("missing build")?;
        if build.len() != 40 || !build.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("release doesn't identify an exact build".into());
        }
        let asset = value["assets"]
            .as_array()
            .and_then(|list| list.iter().find(|a| a["name"] == ASSET))
            .ok_or("the release download isn't available yet; try again shortly")?;
        let url = asset["browser_download_url"]
            .as_str()
            .ok_or("missing download URL")?;
        if !url.starts_with(DOWNLOAD) || !url.ends_with(&format!("/{ASSET}")) {
            return Err("unexpected release download URL".into());
        }
        Ok(Self {
            build: build.to_owned(),
            url: url.to_owned(),
            date: asset["updated_at"]
                .as_str()
                .ok_or("missing upload date")?
                .to_owned(),
            digest: asset["digest"].as_str().map(str::to_owned),
        })
    }
}

fn fetch(url: &str, to: &Path, limit: u64) -> Result<()> {
    // curl ships with supported Windows 10/11 systems; Linux uses its native curl.
    // Use the system copy on Windows rather than depending on the manager or PowerShell.
    #[cfg(windows)]
    let curl = {
        let root =
            PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot is unavailable")?);
        // A 32-bit launcher on 64-bit Windows must bypass System32's WOW64 redirection.
        let native = root.join("Sysnative/curl.exe");
        if native.is_file() {
            native
        } else {
            root.join("System32/curl.exe")
        }
    };
    #[cfg(not(windows))]
    let curl = PathBuf::from("curl");
    let mut command = Command::new(curl);
    command.args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "10",
        "--max-time",
        "90",
        "--retry",
        "1",
        "--user-agent",
        "SA2-Chao-Online-Updater",
        "--header",
        "Accept: application/vnd.github+json",
        "--max-filesize",
    ]);
    command
        .arg(limit.to_string())
        .arg("--output")
        .arg(to)
        .arg(url);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command
        .output()
        .map_err(|e| format!("couldn't run the system curl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    if fs::metadata(to)?.len() > limit {
        return Err("download is larger than expected".into());
    }
    Ok(())
}

fn unpack(archive: &Path, stage: &Path, release: &Release) -> Result<Vec<String>> {
    if let Some(digest) = &release.digest {
        let actual = format!("sha256:{:x}", Sha256::digest(fs::read(archive)?));
        if digest != &actual {
            return Err("download checksum differs from GitHub's release asset".into());
        }
    }
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    if zip.len() > 128 {
        return Err("unexpected number of files in update".into());
    }
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_owned();
        if entry.enclosed_name().is_none()
            || name.contains(['\\', ':'])
            || name.split('/').any(|part| part == "..")
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("unsafe path in update".into());
        }
        if entry.is_dir() {
            continue;
        }
        let Some(name) = name.strip_prefix("sa2-chao-online/") else {
            return Err("unexpected update folder".into());
        };
        // Settings, skins, Chao exports and saves are never part of an automatic update.
        if !FILES.contains(&name) {
            continue;
        }
        if !seen.insert(name.to_owned()) {
            return Err("duplicate file in update".into());
        }
        if entry.size() > MAX_DOWNLOAD || entry.size() > (64 << 20) - total {
            return Err("unpacked update is larger than expected".into());
        }
        total += entry.size();
        let mut bytes = Vec::new();
        (&mut entry)
            .take(MAX_DOWNLOAD + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_DOWNLOAD {
            return Err("unpacked file is larger than expected".into());
        }
        fs::write(stage.join(name), bytes)?;
        files.push(name.to_owned());
    }
    for required in ["sa2-chao-online.dll", "mod.ini", "mod.build", "mod.sha256"] {
        if !seen.contains(required) {
            return Err(format!("update is missing {required}").into());
        }
    }
    if fs::read_to_string(stage.join("mod.build"))?.trim() != release.build {
        return Err("release changed during download; restart to try again".into());
    }
    if !fs::read(stage.join("sa2-chao-online.dll"))?.starts_with(b"MZ") {
        return Err("update doesn't contain a Windows DLL".into());
    }
    if !dll_matches(stage) {
        return Err("the mod DLL doesn't match the packaged checksum".into());
    }
    #[cfg(unix)]
    if seen.contains("sa2co-launcher") {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            stage.join("sa2co-launcher"),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    fs::write(stage.join("mod.version"), format!("{}\n", release.date))?;
    files.push("mod.version".into());
    // Mark the build only after every other file has been installed.
    files.sort_by_key(|name| name == "mod.build");
    Ok(files)
}

/// Back up the old files before changing anything, then replace each completed file. Roll
/// back if a locked DLL or another write error prevents completing the entire update.
fn install(mod_dir: &Path, stage: &Path, files: &[String]) -> Result<()> {
    let backup = stage.join("previous");
    fs::create_dir(&backup)?;
    let mut existing = HashSet::new();
    for name in files {
        let dest = mod_dir.join(name);
        if dest.is_symlink() || (dest.exists() && !dest.is_file()) {
            return Err(format!("{} isn't a regular mod file", dest.display()).into());
        }
        if dest.exists() {
            fs::copy(&dest, backup.join(name))?;
            existing.insert(name);
        }
    }
    let mut changed: Vec<&String> = Vec::new();
    for name in files {
        if let Err(error) = fs::rename(stage.join(name), mod_dir.join(name)) {
            let mut errors = Vec::new();
            for previous in changed.into_iter().rev() {
                let result = if existing.contains(previous) {
                    fs::rename(backup.join(previous), mod_dir.join(previous))
                } else {
                    fs::remove_file(mod_dir.join(previous))
                };
                if let Err(e) = result {
                    errors.push(e.to_string());
                }
            }
            return Err(format!(
                "couldn't install {name}: {error}. {}",
                if errors.is_empty() {
                    "The old mod was restored. Close the game before updating.".to_owned()
                } else {
                    format!(
                        "Recovery files remain at {}: {}",
                        backup.display(),
                        errors.join("; ")
                    )
                }
            )
            .into());
        }
        changed.push(name);
    }
    Ok(())
}

fn update(mod_dir: &Path) -> Result<()> {
    let _lock = update_lock(mod_dir)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let stage = mod_dir.join(format!(".sa2co-update-{}-{nonce}", std::process::id()));
    fs::create_dir(&stage)?;
    let result = (|| -> Result<()> {
        let metadata = stage.join("release.json");
        fetch(API, &metadata, 2 << 20)?;
        let release = Release::parse(&fs::read(metadata)?)?;
        if fs::read_to_string(mod_dir.join("mod.build")).is_ok_and(|s| s.trim() == release.build)
            && dll_matches(mod_dir)
        {
            log(mod_dir, &format!("Already current: {}", release.build));
            return Ok(());
        }
        log(mod_dir, &format!("Installing {}", release.build));
        let archive = stage.join("update.zip");
        fetch(&release.url, &archive, MAX_DOWNLOAD)?;
        let files = unpack(&archive, &stage, &release)?;
        install(mod_dir, &stage, &files)?;
        log(mod_dir, &format!("Installed {}", release.build));
        Ok(())
    })();
    // Keep backups after a failed install for recovery; they contain only mod program files.
    if result.is_ok() || !stage.join("previous").exists() {
        let _ = fs::remove_dir_all(stage);
    }
    result
}

fn update_lock(mod_dir: &Path) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options
        .open(mod_dir.join(".sa2co-update.lock"))
        .map_err(|e| format!("couldn't lock the updater (another launch may be updating): {e}"))?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        // Kernel releases the lock even if the updater crashes; the lock file can stay.
        if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
            return Err("another launch is already updating; wait for it to finish".into());
        }
    }
    Ok(file)
}

fn dll_matches(mod_dir: &Path) -> bool {
    let Ok(expected) = fs::read_to_string(mod_dir.join("mod.sha256")) else {
        return false;
    };
    let Ok(bytes) = fs::read(mod_dir.join("sa2-chao-online.dll")) else {
        return false;
    };
    expected.trim() == format!("{:x}", Sha256::digest(bytes))
}

fn log(mod_dir: &Path, message: &str) {
    eprintln!("{message}");
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(mod_dir.join("auto-update.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}

fn run() -> Result<i32> {
    let exe = std::env::current_exe()?;
    let mod_dir = exe.parent().ok_or("launcher folder is unavailable")?;
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // Windows locks running executables. Run a bounded, reusable worker copy so an update
    // can replace the actual launcher too. The worker keeps Steam's inherited environment.
    #[cfg(windows)]
    {
        if args.first().is_some_and(|s| s == "--worker") {
            args.remove(0);
            std::thread::sleep(std::time::Duration::from_millis(500));
        } else {
            let temp = mod_dir.join(".sa2co-launcher-new.exe");
            let worker = mod_dir.join(".sa2co-launcher-running.exe");
            fs::copy(&exe, &temp)?;
            fs::rename(&temp, &worker)
                .map_err(|e| format!("close the previous game/launcher first: {e}"))?;
            Command::new(worker).arg("--worker").args(args).spawn()?;
            return Ok(0);
        }
    }
    update(mod_dir).inspect_err(|error| log(mod_dir, &format!("Update failed: {error}")))?;
    if args.first().is_some_and(|s| s == "--update-only") {
        return Ok(0);
    }
    let game_dir = mod_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("put the launcher in the game's mods/sa2-chao-online folder")?;
    if args.is_empty() {
        args.push(game_dir.join("sonic2app.exe").into_os_string());
    }
    // Steam normally points at the stock settings launcher. Keep Steam's command/env but
    // run the already-configured modded game directly after the automatic update.
    for argument in &mut args {
        let path = Path::new(argument);
        if path
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("Launcher.exe"))
        {
            *argument = path.with_file_name("sonic2app.exe").into_os_string();
        }
    }
    let mut game = Command::new(&args[0]);
    game.args(&args[1..]).current_dir(game_dir);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(game.exec().into())
    }
    #[cfg(not(unix))]
    Ok(game.status()?.code().unwrap_or(1))
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            let message = format!(
                "SA2 Chao Online couldn't update: {error}\n\nThe game wasn't started. Retry after closing the game and checking your internet connection."
            );
            eprintln!("{message}");
            #[cfg(windows)]
            unsafe {
                #[link(name = "user32")]
                unsafe extern "system" {
                    fn MessageBoxW(
                        window: usize,
                        text: *const u16,
                        title: *const u16,
                        kind: u32,
                    ) -> i32;
                }
                let message: Vec<u16> = message.encode_utf16().chain([0]).collect();
                let title: Vec<u16> = "SA2 Chao Online update".encode_utf16().chain([0]).collect();
                MessageBoxW(0, message.as_ptr(), title.as_ptr(), 0x10);
            }
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    const BUILD: &str = "1234567890abcdef1234567890abcdef12345678";

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("sa2-updater-{}-{nonce}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn directory(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir(&path).unwrap();
            path
        }
        fn archive(&self, extra: &[(&str, &[u8])]) -> PathBuf {
            let path = self.0.join("release.zip");
            let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            for (name, bytes) in [
                (
                    "sa2-chao-online/sa2-chao-online.dll",
                    b"MZnew DLL".as_slice(),
                ),
                (
                    "sa2-chao-online/mod.ini",
                    b"Name=SA2 Chao Online".as_slice(),
                ),
                ("sa2-chao-online/mod.build", BUILD.as_bytes()),
            ]
            .into_iter()
            .chain(extra.iter().copied())
            {
                writer
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer
                .start_file("sa2-chao-online/mod.sha256", SimpleFileOptions::default())
                .unwrap();
            write!(writer, "{:x}", Sha256::digest(b"MZnew DLL")).unwrap();
            writer.finish().unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn release() -> Release {
        Release {
            build: BUILD.into(),
            url: format!("{DOWNLOAD}latest/{ASSET}"),
            date: "2026-10-19T06:00:00Z".into(),
            digest: None,
        }
    }

    #[test]
    fn release_requires_an_exact_build_and_our_repository_asset() {
        let mut json = serde_json::json!({
            "target_commitish": BUILD,
            "assets": [{"name": ASSET, "browser_download_url": format!("{DOWNLOAD}latest/{ASSET}"), "updated_at": "2026-10-19T06:00:00Z"}]
        });
        assert_eq!(
            Release::parse(&serde_json::to_vec(&json).unwrap())
                .unwrap()
                .build,
            BUILD
        );
        json["assets"][0]["browser_download_url"] = "https://example.com/unexpected.zip".into();
        assert!(Release::parse(&serde_json::to_vec(&json).unwrap()).is_err());
        json["target_commitish"] = "main".into();
        assert!(Release::parse(&serde_json::to_vec(&json).unwrap()).is_err());
    }

    #[test]
    fn updating_preserves_player_settings_skins_and_chao_files() {
        let f = Fixture::new();
        let installed = f.directory("mod");
        let stage = f.directory("stage");
        fs::write(installed.join("sa2-chao-online.dll"), b"MZold DLL").unwrap();
        fs::write(installed.join("config.ini"), b"player settings").unwrap();
        fs::create_dir(installed.join("skins")).unwrap();
        fs::write(installed.join("skins/custom"), b"custom skin").unwrap();
        fs::create_dir(installed.join("chao")).unwrap();
        fs::write(installed.join("chao/my-chao.sa2chao"), b"player Chao").unwrap();
        let archive = f.archive(&[
            ("sa2-chao-online/config.ini", b"must not overwrite settings"),
            (
                "sa2-chao-online/chao/my-chao.sa2chao",
                b"must not overwrite Chao",
            ),
        ]);
        let files = unpack(&archive, &stage, &release()).unwrap();
        assert_eq!(files.last().unwrap(), "mod.build");
        install(&installed, &stage, &files).unwrap();
        assert_eq!(
            fs::read(installed.join("sa2-chao-online.dll")).unwrap(),
            b"MZnew DLL"
        );
        assert_eq!(
            fs::read(installed.join("config.ini")).unwrap(),
            b"player settings"
        );
        assert_eq!(
            fs::read(installed.join("skins/custom")).unwrap(),
            b"custom skin"
        );
        assert_eq!(
            fs::read(installed.join("chao/my-chao.sa2chao")).unwrap(),
            b"player Chao"
        );
        assert_eq!(
            fs::read_to_string(installed.join("mod.build")).unwrap(),
            BUILD
        );
    }

    #[test]
    fn a_failed_install_restores_files_already_replaced() {
        let f = Fixture::new();
        let installed = f.directory("mod");
        let stage = f.directory("stage");
        fs::write(installed.join("sa2-chao-online.dll"), b"old DLL").unwrap();
        fs::write(installed.join("mod.ini"), b"old metadata").unwrap();
        fs::write(stage.join("sa2-chao-online.dll"), b"new DLL").unwrap();
        // Simulate a failure after the first replacement: the second staged file is gone.
        let error = install(
            &installed,
            &stage,
            &["sa2-chao-online.dll".into(), "mod.ini".into()],
        )
        .unwrap_err();
        assert!(error.to_string().contains("old mod was restored"));
        assert_eq!(
            fs::read(installed.join("sa2-chao-online.dll")).unwrap(),
            b"old DLL"
        );
        assert_eq!(
            fs::read(installed.join("mod.ini")).unwrap(),
            b"old metadata"
        );
    }

    #[test]
    fn a_racing_release_or_bad_checksum_is_never_installed() {
        let f = Fixture::new();
        let stage = f.directory("stage");
        let archive = f.archive(&[]);
        let mut r = release();
        r.build = "ffffffffffffffffffffffffffffffffffffffff".into();
        assert!(
            unpack(&archive, &stage, &r)
                .unwrap_err()
                .to_string()
                .contains("release changed")
        );
        r.build = BUILD.into();
        r.digest = Some("sha256:wrong".into());
        assert!(
            unpack(&archive, &stage, &r)
                .unwrap_err()
                .to_string()
                .contains("checksum")
        );
        r.digest = Some(format!(
            "sha256:{:x}",
            Sha256::digest(fs::read(&archive).unwrap())
        ));
        assert!(unpack(&archive, &stage, &r).is_ok());
    }

    #[test]
    fn archive_paths_cannot_escape_the_mod_folder() {
        let f = Fixture::new();
        let stage = f.directory("stage");
        let archive = f.archive(&[("sa2-chao-online/../../outside", b"bad")]);
        assert!(unpack(&archive, &stage, &release()).is_err());
        assert!(!f.0.join("outside").exists());
    }

    #[test]
    fn directory_targets_are_rejected_before_any_file_changes() {
        let f = Fixture::new();
        let installed = f.directory("mod");
        let stage = f.directory("stage");
        fs::write(installed.join("sa2-chao-online.dll"), b"old DLL").unwrap();
        fs::create_dir(installed.join("mod.ini")).unwrap();
        fs::write(stage.join("sa2-chao-online.dll"), b"new DLL").unwrap();
        assert!(
            install(
                &installed,
                &stage,
                &["sa2-chao-online.dll".into(), "mod.ini".into()]
            )
            .is_err()
        );
        assert_eq!(
            fs::read(installed.join("sa2-chao-online.dll")).unwrap(),
            b"old DLL"
        );
    }

    #[test]
    fn a_build_marker_alone_cannot_hide_a_stale_or_damaged_dll() {
        let f = Fixture::new();
        fs::write(
            f.0.join("mod.sha256"),
            format!("{:x}\n", Sha256::digest(b"MZnew DLL")),
        )
        .unwrap();
        assert!(!dll_matches(&f.0));
        fs::write(f.0.join("sa2-chao-online.dll"), b"MZnew DLL").unwrap();
        assert!(dll_matches(&f.0));
        fs::write(f.0.join("sa2-chao-online.dll"), b"MZold DLL").unwrap();
        assert!(!dll_matches(&f.0));
    }

    #[test]
    fn concurrent_updates_are_excluded_and_an_old_lock_file_does_not_block_restart() {
        let f = Fixture::new();
        let lock = update_lock(&f.0).unwrap();
        assert!(update_lock(&f.0).is_err());
        drop(lock);
        assert!(update_lock(&f.0).is_ok());
    }
}

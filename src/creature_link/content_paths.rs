//! Where the creature link's configured source game image and private cache are.

use std::path::{Component, Path, PathBuf};

fn normalized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            component => result.push(component.as_os_str()),
        }
    }
    result
}

/// A configured source/cache path: as given when absolute, else from the mod folder. No
/// filesystem I/O.
pub fn resolve_configured(mod_root: &Path, configured: &str) -> Result<PathBuf, String> {
    let configured = configured.trim().trim_matches('"');
    if configured.is_empty() {
        return Err("No local content path is configured".into());
    }
    #[cfg(not(windows))]
    {
        // Linux-side tools must not reinterpret a player's external Windows path as a
        // relative Linux path. The running Windows mod uses native path components.
        let bytes = configured.as_bytes();
        if (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
            || configured.starts_with("\\\\")
        {
            return Ok(PathBuf::from(configured));
        }
    }
    let configured = PathBuf::from(configured);
    if configured.is_absolute() {
        return Ok(configured);
    }
    let root = if mod_root.is_absolute() {
        normalized(mod_root)
    } else {
        normalized(
            &std::env::current_dir()
                .map_err(|error| format!("Cannot resolve the mod folder: {error}"))?
                .join(mod_root),
        )
    };
    Ok(root.join(configured))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_from_the_mod_folder_and_absolute_ones_are_kept() {
        let root = std::env::temp_dir().join("creature-content-paths");
        assert_eq!(
            resolve_configured(&root, "creature/source.3ds").unwrap(),
            root.join("creature/source.3ds")
        );
        assert_eq!(
            resolve_configured(&root, "  \"private/cache\"  ").unwrap(),
            root.join("private/cache")
        );
        let external = root.parent().unwrap().join("external/cache");
        assert_eq!(
            resolve_configured(&root, external.to_str().unwrap()).unwrap(),
            external
        );
        assert!(resolve_configured(&root, " \"\" ").is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_tools_preserve_external_windows_configurations() {
        let root = std::env::temp_dir().join("creature-content-paths");
        for path in [r"Z:\home\player\game.3ds", r"\\host\share\cache"] {
            assert_eq!(
                resolve_configured(&root, path).unwrap(),
                PathBuf::from(path)
            );
        }
    }
}

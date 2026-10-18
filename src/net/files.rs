//! Shared skins: the files a host (the dedicated server, or a player hosting) hands out, so
//! everyone has the same skins (docs/protocol.md "Skins").
//!
//! The store is a folder of skin folders (`<id>/skin.ini`, `<id>/model.sa2mdl`, ...), read
//! into memory once. Clients ask for its list a page at a time and then for chunks of the files
//! they don't have; nothing here trusts a client beyond the index it asks for.

use std::path::Path;
use std::sync::Arc;

use super::protocol::{
    Body, MAX_SKIN_FILE, MAX_SKIN_FILES, SKIN_CHUNK, SKIN_LIST_PAGE, SkinFile, skin_hash,
    skin_path_ok,
};

/// Everything shared together stays under this.
pub const MAX_STORE_BYTES: usize = 256 << 20;

#[derive(Default)]
pub struct SkinStore {
    files: Vec<(SkinFile, Arc<[u8]>)>,
}

impl SkinStore {
    /// Reads `dir`'s skin folders (paths and sizes within the protocol's limits; anything
    /// else is skipped), sorted by path.
    pub fn scan(dir: &Path) -> Self {
        let mut found = Vec::new();
        walk(dir, dir, 0, &mut found);
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let mut files = Vec::new();
        let mut total = 0usize;
        for (path, full) in found {
            if files.len() >= MAX_SKIN_FILES {
                break;
            }
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            if bytes.len() > MAX_SKIN_FILE as usize || total + bytes.len() > MAX_STORE_BYTES {
                continue;
            }
            total += bytes.len();
            let file = SkinFile {
                size: bytes.len() as u32,
                hash: skin_hash(&bytes),
                path,
            };
            files.push((file, Arc::from(bytes)));
        }
        Self { files }
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.files.iter().map(|(f, _)| f.size as usize).sum()
    }

    /// Page `page` of the list (an empty last page when there are no files).
    pub fn page(&self, page: u16) -> Option<Body> {
        let pages = self.files.len().div_ceil(SKIN_LIST_PAGE) as u16;
        if page >= pages.max(1) {
            return None;
        }
        let start = usize::from(page) * SKIN_LIST_PAGE;
        let files = self
            .files
            .iter()
            .skip(start)
            .take(SKIN_LIST_PAGE)
            .map(|(f, _)| f.clone())
            .collect();
        Some(Body::SkinList { page, pages, files })
    }

    /// The chunk of file `file` from `offset`, if there is one.
    pub fn chunk(&self, file: u16, offset: u32) -> Option<Body> {
        let (_, bytes) = self.files.get(usize::from(file))?;
        let start = offset as usize;
        if start >= bytes.len() {
            return None;
        }
        let end = (start + SKIN_CHUNK).min(bytes.len());
        Some(Body::SkinChunk {
            file,
            offset,
            data: bytes[start..end].to_vec(),
        })
    }
}

/// Files under `dir` as `<id>/<...>` paths (plain names only; at most a few folders deep).
fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, std::path::PathBuf)>) {
    if depth > 3 || out.len() > MAX_SKIN_FILES * 2 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let full = e.path();
        let Ok(kind) = e.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(root, &full, depth + 1, out);
        } else if kind.is_file()
            && let Ok(rel) = full.strip_prefix(root)
            && let Some(rel) = rel.to_str()
        {
            let path = rel.replace('\\', "/");
            if skin_path_ok(&path) {
                out.push((path, full));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_lists_and_hands_out_its_files() {
        let dir = std::env::temp_dir().join(format!("sa2co-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("big/sounds/jump")).unwrap();
        std::fs::create_dir_all(dir.join("Bad Name")).unwrap();
        let big: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
        std::fs::write(dir.join("big/model.sa2mdl"), &big).unwrap();
        std::fs::write(dir.join("big/sounds/jump/whoa.wav"), b"RIFF").unwrap();
        std::fs::write(dir.join("Bad Name/x"), b"no").unwrap();
        std::fs::write(dir.join("big/.hidden"), b"no").unwrap();
        let s = SkinStore::scan(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(s.len(), 2);
        let Some(Body::SkinList { page, pages, files }) = s.page(0) else {
            panic!();
        };
        assert_eq!((page, pages), (0, 1));
        assert_eq!(files[0].path, "big/model.sa2mdl");
        assert_eq!(files[0].hash, skin_hash(&big));
        assert!(s.page(1).is_none());
        // The whole file, chunk by chunk.
        let mut got = Vec::new();
        while let Some(Body::SkinChunk { data, .. }) = s.chunk(0, got.len() as u32) {
            got.extend(data);
        }
        assert_eq!(got, big);
        assert!(s.chunk(5, 0).is_none());
    }
}

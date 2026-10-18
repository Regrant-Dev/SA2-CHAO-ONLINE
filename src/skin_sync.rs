//! Shared skins, our side: on joining, the host's skin list is compared with our `skins/`
//! folder and whatever is new or changed comes down in the background, a chunk at a time
//! (net/files.rs is the host's side; docs/protocol.md "Skins"). Nothing shows on screen but
//! the progress bar on the main menu (`progress`); finished skins load without a restart.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::log;
use crate::net::protocol::{Body, SKIN_CHUNK, SKIN_LIST_PAGE, SkinFile, skin_hash};

/// Chunk requests out at once, and when one counts as lost.
const WINDOW: usize = 64;
const CHUNK_TIMEOUT: Duration = Duration::from_millis(400);
/// A list page that doesn't come: asked again this often, this many times (a host without
/// skins never answers).
const LIST_RETRY: Duration = Duration::from_secs(2);
const LIST_TRIES: u32 = 5;
/// A file that stops coming at all is left for next time.
const FILE_STALL: Duration = Duration::from_secs(20);

/// Download progress (bytes) for the menu bar, and "skins changed: look again".
static DONE: AtomicU64 = AtomicU64::new(0);
static TOTAL: AtomicU64 = AtomicU64::new(0);
static CHANGED: AtomicBool = AtomicBool::new(false);

/// (bytes done, bytes in all) while skins are coming down.
pub fn progress() -> Option<(u64, u64)> {
    let (done, total) = (DONE.load(Ordering::Relaxed), TOTAL.load(Ordering::Relaxed));
    (total > 0 && done < total).then_some((done, total))
}

/// Whether new skins arrived since last asked (the game thread then reloads the list).
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

struct Fetch {
    index: u16,
    file: SkinFile,
    buf: Vec<u8>,
    have: Vec<bool>,
    /// Next chunk not yet asked for, and the asked ones still out (chunk -> when).
    next: usize,
    out: HashMap<usize, Instant>,
    last_progress: Instant,
}

enum Phase {
    Idle,
    Listing {
        page: u16,
        asked: Instant,
        tries: u32,
        files: Vec<SkinFile>,
    },
    Fetching {
        queue: VecDeque<(u16, SkinFile)>,
        current: Option<Fetch>,
        wrote: usize,
    },
}

pub struct SkinSync {
    dir: PathBuf,
    phase: Phase,
}

impl SkinSync {
    /// Keeps `<mod>/skins` in step with the host's.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            phase: Phase::Idle,
        }
    }

    /// Just connected: ask for the list.
    pub fn start(&mut self, now: Instant) -> Vec<Body> {
        self.phase = Phase::Listing {
            page: 0,
            asked: now,
            tries: 1,
            files: Vec::new(),
        };
        vec![Body::SkinListGet { page: 0 }]
    }

    /// Disconnected: stop (what's done stays done).
    pub fn stop(&mut self) {
        self.phase = Phase::Idle;
        TOTAL.store(0, Ordering::Relaxed);
        DONE.store(0, Ordering::Relaxed);
    }

    /// A list page or a chunk from the host. Returns what to send.
    pub fn on_body(&mut self, body: &Body, now: Instant) -> Vec<Body> {
        match (body, &mut self.phase) {
            (
                Body::SkinList {
                    page: got,
                    pages,
                    files: more,
                },
                Phase::Listing {
                    page,
                    asked,
                    tries,
                    files,
                },
            ) if got == page => {
                files.extend(more.iter().cloned());
                if *page + 1 < *pages && more.len() == SKIN_LIST_PAGE {
                    *page += 1;
                    *asked = now;
                    *tries = 1;
                    return vec![Body::SkinListGet { page: *page }];
                }
                let files = std::mem::take(files);
                self.plan(files, now)
            }
            (Body::SkinChunk { file, offset, data }, Phase::Fetching { current, .. }) => {
                if let Some(f) = current
                    && f.index == *file
                    && (*offset as usize).is_multiple_of(SKIN_CHUNK)
                {
                    let chunk = *offset as usize / SKIN_CHUNK;
                    let start = *offset as usize;
                    let expect = (f.buf.len() - start.min(f.buf.len())).min(SKIN_CHUNK);
                    if chunk < f.have.len() && !f.have[chunk] && data.len() == expect {
                        f.buf[start..start + expect].copy_from_slice(data);
                        f.have[chunk] = true;
                        f.out.remove(&chunk);
                        f.last_progress = now;
                        DONE.fetch_add(expect as u64, Ordering::Relaxed);
                    }
                }
                self.tick(now)
            }
            _ => Vec::new(),
        }
    }

    /// Every poll: lost requests again, more requests out, finished files written.
    pub fn tick(&mut self, now: Instant) -> Vec<Body> {
        let mut send = Vec::new();
        match &mut self.phase {
            Phase::Idle => {}
            Phase::Listing {
                page, asked, tries, ..
            } => {
                if now.duration_since(*asked) >= LIST_RETRY {
                    if *tries >= LIST_TRIES {
                        // This host shares no skins.
                        self.phase = Phase::Idle;
                    } else {
                        *tries += 1;
                        *asked = now;
                        send.push(Body::SkinListGet { page: *page });
                    }
                }
            }
            Phase::Fetching {
                queue,
                current,
                wrote,
            } => {
                if current.is_none() {
                    *current = queue.pop_front().map(|(index, file)| Fetch {
                        index,
                        buf: vec![0; file.size as usize],
                        have: vec![false; (file.size as usize).div_ceil(SKIN_CHUNK)],
                        file,
                        next: 0,
                        out: HashMap::new(),
                        last_progress: now,
                    });
                }
                let Some(f) = current else {
                    // All done.
                    log!("skins: {wrote} shared file(s) updated");
                    if *wrote > 0 {
                        CHANGED.store(true, Ordering::Relaxed);
                    }
                    TOTAL.store(0, Ordering::Relaxed);
                    DONE.store(0, Ordering::Relaxed);
                    self.phase = Phase::Idle;
                    return send;
                };
                if f.have.iter().all(|&h| h) {
                    if skin_hash(&f.buf) == f.file.hash && write(&self.dir, &f.file.path, &f.buf) {
                        *wrote += 1;
                    } else {
                        log!("skins: {} didn't come down right; next time", f.file.path);
                    }
                    *current = None;
                    return send;
                }
                if now.duration_since(f.last_progress) > FILE_STALL {
                    log!("skins: {} stalled; next time", f.file.path);
                    let left = f.have.iter().filter(|&&h| !h).count() * SKIN_CHUNK;
                    DONE.fetch_add(left.min(f.buf.len()) as u64, Ordering::Relaxed);
                    *current = None;
                    return send;
                }
                // Lost ones again, then new ones up to the window.
                let lost: Vec<usize> = f
                    .out
                    .iter()
                    .filter(|(_, at)| now.duration_since(**at) >= CHUNK_TIMEOUT)
                    .map(|(&c, _)| c)
                    .collect();
                for c in lost {
                    f.out.insert(c, now);
                    send.push(chunk_request(f.index, c));
                }
                while f.out.len() < WINDOW && f.next < f.have.len() {
                    let c = f.next;
                    f.next += 1;
                    if !f.have[c] {
                        f.out.insert(c, now);
                        send.push(chunk_request(f.index, c));
                    }
                }
            }
        }
        send
    }

    /// The host's list in full: fetch what we don't have the same of.
    fn plan(&mut self, files: Vec<SkinFile>, now: Instant) -> Vec<Body> {
        let queue: VecDeque<(u16, SkinFile)> = files
            .into_iter()
            .enumerate()
            .filter(|(_, f)| !same_here(&self.dir, f))
            .map(|(i, f)| (i as u16, f))
            .collect();
        let total: u64 = queue.iter().map(|(_, f)| u64::from(f.size)).sum();
        log!(
            "skins: {} shared file(s) to fetch ({:.1} MB)",
            queue.len(),
            total as f64 / 1e6
        );
        DONE.store(0, Ordering::Relaxed);
        TOTAL.store(total, Ordering::Relaxed);
        self.phase = Phase::Fetching {
            queue,
            current: None,
            wrote: 0,
        };
        self.tick(now)
    }
}

fn chunk_request(file: u16, chunk: usize) -> Body {
    Body::SkinGet {
        file,
        offset: (chunk * SKIN_CHUNK) as u32,
    }
}

/// Whether our copy of a shared file is the same (size, then checksum).
fn same_here(dir: &Path, f: &SkinFile) -> bool {
    let path = dir.join(&f.path);
    std::fs::metadata(&path).is_ok_and(|m| m.len() == u64::from(f.size))
        && std::fs::read(&path).is_ok_and(|b| skin_hash(&b) == f.hash)
}

/// Writes a finished file (a checked `<id>/<parts>` path) whole: to a side file, then renamed.
fn write(dir: &Path, path: &str, bytes: &[u8]) -> bool {
    let full = dir.join(path);
    let part = full.with_extension("part");
    let ok = full
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
        && std::fs::write(&part, bytes).is_ok()
        && std::fs::rename(&part, &full).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&part);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::files::SkinStore;

    /// A whole sync against a store, losing every 7th chunk the first time it's asked for.
    #[test]
    fn a_sync_fetches_what_differs() {
        let base = std::env::temp_dir().join(format!("sa2co-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (host, mine) = (base.join("host"), base.join("mine"));
        std::fs::create_dir_all(host.join("big/sounds")).unwrap();
        std::fs::create_dir_all(mine.join("big")).unwrap();
        let model: Vec<u8> = (0..5000u32).map(|i| (i * 7) as u8).collect();
        std::fs::write(host.join("big/model.sa2mdl"), &model).unwrap();
        std::fs::write(host.join("big/skin.ini"), b"name=Big").unwrap();
        std::fs::write(host.join("big/sounds/a.wav"), b"RIFFxxxx").unwrap();
        // Already have the same skin.ini; an older model.
        std::fs::write(mine.join("big/skin.ini"), b"name=Big").unwrap();
        std::fs::write(mine.join("big/model.sa2mdl"), b"old").unwrap();
        let store = SkinStore::scan(&host);
        let mut sync = SkinSync::new(mine.clone());
        let mut now = Instant::now();
        let mut out = sync.start(now);
        let mut asked: HashMap<(u16, u32), u32> = HashMap::new();
        for _ in 0..2000 {
            let mut replies = Vec::new();
            for b in out.drain(..) {
                match b {
                    Body::SkinListGet { page } => replies.extend(store.page(page)),
                    Body::SkinGet { file, offset } => {
                        let n = asked.entry((file, offset)).or_default();
                        *n += 1;
                        if !(offset / SKIN_CHUNK as u32).is_multiple_of(7) || *n > 1 {
                            replies.extend(store.chunk(file, offset));
                        }
                    }
                    _ => {}
                }
            }
            for r in replies {
                out.extend(sync.on_body(&r, now));
            }
            now += Duration::from_millis(50);
            out.extend(sync.tick(now));
            if matches!(sync.phase, Phase::Idle) {
                break;
            }
        }
        assert!(matches!(sync.phase, Phase::Idle));
        assert_eq!(std::fs::read(mine.join("big/model.sa2mdl")).unwrap(), model);
        assert_eq!(
            std::fs::read(mine.join("big/sounds/a.wav")).unwrap(),
            b"RIFFxxxx"
        );
        // skin.ini was the same: never asked for.
        assert!(!asked.keys().any(|(f, _)| *f == 1));
        assert!(take_changed());
        let _ = std::fs::remove_dir_all(&base);
    }
}

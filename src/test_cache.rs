//! The local creature catalog for tests that check against real data: `SA2CO_TEST_CACHE`, a
//! private cache directory (or its catalog file). Without it those tests are skipped and say so.

use std::path::PathBuf;

/// `SA2CO_TEST_CACHE` as a path, or none (the test returns early) when it isn't set or isn't
/// there.
pub(crate) fn path() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("SA2CO_TEST_CACHE") else {
        println!("skipped: SA2CO_TEST_CACHE not set");
        return None;
    };
    let path = PathBuf::from(path);
    if !path.exists() {
        println!("skipped: no local catalog at {}", path.display());
        return None;
    }
    Some(path)
}

/// The local catalog opened, or none (skipped, as `path` says) without one.
pub(crate) fn rom() -> Option<crate::creature::rom::Rom> {
    Some(crate::creature::rom::Rom::open(&path()?).expect("the local catalog opens"))
}

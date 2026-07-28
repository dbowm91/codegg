//! Debug-build failpoints for cross-process recovery tests.
//!
//! Normal and release builds never alter execution. A debug process pauses
//! only when both the requested point and a marker path are explicitly set.

#[cfg(debug_assertions)]
pub(crate) fn hit(name: &str) {
    if std::env::var("CODEGG_TEST_FAILPOINT").ok().as_deref() != Some(name) {
        return;
    }
    let Some(path) = std::env::var_os("CODEGG_TEST_FAILPOINT_MARKER") else {
        return;
    };
    if let Ok(file) = std::fs::File::create(path) {
        let _ = file.sync_all();
    }
    loop {
        std::thread::park_timeout(std::time::Duration::from_secs(1));
    }
}

#[cfg(not(debug_assertions))]
#[inline]
pub(crate) fn hit(_name: &str) {}

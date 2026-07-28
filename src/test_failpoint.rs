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

/// Whether this debug process was explicitly launched as a cross-process
/// recovery fixture. Both variables are process-owner capabilities; a
/// protocol client cannot enable this mode.
#[cfg(debug_assertions)]
pub(crate) fn recovery_fixture_enabled() -> bool {
    std::env::var_os("CODEGG_TEST_RECOVERY_FIXTURE").is_some()
        && (std::env::var_os("CODEGG_TEST_FAILPOINT").is_none()
            || std::env::var_os("CODEGG_TEST_FAILPOINT_MARKER").is_some())
}

#[cfg(not(debug_assertions))]
#[inline]
pub(crate) fn hit(_name: &str) {}

#[cfg(not(debug_assertions))]
#[inline]
pub(crate) fn recovery_fixture_enabled() -> bool {
    false
}

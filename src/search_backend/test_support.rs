//! Shared test support for `search_backend` integration tests.
//!
//! `search_backend::state` holds a process-global `McpService` and
//! `SearchConfig` slot. Integration tests across multiple test files
//! (and across multiple test binaries) mutate those slots, so they
//! must serialize against each other. Each test binary is a separate
//! process, so per-process `tokio::sync::Mutex` instances cannot
//! protect cross-binary races. This module exposes a process-wide
//! `flock`-based lock that all integration tests must acquire before
//! touching the global state.

use std::path::PathBuf;

use tokio::sync::Mutex;

/// In-process `tokio::sync::Mutex` that serializes tests within a
/// single binary. Held across `.await` boundaries while the
/// cross-process lock is also held. Tests must acquire BOTH locks.
pub static SHARED_TEST_LOCK: Mutex<()> = Mutex::const_new(());

/// RAII guard returned by [`acquire_cross_process_lock`]. When
/// dropped, the underlying flock is released.
pub struct CrossProcessLockGuard {
    _file: std::fs::File,
    _path: PathBuf,
}

#[cfg(unix)]
#[allow(unsafe_code)]
mod imp {
    use super::CrossProcessLockGuard;
    use std::os::unix::io::AsRawFd;
    use std::path::PathBuf;

    /// Acquire a process-wide advisory lock. Blocks until acquired.
    /// Uses `flock(2)` on a temp file so every test binary started by
    /// the same `cargo test` invocation serializes on the same file.
    pub fn acquire() -> CrossProcessLockGuard {
        use std::os::unix::fs::OpenOptionsExt;
        let path: PathBuf = std::env::temp_dir().join("codegg-search-backend-test.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(&path)
            .expect("open cross-process test lock file");
        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if ret != 0 {
            panic!("flock LOCK_EX failed: {}", std::io::Error::last_os_error());
        }
        CrossProcessLockGuard {
            _file: file,
            _path: path,
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::CrossProcessLockGuard;
    /// On non-unix platforms fall back to a no-op guard. CI runs on
    /// Linux, so the cross-process race only matters there.
    pub fn acquire() -> CrossProcessLockGuard {
        CrossProcessLockGuard {
            _file: std::fs::File::open("/dev/null").expect("open /dev/null"),
            _path: std::path::PathBuf::new(),
        }
    }
}

/// Acquire the cross-process flock. Blocks until the lock is held.
pub fn acquire_cross_process_lock() -> CrossProcessLockGuard {
    imp::acquire()
}

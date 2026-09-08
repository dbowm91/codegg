//! Shared test support for `search_backend` bootstrap compat tests.
//!
//! M005 migrated execution-path tests to isolated
//! `SearchRuntimeContext` values, which need no serialization. Only the
//! bootstrap tests (which assert on the legacy process-global install
//! slots retained for daemon connection reuse) still mutate global
//! state. Those tests — across multiple test files and binaries — must
//! serialize against each other: each test binary is a separate
//! process, so per-process `tokio::sync::Mutex` instances cannot
//! protect cross-binary races. This module exposes a process-wide
//! `flock`-based lock that those compat tests must acquire before
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
    _file: std::sync::Arc<std::fs::File>,
    _path: PathBuf,
}

#[cfg(unix)]
#[allow(unsafe_code)]
mod imp {
    use super::CrossProcessLockGuard;
    use std::path::PathBuf;

    /// Acquire a process-wide advisory lock. Blocks until acquired.
    /// Uses `flock(2)` on a file under `target/` (per-build) so every
    /// test binary started by the same `cargo test` invocation
    /// serializes on the same file. `target/` is owned by the build
    /// and is reliably writable on every platform/CI.
    pub fn acquire() -> CrossProcessLockGuard {
        use std::os::fd::{FromRawFd, IntoRawFd};
        use std::os::unix::fs::OpenOptionsExt;
        let path: PathBuf = std::env::var("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("target"))
            .join("codegg-search-backend-test.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .custom_flags(libc::O_CLOEXEC)
            .open(&path)
            .expect("open cross-process test lock file");
        let fd = file.into_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            panic!("flock LOCK_EX failed: {err}");
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        CrossProcessLockGuard {
            _file: std::sync::Arc::new(file),
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
            _file: std::sync::Arc::new(std::fs::File::open("/dev/null").expect("open /dev/null")),
            _path: std::path::PathBuf::new(),
        }
    }
}

/// Acquire the cross-process flock. Blocks until the lock is held.
pub fn acquire_cross_process_lock() -> CrossProcessLockGuard {
    imp::acquire()
}

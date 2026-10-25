//! An isolated process environment for integration tests.
//!
//! Some product behaviour is rooted at the user's **cache** directory — the
//! managed scratch tree is one — so an integration test that exercises it
//! must not write into the developer's real one. The product resolves that
//! directory through `dirs::cache_dir()`, which reads `HOME` (and
//! `XDG_CACHE_HOME` on Linux), so an isolated `HOME` is enough to move the
//! whole tree.
//!
//! The environment is **process-global**: two tests that isolate it at the
//! same time would read each other's `HOME`. Every member that moves the
//! environment takes [`env_lock`], including the ones that do their own
//! juggling, so the isolation is exclusive across the whole test binary.

// Every integration test binary that declares `mod common;` compiles this
// module, but only the ones that isolate the environment use it. The
// shared-module convention (see `pool.rs`) is to silence that here rather
// than in each consumer.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::Path;
use std::sync::OnceLock;

use parking_lot::{Mutex, MutexGuard};

/// The process-wide lock that serialises environment mutation.
///
/// The environment is process-global, so the isolation is only exclusive
/// while this lock is held. Every member that moves the environment takes
/// it, whether it uses [`IsolatedEnv`] or does its own juggling.
pub fn env_lock() -> MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(())).lock()
}

/// An isolated environment: its own `HOME` (so the scratch cache root is
/// isolated), its own daemon home, and its own data home.
///
/// The XDG variables are isolated too. On Linux they are what `dirs` reads
/// for the cache, config, data, and state roots, and a runner that sets
/// them would otherwise give two tests in this binary the **same** config
/// directory — so a config file one test wrote would silently change what
/// another test's daemon does.
pub struct IsolatedEnv {
    _lock: MutexGuard<'static, ()>,
    old_home: Option<OsString>,
    old_daemon_home: Option<OsString>,
    old_data_home: Option<OsString>,
    old_xdg_cache: Option<OsString>,
    old_xdg_config: Option<OsString>,
    old_xdg_data: Option<OsString>,
    old_xdg_state: Option<OsString>,
}

impl IsolatedEnv {
    /// Isolate the environment under `root`.
    ///
    /// The root is created if it is missing. The previous values are
    /// restored when the guard is dropped.
    pub fn new(root: &Path) -> Self {
        let lock = env_lock();
        let old_home = std::env::var_os("HOME");
        let old_daemon_home = std::env::var_os("CODEGG_DAEMON_HOME");
        let old_data_home = std::env::var_os("CODEGG_DATA_HOME");
        let old_xdg_cache = std::env::var_os("XDG_CACHE_HOME");
        let old_xdg_config = std::env::var_os("XDG_CONFIG_HOME");
        let old_xdg_data = std::env::var_os("XDG_DATA_HOME");
        let old_xdg_state = std::env::var_os("XDG_STATE_HOME");
        let home = root.join("home");
        std::env::set_var("HOME", &home);
        std::env::set_var("CODEGG_DAEMON_HOME", root.join("daemon"));
        std::env::set_var("CODEGG_DATA_HOME", root.join("data"));
        // Every XDG root is redirected into the isolated home, so the daemon
        // resolves its config, cache, data, and state roots the same way the
        // test process does — whatever the runner's environment says.
        for (key, leaf) in [
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
        ] {
            let path = home.join(leaf);
            std::fs::create_dir_all(&path).ok();
            std::env::set_var(key, path);
        }
        std::fs::create_dir_all(&home).ok();
        Self {
            _lock: lock,
            old_home,
            old_daemon_home,
            old_data_home,
            old_xdg_cache,
            old_xdg_config,
            old_xdg_data,
            old_xdg_state,
        }
    }
}

impl Drop for IsolatedEnv {
    fn drop(&mut self) {
        for (key, value) in [
            ("HOME", self.old_home.take()),
            ("CODEGG_DAEMON_HOME", self.old_daemon_home.take()),
            ("CODEGG_DATA_HOME", self.old_data_home.take()),
            ("XDG_CACHE_HOME", self.old_xdg_cache.take()),
            ("XDG_CONFIG_HOME", self.old_xdg_config.take()),
            ("XDG_DATA_HOME", self.old_xdg_data.take()),
            ("XDG_STATE_HOME", self.old_xdg_state.take()),
        ] {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

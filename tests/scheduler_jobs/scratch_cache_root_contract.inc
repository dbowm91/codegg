//! The per-user cache root contract of the scratch allocator.
//!
//! The allocator keeps every session's scratch under the per-user CodeGG cache
//! root (`dirs::cache_dir()/codegg`). These qualifications pin the two halves
//! of that contract the allocator cannot observe from the inside: the
//! **default** resolution and the **explicit** one (an `XDG_CACHE_HOME`).
//!
//! The unsafe-cache-root refusal and root privacy are qualified in
//! `src/scratch.rs`.

use std::path::{Path, PathBuf};

use super::common::isolated_env::env_lock;

/// Restores the environment entries it replaced.
struct EnvGuard(Vec<(&'static str, Option<String>)>);

impl EnvGuard {
    fn install(values: &[(&'static str, Option<String>)]) -> Self {
        let prior = values
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();
        for (key, value) in values {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self(prior)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn session_id(label: &str) -> String {
    format!("scratch-cache-root-{label}-{}", uuid::Uuid::new_v4())
}

/// The CodeGG scratch root the allocator derived from `cache_root`.
fn codegg_scratch_root(cache_root: &Path) -> PathBuf {
    cache_root.join("codegg").join("scratch").join("sessions")
}

#[test]
fn the_default_cache_root_is_the_home_directory_cache() {
    let _guard = env_lock();
    let home = tempfile::tempdir().expect("isolated home");
    // The allocator refuses a path with a symlinked component, and a temporary
    // directory's ancestors can contain one, so the fixture canonicalizes the
    // way the product's own scratch home does.
    let home_path = std::fs::canonicalize(home.path()).expect("canonical home");
    let _env = EnvGuard::install(&[
        ("HOME", Some(home_path.to_string_lossy().into_owned())),
        ("XDG_CACHE_HOME", None),
    ]);

    let session = session_id("default");
    let root = codegg::scratch::session_scratch_dir(&session).expect("session scratch root");

    // The allocator follows the platform's per-user cache convention, so the
    // expected root is resolved the same way.
    let expected_cache = dirs::cache_dir().expect("the platform cache directory");
    assert!(
        root.starts_with(codegg_scratch_root(&expected_cache)),
        "without XDG_CACHE_HOME the scratch root must live under the home directory's cache: {root:?}"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// The explicit cache-home contract is an XDG-platform property: the
/// allocator honours `XDG_CACHE_HOME` on Linux and ignores it on macOS, where
/// the Apple convention applies — so this qualification is a Linux one.
#[cfg(target_os = "linux")]
#[test]
fn an_explicit_cache_home_is_the_root_the_allocator_uses() {
    let _guard = env_lock();
    let cache = tempfile::tempdir().expect("isolated cache");
    // The allocator refuses a cache root another user could write to, and
    // `tempfile` creates its directories with the *default* permissions — so
    // the mode depends on the operator's umask. The fixture makes the root
    // private on every host, the way the product creates the roots it owns.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(cache.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private cache root");
    }
    // The allocator refuses a path with a symlinked component, and a temporary
    // directory's ancestors can contain one, so the fixture canonicalizes the
    // way the product's own scratch home does.
    let cache_path = std::fs::canonicalize(cache.path()).expect("canonical cache");
    let _env = EnvGuard::install(&[(
        "XDG_CACHE_HOME",
        Some(cache_path.to_string_lossy().into_owned()),
    )]);

    let session = session_id("explicit");
    let root = codegg::scratch::session_scratch_dir(&session).expect("session scratch root");

    assert!(
        root.starts_with(codegg_scratch_root(&cache_path)),
        "the scratch root must live under the supplied cache root: {root:?}"
    );
    std::fs::remove_dir_all(&root).ok();
}

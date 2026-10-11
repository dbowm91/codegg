//! Backend-neutral containment policy.
//!
//! A policy is a statement about *paths*, not about any one OS mechanism:
//! what may be read, what may be written, and what must never be touched.
//! Backends compile a [`BackendPolicy`] into their native form (Landlock
//! ruleset, SBPL profile, …).
//!
//! ## Tool compatibility
//!
//! CodeGG's containment has to survive contact with real developer tooling.
//! A deny-first allowlist that exposes only the workspace plus a couple of
//! library directories is *nominally* stronger and *practically* useless:
//! `git` cannot read `/etc/gitconfig`, `cargo` cannot read `~/.cargo`,
//! `gh` cannot read `~/.config/gh`, and nothing can resolve a hostname
//! without `/etc/resolv.conf`. A sandbox that fails every command is not
//! containment, it is denial.
//!
//! Read roots are deliberately curated: shared user configuration and cache
//! ancestors can contain credentials, so only toolchain data needed to run
//! installed tools is included. Explicit denies are carved out of broader
//! allow roots before a deny-first backend sees them. Writes are limited to
//! the workspace, stateless device nodes, and launch-scoped scratch.

use std::path::{Path, PathBuf};

use super::SandboxLaunchSpec;

/// Path sets a backend compiles into its native containment rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendPolicy {
    /// Roots the target may read.
    pub read_roots: Vec<PathBuf>,
    /// Directories that may be listed while their denied descendants remain
    /// excluded from file reads.
    pub read_dir_roots: Vec<PathBuf>,
    /// Roots the target may write. Empty means no writes at all.
    pub write_roots: Vec<PathBuf>,
    /// Roots the target may never read or write.
    ///
    /// Meaningful for backends that allow by default (Seatbelt). Backends
    /// that are deny-first by construction (Landlock) get these for free;
    /// the list is still populated so the policy describes one intent
    /// regardless of which mechanism enforces it.
    pub denied_roots: Vec<PathBuf>,
}

impl BackendPolicy {
    /// Canonicalize, de-duplicate, and drop non-existent grant entries.
    ///
    /// Canonicalization is what makes a policy portable across `/tmp` ->
    /// `/private/tmp` symlinks and per-user worktree aliases; de-duplication
    /// keeps generated profiles bounded when roots overlap.
    fn normalized(roots: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut seen = Vec::new();
        let mut out = Vec::with_capacity(roots.len());
        for root in roots {
            let Ok(canonical) = std::fs::canonicalize(&root) else {
                continue;
            };
            if !seen.contains(&canonical) {
                seen.push(canonical.clone());
                out.push(canonical);
            }
        }
        out.sort();
        out
    }

    /// Build a normalized policy from raw roots.
    pub fn new(read: Vec<PathBuf>, write: Vec<PathBuf>, denied: Vec<PathBuf>) -> Self {
        let denied_roots = Self::normalized_denies(denied);
        Self {
            read_roots: Self::normalized(read),
            read_dir_roots: Vec::new(),
            write_roots: Self::normalized(write),
            denied_roots,
        }
    }

    /// Compile the requested sets into the closest Landlock allowlist. Deny
    /// descendants are removed from file grants; directory-listing rights on
    /// their ancestors are retained separately. Seatbelt uses the requested
    /// roots directly and applies its explicit final deny rules.
    pub fn landlock_effective(&self) -> Self {
        let (read_roots, read_dir_roots) =
            Self::carve_read_denied(self.read_roots.clone(), &self.denied_roots);
        Self {
            read_roots,
            read_dir_roots,
            write_roots: Self::carve_denied(self.write_roots.clone(), &self.denied_roots),
            denied_roots: self.denied_roots.clone(),
        }
    }

    /// Resolve existing deny paths and preserve absent paths relative to the
    /// nearest existing canonical parent. Absence during policy construction
    /// must not remove protection for a future credential file.
    fn normalized_denies(roots: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in roots {
            let resolved = if let Ok(path) = std::fs::canonicalize(&root) {
                Some(path)
            } else {
                let mut missing = Vec::new();
                let mut ancestor = root.as_path();
                while !ancestor.exists() {
                    let Some(name) = ancestor.file_name() else {
                        break;
                    };
                    missing.push(name.to_os_string());
                    let Some(parent) = ancestor.parent() else {
                        break;
                    };
                    ancestor = parent;
                }
                std::fs::canonicalize(ancestor).ok().map(|mut parent| {
                    for component in missing.into_iter().rev() {
                        parent.push(component);
                    }
                    parent
                })
            };
            if let Some(path) = resolved {
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }

    /// Landlock has no deny rules. Split each broad allow root around denied
    /// descendants so an ancestor allowance cannot expose a credential.
    /// An absent denied child is omitted from grants and stays denied if it is
    /// later created beneath an allowed root.
    fn carve_denied(roots: Vec<PathBuf>, denied: &[PathBuf]) -> Vec<PathBuf> {
        fn carve(root: &Path, denied: &[PathBuf], out: &mut Vec<PathBuf>) {
            if denied.iter().any(|deny| root.starts_with(deny)) {
                return;
            }
            let nested: Vec<_> = denied
                .iter()
                .filter(|deny| deny.starts_with(root))
                .collect();
            if nested.is_empty() {
                out.push(root.to_path_buf());
                return;
            }
            let Ok(entries) = std::fs::read_dir(root) else {
                return;
            };
            for entry in entries.flatten() {
                let raw = entry.path();
                let Ok(child) = std::fs::canonicalize(&raw) else {
                    continue;
                };
                if !child.starts_with(root) {
                    continue;
                }
                if denied.iter().any(|deny| child.starts_with(deny)) {
                    continue;
                }
                if nested.iter().any(|deny| deny.starts_with(&child)) {
                    carve(&child, denied, out);
                } else {
                    out.push(child);
                }
            }
        }

        let mut out = Vec::new();
        for root in roots {
            carve(&root, denied, &mut out);
        }
        out.sort();
        out.dedup();
        out
    }

    /// Preserve directory listing on ancestors of denied files without
    /// granting file-read rights through those ancestors.
    fn carve_read_denied(roots: Vec<PathBuf>, denied: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
        fn carve(
            root: &Path,
            denied: &[PathBuf],
            read: &mut Vec<PathBuf>,
            read_dir: &mut Vec<PathBuf>,
        ) {
            if denied.iter().any(|deny| root.starts_with(deny)) {
                return;
            }
            let nested: Vec<_> = denied
                .iter()
                .filter(|deny| deny.starts_with(root))
                .collect();
            if nested.is_empty() {
                read.push(root.to_path_buf());
                return;
            }
            if !root.is_dir() {
                return;
            }
            read_dir.push(root.to_path_buf());
            let Ok(entries) = std::fs::read_dir(root) else {
                return;
            };
            for entry in entries.flatten() {
                let raw = entry.path();
                let Ok(child) = std::fs::canonicalize(&raw) else {
                    continue;
                };
                if !child.starts_with(root) {
                    continue;
                }
                if denied.iter().any(|deny| child.starts_with(deny)) {
                    continue;
                }
                if nested.iter().any(|deny| deny.starts_with(&child)) {
                    carve(&child, denied, read, read_dir);
                } else {
                    read.push(child);
                }
            }
        }

        let mut read = Vec::new();
        let mut read_dir = Vec::new();
        for root in roots {
            carve(&root, denied, &mut read, &mut read_dir);
        }
        read.sort();
        read.dedup();
        read_dir.sort();
        read_dir.dedup();
        (read, read_dir)
    }

    /// Derive the policy for one launch description.
    pub fn for_spec(spec: &SandboxLaunchSpec) -> Self {
        Self::new(
            spec.read_paths.clone(),
            spec.write_paths.clone(),
            spec.deny_paths.clone(),
        )
    }
}

/// Refuse a launch when an existing denied regular file has hardlink aliases.
/// Path-based sandbox backends cannot distinguish two names for one inode;
/// proceeding could expose the denied credential through its other name.
pub fn validate_denied_paths(denied: &[PathBuf]) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        for path in denied {
            let metadata = match std::fs::metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                // The process cannot traverse this host-protected path, so
                // its ordinary filesystem permissions already deny the same
                // child. This commonly covers `/root/.ssh` for a non-root
                // daemon; do not make unrelated constrained launches fail
                // because the host has an inaccessible account directory.
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => continue,
                Err(error) => {
                    return Err(format!(
                        "cannot inspect denied sandbox path {}: {error}",
                        path.display()
                    ));
                }
            };
            if metadata.is_file() && metadata.nlink() > 1 {
                return Err(format!(
                    "sandbox cannot safely exclude hardlinked denied file: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

/// Landlock cannot express a deny below a recursively writable ancestor.
/// Refuse such a requested grant instead of carving an incomplete set of
/// child roots and pretending the workspace is writable under the same
/// policy.
pub fn validate_landlock_write_exclusions(
    write_paths: &[PathBuf],
    denied_paths: &[PathBuf],
) -> Result<(), String> {
    let writes = BackendPolicy::normalized(write_paths.to_vec());
    let denied = BackendPolicy::normalized_denies(denied_paths.to_vec());
    for write in &writes {
        for deny in &denied {
            if write.starts_with(deny) || deny.starts_with(write) {
                return Err(format!(
                    "Landlock cannot represent a denied path beneath writable root: {}",
                    deny.display()
                ));
            }
        }
    }
    Ok(())
}

/// System and selected read-only toolchain locations needed to run a command.
///
/// This is the "tool compatible" half of the policy. It grants **read**
/// only; none of these paths become writable under any profile.
pub fn tool_read_allowances() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = [
        // Interpreter/runtime search path and shared objects. On macOS the
        // toolchain lives in `/usr` (with `/bin`, `/sbin`, `/lib*` as
        // compatibility symlinks); on Linux the dynamic loader and shared
        // libraries live under the same roots.
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/opt",
        "/etc",
        "/dev",
        "/proc",
        "/sys",
        "/var/run",
        "/private/etc",
        "/private/var/db/timezone",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();

    if let Some(home) = home_dir() {
        // Shared configuration and cache ancestors may hold credentials.
        // Expose installed Rust programs and read-only downloaded inputs,
        // while keeping Cargo/Rustup credentials and mutable state excluded.
        for relative in [
            ".cargo/bin",
            ".cargo/registry",
            ".cargo/git",
            ".rustup/settings.toml",
            ".rustup/toolchains",
            ".rustup/update-hashes",
        ] {
            paths.push(home.join(relative));
        }
    }
    paths
}

/// Shared scratch and global package caches are intentionally absent. The
/// managed process owner supplies a fresh private scratch root per launch.
pub fn tool_write_allowances() -> Vec<PathBuf> {
    Vec::new()
}

/// Stateless device nodes that must stay writable under **every** profile,
/// including `ReadOnly`.
///
/// These are not filesystem state: writing to `/dev/null` discards the
/// bytes, reading `/dev/urandom` returns entropy, and a PTY is the terminal
/// the user is already talking to. Denying them is not containment — it is
/// breakage with no security benefit, because `sh -c 'cmd > /dev/null'` and
/// every `2>&1` fail outright without them. Under Landlock this also matters:
/// the read allowance for `/dev` grants no `WriteFile` right, so a
/// redirect-capable shell needs these named explicitly in the write set.
pub fn device_write_allowances() -> Vec<PathBuf> {
    [
        "/dev/null",
        "/dev/zero",
        "/dev/random",
        "/dev/urandom",
        "/dev/tty",
        "/dev/console",
        "/dev/ptmx",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

/// Credential material and CodeGG's own secret store.
///
/// Denied on every backend. Under a deny-first backend this is redundant,
/// but it is stated here so the policy has one intent and so a future
/// allow-default backend inherits it for free.
pub fn sensitive_deny_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = [
        // Host-wide secrets and privilege material. Named individually
        // rather than by subtree: a blanket `/var/db` also covers
        // `/var/db/timezone`, which time-zone-aware programs read, and
        // normalization would silently reintroduce the conflict the
        // `no_deny_path_covers_a_read_allowance_root` test forbids.
        "/etc/sudoers",
        "/etc/shadow",
        "/etc/gshadow",
        "/root/.ssh",
        // Directory Service store: local account hashes.
        "/var/db/dslocal",
        "/var/db/dhcpd_leases",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();

    if let Some(home) = home_dir() {
        for relative in [
            ".ssh",
            ".aws",
            ".gnupg",
            ".gpg",
            ".kube",
            ".docker",
            ".netrc",
            ".npmrc",
            ".pypirc",
            ".gem/credentials",
            ".cargo/credentials",
            ".cargo/credentials.toml",
            ".cargo/config",
            ".cargo/config.toml",
            ".config/gh/hosts.yml",
            ".config/gcloud",
            ".azure",
            ".terraform.d",
            // CodeGG's own durable credential store. A sandboxed command
            // must never be able to read the secrets the agent itself uses.
            ".config/codegg",
            ".codegg",
            "Library/Keychains",
            "Library/Application Support/codegg",
        ] {
            paths.push(home.join(relative));
        }
    }
    paths
}

/// `$HOME`, if it exists and is a usable home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::sandbox::SandboxConfig;
    use std::path::Path;

    fn exists(path: &Path) -> bool {
        path.exists()
    }

    #[test]
    fn read_allowances_cover_the_interpreter_search_path() {
        let roots = BackendPolicy::normalized(tool_read_allowances());
        // Every one of these is needed before a shell can start at all.
        // Compared after canonicalization: `/bin` is `/usr/bin` and `/etc`
        // is `/private/etc` on macOS, which is exactly why the policy
        // canonicalizes before a backend ever sees it.
        for required in ["/usr", "/bin", "/etc"] {
            let resolved = Path::new(required)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(required));
            assert!(
                roots.contains(&resolved),
                "{required} must be readable or no command can start: {roots:?}"
            );
        }
    }

    #[test]
    fn device_nodes_are_writable_but_ordinary_paths_are_not() {
        let roots = BackendPolicy::normalized(device_write_allowances());
        assert!(
            roots.contains(&PathBuf::from("/dev/null")),
            "without /dev/null every shell redirect fails: {roots:?}"
        );
        assert!(
            !roots.iter().any(|root| root.starts_with("/etc")),
            "device allowances must not leak into real filesystem paths: {roots:?}"
        );
    }

    #[test]
    fn shared_scratch_and_global_cache_roots_are_not_writable() {
        assert!(tool_write_allowances().is_empty());
    }

    #[test]
    fn sensitive_paths_are_denied() {
        // The deny intent is platform-independent; normalization drops entries
        // that do not exist on this host (e.g. `~/.ssh` on a fresh CI runner),
        // so assert intent on the raw curated set.
        let raw = sensitive_deny_paths();
        assert!(
            raw.iter().any(|root| root.ends_with(".ssh")),
            "ssh keys must be on the deny list"
        );
        assert!(
            raw.iter().any(|root| root.ends_with("codegg")),
            "CodeGG's own credential store must be on the deny list"
        );
    }

    #[test]
    fn normalization_drops_missing_paths_and_canonicalizes_symlinks() {
        let normalized = BackendPolicy::normalized(vec![
            PathBuf::from("/definitely/missing/codegg-policy-path"),
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
        ]);
        assert!(
            !normalized
                .iter()
                .any(|path| path.to_string_lossy().contains("codegg-policy-path")),
            "a missing path must not reach a backend policy"
        );
        // `/tmp` canonicalizes to `/private/tmp`; the point is that both
        // copies collapse onto that one resolved path.
        let resolved = Path::new("/tmp").canonicalize().expect("canonical /tmp");
        assert_eq!(
            normalized.iter().filter(|path| **path == resolved).count(),
            1,
            "duplicates must collapse so generated profiles stay bounded: {normalized:?}"
        );
        for path in &normalized {
            assert!(exists(path), "{} must exist", path.display());
        }
    }

    #[test]
    fn broad_allowance_is_split_around_existing_and_future_denies() {
        let dir = tempfile::tempdir().expect("fixture root");
        let safe = dir.path().join("safe");
        let secret_dir = dir.path().join("credentials");
        std::fs::create_dir_all(&safe).expect("safe directory");
        std::fs::create_dir_all(&secret_dir).expect("secret directory");
        std::fs::write(secret_dir.join("token"), b"secret").expect("secret fixture");
        let future_secret = secret_dir.join("future-token");
        let policy = BackendPolicy::new(
            vec![dir.path().to_path_buf()],
            vec![dir.path().to_path_buf()],
            vec![secret_dir.join("token"), future_secret.clone()],
        )
        .landlock_effective();
        assert!(policy.read_roots.iter().any(|root| root == &safe));
        assert!(policy.read_dir_roots.iter().any(|root| root == dir.path()));
        assert!(policy.write_roots.iter().any(|root| root == &safe));
        assert!(!policy
            .read_roots
            .iter()
            .any(|root| root.starts_with(&secret_dir)));
        assert!(!policy
            .write_roots
            .iter()
            .any(|root| root.starts_with(&secret_dir)));
        assert!(policy.denied_roots.contains(&future_secret));
    }

    #[cfg(unix)]
    #[test]
    fn hardlinked_denied_files_reject_the_policy() {
        let dir = tempfile::tempdir().expect("fixture root");
        let denied = dir.path().join("credential");
        let alias = dir.path().join("workspace-alias");
        std::fs::write(&denied, b"secret").expect("credential fixture");
        std::fs::hard_link(&denied, &alias).expect("hardlink alias");
        let error = validate_denied_paths(&[denied]).expect_err("hardlink must fail closed");
        assert!(error.contains("hardlinked denied file"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn host_inaccessible_deny_path_does_not_block_preflight() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("fixture root");
        let protected = dir.path().join("protected");
        std::fs::create_dir(&protected).expect("protected fixture");
        let secret = protected.join("secret");
        std::fs::write(&secret, "host-protected").expect("secret fixture");
        std::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o000))
            .expect("remove traversal permission");

        let metadata = std::fs::metadata(&secret);
        let validation = validate_denied_paths(std::slice::from_ref(&secret));
        std::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o700))
            .expect("restore fixture permissions");
        if metadata.is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied) {
            assert!(
                validation.is_ok(),
                "host DAC already blocks the path: {validation:?}"
            );
        } else {
            eprintln!("skipped: test process can traverse mode-000 fixture directory");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_alias_of_denied_file_is_not_granted() {
        let dir = tempfile::tempdir().expect("fixture root");
        let secret = dir.path().join("secret");
        let allowed = dir.path().join("allowed");
        std::fs::create_dir_all(&allowed).expect("allowed directory");
        std::fs::write(&secret, b"secret").expect("secret fixture");
        std::os::unix::fs::symlink(&secret, allowed.join("alias")).expect("symlink alias");
        let policy = BackendPolicy::new(
            vec![dir.path().to_path_buf()],
            Vec::new(),
            vec![secret.clone()],
        )
        .landlock_effective();
        assert!(
            !policy.read_roots.iter().any(|root| root == &secret),
            "canonicalized symlink aliases of a denied file must not be granted: {policy:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn carving_does_not_turn_external_symlinks_into_new_grants() {
        let root = tempfile::tempdir().expect("allowed root");
        let outside = tempfile::tempdir().expect("outside root");
        std::fs::write(outside.path().join("secret"), b"outside").expect("outside fixture");
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape"))
            .expect("external symlink");
        let policy = BackendPolicy::new(
            vec![root.path().to_path_buf()],
            Vec::new(),
            vec![root.path().join("future-deny")],
        )
        .landlock_effective();
        assert!(
            !policy
                .read_roots
                .iter()
                .any(|grant| grant.starts_with(outside.path())),
            "carving an allow root must not grant an external symlink target: {policy:?}"
        );
    }

    #[test]
    fn landlock_rejects_writable_ancestor_of_a_denied_path() {
        let dir = tempfile::tempdir().expect("fixture root");
        let future_deny = dir.path().join("credentials/new-token");
        let error = validate_landlock_write_exclusions(&[dir.path().to_path_buf()], &[future_deny])
            .expect_err("Landlock cannot subtract a deny from a writable ancestor");
        assert!(error.contains("cannot represent a denied path"), "{error}");
    }

    #[test]
    fn no_deny_path_covers_a_read_allowance_root() {
        // A deny that *contains* an allowance silently breaks the tooling the
        // allowance exists for: denying all of `/etc` blocks `/etc/hosts` and
        // `/etc/resolv.conf`, denying all of `/var` blocks the per-user
        // temporary directory. Backend compilation preserves the parent's
        // listing right separately while excluding the denied descendants.
        let read = BackendPolicy::normalized(tool_read_allowances());
        for denied in BackendPolicy::normalized(sensitive_deny_paths()) {
            for allowed in &read {
                assert!(
                    !allowed.starts_with(&denied),
                    "deny {denied:?} would swallow read allowance {allowed:?}, \
                     breaking the tools that allowance exists for"
                );
            }
        }
    }

    #[test]
    fn legacy_sensitive_path_wrapper_matches_the_policy() {
        // `get_sensitive_paths` feeds `SandboxConfig::deny_paths`; if it
        // drifts from the policy, callers silently get a different sandbox
        // than the one the tests describe.
        let mut config = SandboxConfig::new()
            .with_enabled(true)
            .with_deny_paths(crate::security::sandbox::get_sensitive_paths());
        let workspace = tempfile::tempdir().expect("workspace fixture");
        config.allowed_paths = vec![workspace.path().to_string_lossy().into_owned()];
        let spec = config
            .launch_spec(
                "sh",
                &["-c".to_string(), "true".to_string()],
                Some(workspace.path()),
            )
            .expect("launch spec");
        for expected in BackendPolicy::normalized(sensitive_deny_paths()) {
            assert!(
                spec.deny_paths.iter().any(|path| path == &expected),
                "wrapper dropped the curated deny path {expected:?}"
            );
        }
    }

    #[test]
    fn policy_derives_from_a_launch_spec() {
        let dir = tempfile::tempdir().expect("workspace fixture");
        let spec = SandboxLaunchSpec {
            target: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "true".to_string()],
            read_paths: vec![dir.path().to_path_buf()],
            write_paths: vec![dir.path().to_path_buf()],
            deny_paths: sensitive_deny_paths(),
        };
        let policy = BackendPolicy::for_spec(&spec);
        let canonical = dir.path().canonicalize().expect("canonical root");
        assert!(policy.read_roots.contains(&canonical));
        assert!(policy.write_roots.contains(&canonical));
        assert!(
            !policy.denied_roots.is_empty(),
            "a policy must carry the deny intent into the backend"
        );
    }
}

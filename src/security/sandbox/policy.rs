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
//! So the curated sets below grant read access to the *system and tool-cache*
//! locations real tools need, and confine **writes** — the operation that
//! actually damages a host — to the workspace plus temporary directories.
//! Sensitive material (credentials, keys, the CodeGG credential store) is
//! denied explicitly on every backend.
//!
//! Every path is canonicalized before it reaches a policy: Seatbelt matches
//! `subpath` against the *resolved* path, so an allowance naming `/tmp` does
//! not cover `/private/tmp` on macOS. Roots that do not exist are dropped,
//! because a rule naming a missing path is either an error or dead weight
//! depending on the backend.

use std::path::PathBuf;

use super::SandboxLaunchSpec;

/// Path sets a backend compiles into its native containment rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendPolicy {
    /// Roots the target may read.
    pub read_roots: Vec<PathBuf>,
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
    /// Canonicalize, de-duplicate, and drop non-existent entries.
    ///
    /// Canonicalization is what makes a policy portable across `/tmp` ->
    /// `/private/tmp` symlinks and per-user worktree aliases; de-duplication
    /// keeps generated profiles bounded when roots overlap.
    fn normalized(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
        roots.retain(|path| path.exists());
        let mut seen = Vec::new();
        let mut out = Vec::with_capacity(roots.len());
        for root in roots {
            let canonical = std::fs::canonicalize(&root).unwrap_or(root);
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
        Self {
            read_roots: Self::normalized(read),
            write_roots: Self::normalized(write),
            denied_roots: Self::normalized(denied),
        }
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

/// System and tool-cache locations a command needs just to run.
///
/// This is the "tool compatible" half of the policy. It grants **read**
/// only; none of these paths become writable under `ReadOnly`, and only the
/// explicit write allowances below ever become writable at all.
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
        // Per-user tool state. These are caches and configuration, not
        // credentials, and every mainstream toolchain needs at least one of
        // them to function at all.
        for relative in [
            ".cargo",
            ".rustup",
            ".npm",
            ".cache",
            ".local/share",
            ".local/state",
            ".config",
            ".gitconfig",
            "Library/Caches",
            "Library/Preferences",
            "Library/Application Support",
        ] {
            paths.push(home.join(relative));
        }
    }
    paths
}

/// Directories a command may write even outside the workspace.
///
/// Confined scratch space, not host authority: temporary directories and
/// package-manager caches. Without these, `cargo`, `npm`, and every test
/// runner that writes a temp fixture fail immediately.
pub fn tool_write_allowances() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = [
        "/tmp",
        "/private/tmp",
        "/var/tmp",
        "/private/var/tmp",
        "/dev/shm",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();

    // `TMPDIR` is per-user on both platforms (`/var/folders/...` on macOS,
    // `/tmp/...` on Linux) and is frequently outside every other root.
    if let Some(tmp) = std::env::var_os("TMPDIR") {
        let tmp = PathBuf::from(tmp);
        if tmp.as_os_str().is_empty() {
            // An empty TMPDIR is a common way to break tools; ignore it.
        } else {
            paths.push(tmp);
        }
    }

    if let Some(home) = home_dir() {
        for relative in [
            ".cargo",
            ".rustup",
            ".npm",
            ".cache",
            ".local/state",
            "Library/Caches",
        ] {
            paths.push(home.join(relative));
        }
    }
    paths
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
    fn write_allowances_include_scratch_space() {
        let roots = BackendPolicy::normalized(tool_write_allowances());
        assert!(
            roots
                .iter()
                .any(|root| root == &PathBuf::from("/private/tmp")),
            "temporary space must be writable or test runners cannot work"
        );
    }

    #[test]
    fn sensitive_paths_are_denied() {
        let roots = BackendPolicy::normalized(sensitive_deny_paths());
        assert!(
            roots
                .iter()
                .any(|root| root.ends_with(".ssh") && root.is_dir()),
            "ssh keys must be on the deny list"
        );
        assert!(
            roots.iter().any(|root| root.ends_with("codegg")),
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
    fn no_deny_path_covers_a_read_allowance_root() {
        // A deny that *contains* an allowance silently breaks the tooling the
        // allowance exists for: denying all of `/etc` blocks `/etc/hosts` and
        // `/etc/resolv.conf`, denying all of `/var` blocks the per-user
        // temporary directory. The profile generator emits denies before any
        // later allow, so a deny always wins over the `allow default`
        // baseline — which is exactly why the two sets must not overlap that
        // way.
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

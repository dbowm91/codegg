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

/// Resolve a policy root for later backend compilation. On Windows this is a
/// conservative lexical and reparse-point check only; it is not a security
/// boundary against a path being replaced after the check. M004 must bind
/// enforcement to opened handles and ACL identity before constrained runs are
/// enabled.
pub fn resolve_policy_path(path: &std::path::Path) -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        path.canonicalize().map_err(|error| error.to_string())
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        let raw = path.as_os_str().to_string_lossy();
        let folded = raw.replace('/', "\\");
        if folded.starts_with(r"\\?\") {
            return Err("extended-length device paths are not accepted as policy roots".into());
        }
        if folded.starts_with(r"\\") || folded.starts_with(r"\\.\") {
            return Err("UNC and device namespace paths are not accepted as policy roots".into());
        }
        if !path.is_absolute() {
            return Err("Windows policy roots must be absolute drive paths".into());
        }
        if folded
            .split('\\')
            .skip(1)
            .any(|component| component.contains(':'))
        {
            return Err("alternate data stream syntax is not accepted in policy roots".into());
        }
        if path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err("parent traversal is not accepted in Windows policy roots".into());
        }

        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component.as_os_str());
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
                    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                        return Err(format!(
                            "reparse-point component is not accepted in policy root: {}",
                            current.display()
                        ));
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        path.canonicalize().map_err(|error| error.to_string())
    }
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
#[cfg(unix)]
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

/// Windows runtime and installed-tool locations required to execute native
/// commands. These roots are read inputs for a future enforcement backend;
/// they do not enable Windows constrained launches in this milestone.
#[cfg(windows)]
pub fn tool_read_allowances() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(system_root) = windows_env_dir("SystemRoot").or_else(|| windows_env_dir("WINDIR")) {
        paths.extend([
            system_root.clone(),
            system_root.join("System32"),
            system_root.join("SysWOW64"),
            system_root.join("System32/WindowsPowerShell/v1.0"),
        ]);
    }
    for name in ["ProgramFiles", "ProgramFiles(x86)", "CommonProgramFiles"] {
        if let Some(root) = windows_env_dir(name) {
            paths.push(root);
        }
    }
    if let Some(home) = home_dir() {
        for relative in [
            ".cargo",
            ".rustup",
            ".gitconfig",
            ".npm",
            ".cache",
            "AppData/Local/Programs",
            "AppData/Roaming/npm",
        ] {
            paths.push(home.join(relative));
        }
    }
    paths
}

/// Shared scratch and global package caches are intentionally absent. The
/// managed process owner supplies a fresh private scratch root per launch.
#[cfg(unix)]
pub fn tool_write_allowances() -> Vec<PathBuf> {
    Vec::new()
}

/// Temporary and package-cache write roots used by native developer tools.
/// M004 must replace ambient TEMP/TMP authority with per-run scratch before
/// it advertises constrained Windows execution.
#[cfg(windows)]
pub fn tool_write_allowances() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for name in ["TEMP", "TMP"] {
        if let Some(root) = windows_env_dir(name) {
            paths.push(root);
        }
    }
    if let Some(home) = home_dir() {
        for relative in [
            ".cargo",
            ".rustup",
            ".npm",
            ".cache",
            "AppData/Local/Temp",
            "AppData/Local/Cargo",
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
#[cfg(unix)]
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

#[cfg(windows)]
pub fn device_write_allowances() -> Vec<PathBuf> {
    Vec::new()
}

/// Credential material and CodeGG's own secret store.
///
/// Denied on every backend. Under a deny-first backend this is redundant,
/// but it is stated here so the policy has one intent and so a future
/// allow-default backend inherits it for free.
#[cfg(unix)]
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

#[cfg(windows)]
pub fn sensitive_deny_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
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
            ".config/codegg",
            ".codegg",
            "AppData/Roaming/codegg",
            "AppData/Local/codegg",
            "AppData/Roaming/Microsoft/Credentials",
            "AppData/Local/Microsoft/Credentials",
            "AppData/Roaming/Microsoft/Protect",
            "AppData/Local/Microsoft/Vault",
        ] {
            paths.push(home.join(relative));
        }
    }
    paths
}

/// `$HOME`, if it exists and is a usable home directory.
#[cfg(unix)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

#[cfg(windows)]
fn home_dir() -> Option<PathBuf> {
    windows_profile_root()
}

#[cfg(windows)]
pub fn windows_profile_root() -> Option<PathBuf> {
    windows_env_dir("USERPROFILE").filter(|path| path.is_dir())
}

#[cfg(windows)]
fn windows_env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

#[cfg(all(test, unix))]
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
        // Policy roots are canonicalized; on macOS the system temp root is a
        // symlink (`/var/folders/...` -> `/private/var/folders/...`), so the
        // fixture paths must be resolved before they are compared.
        let root = std::fs::canonicalize(dir.path()).expect("canonical fixture root");
        let safe = root.join("safe");
        let secret_dir = root.join("credentials");
        std::fs::create_dir_all(&safe).expect("safe directory");
        std::fs::create_dir_all(&secret_dir).expect("secret directory");
        std::fs::write(secret_dir.join("token"), b"secret").expect("secret fixture");
        let future_secret = secret_dir.join("future-token");
        let policy = BackendPolicy::new(
            vec![root.clone()],
            vec![root.clone()],
            vec![secret_dir.join("token"), future_secret.clone()],
        )
        .landlock_effective();
        assert!(policy.read_roots.iter().any(|r| r == &safe));
        assert!(policy.read_dir_roots.iter().any(|r| r == &root));
        assert!(policy.write_roots.iter().any(|r| r == &safe));
        assert!(!policy.read_roots.iter().any(|r| r.starts_with(&secret_dir)));
        assert!(!policy
            .write_roots
            .iter()
            .any(|r| r.starts_with(&secret_dir)));
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
        // The spec carries the curated denies verbatim: `launch_spec`
        // deliberately leaves them unresolved so an absent credential file
        // stays denied, and `normalized_denies` resolves them when a backend
        // compiles the policy.
        for expected in sensitive_deny_paths() {
            assert!(
                spec.deny_paths.iter().any(|path| path == &expected),
                "wrapper dropped the curated deny path {expected:?}"
            );
        }
        // Resolution still happens before a backend sees the policy, and
        // matches the canonicalized form the allow sets are normalized to.
        let resolved = BackendPolicy::for_spec(&spec).denied_roots;
        for expected in BackendPolicy::normalized(sensitive_deny_paths()) {
            assert!(
                resolved.iter().any(|path| path == &expected),
                "compiled policy dropped the curated deny path {expected:?}"
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

#[cfg(all(test, windows))]
mod windows_policy_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn windows_roots_use_known_folders_and_retain_credential_denies() {
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap();
        let reads = tool_read_allowances();
        assert!(reads.iter().any(|path| path.ends_with("System32")));
        assert!(reads.iter().any(|path| path.starts_with(&home)));
        let writes = tool_write_allowances();
        assert!(writes.iter().any(|path| {
            std::env::var_os("TEMP").is_some_and(|temp| path.as_os_str() == temp.as_os_str())
        }));
        let denies = sensitive_deny_paths();
        assert!(denies.contains(&home.join(".ssh")));
        assert!(denies.contains(&home.join(".config/codegg")));
        assert!(device_write_allowances().is_empty());
    }

    #[test]
    fn windows_policy_path_rejects_unc_device_ads_and_traversal() {
        for rejected in [
            r"\\server\share\repo",
            r"\\?\C:\repo",
            r"\\.\PhysicalDrive0",
            r"C:\repo\secrets:stream",
            r"C:\repo\..\outside",
        ] {
            assert!(
                resolve_policy_path(Path::new(rejected)).is_err(),
                "must reject {rejected}"
            );
        }
    }

    #[test]
    fn windows_policy_path_rejects_junction_components() {
        let fixture = tempfile::tempdir().expect("fixture root");
        let target = fixture.path().join("target");
        let junction = fixture.path().join("junction");
        std::fs::create_dir(&target).expect("target directory");
        // execution-ownership: test_only
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("start junction fixture command");
        assert!(
            output.status.success() && junction.is_dir(),
            "native Windows runner must create a junction fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(resolve_policy_path(&junction).is_err());
    }

    #[test]
    fn windows_policy_path_normalizes_case_aliases() {
        let fixture = tempfile::tempdir().expect("fixture root");
        let mixed = fixture.path().join("CaseAlias");
        std::fs::create_dir(&mixed).expect("fixture directory");
        let lower = PathBuf::from(mixed.to_string_lossy().to_lowercase());
        let upper = PathBuf::from(mixed.to_string_lossy().to_uppercase());
        assert_eq!(
            resolve_policy_path(&lower).expect("lower-case path"),
            resolve_policy_path(&upper).expect("upper-case path")
        );
    }

    #[test]
    fn windows_launch_spec_keeps_workspace_write_roots_distinct() {
        let fixture = tempfile::tempdir().expect("workspace fixture root");
        let first = fixture.path().join("worktree-one");
        let second = fixture.path().join("worktree-two");
        std::fs::create_dir(&first).expect("first worktree");
        std::fs::create_dir(&second).expect("second worktree");
        let argv =
            crate::tool::bash::process::shell_argv("exit 0", None).expect("native shell argv");
        let mut config = crate::security::sandbox::SandboxConfig::new()
            .with_enabled(true)
            .with_mode(crate::security::sandbox::SandboxMode::WorkspaceWrite);
        config.allowed_paths = vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];
        let spec = config
            .launch_spec(&argv[0], &argv[1..], Some(&first))
            .expect("Windows policy inputs");
        for root in [&first, &second] {
            let resolved = resolve_policy_path(root).expect("resolved worktree");
            assert!(
                spec.write_paths.contains(&resolved),
                "worktree root must remain a distinct write input: {resolved:?}"
            );
        }
    }

    #[test]
    fn windows_policy_roots_cannot_be_resolved_through_search_path() {
        assert!(resolve_policy_path(Path::new("powershell.exe")).is_err());
        assert!(resolve_policy_path(Path::new("cmd.exe")).is_err());
    }
}

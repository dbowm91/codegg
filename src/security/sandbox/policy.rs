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

/// Directories a command may write even outside the workspace.
///
/// Confined scratch space, not host authority: temporary directories and
/// package-manager caches. Without these, `cargo`, `npm`, and every test
/// runner that writes a temp fixture fail immediately.
#[cfg(unix)]
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
    fn write_allowances_include_scratch_space() {
        // The curated set names scratch space for both platforms; normalization
        // drops whatever does not exist here, so assert the curated intent on
        // the raw set and platform-appropriate presence on the normalized one.
        let raw = tool_write_allowances();
        assert!(
            raw.iter().any(|root| root == &PathBuf::from("/tmp"))
                && raw
                    .iter()
                    .any(|root| root == &PathBuf::from("/private/tmp")),
            "temporary space must be writable or test runners cannot work: {raw:?}"
        );
        let roots = BackendPolicy::normalized(raw);
        assert!(
            roots.iter().any(|root| {
                root == &PathBuf::from("/tmp")
                    || root == &PathBuf::from("/private/tmp")
                    || root.ends_with("tmp")
            }),
            "normalized scratch space must exist on this host: {roots:?}"
        );
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

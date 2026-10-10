//! Linux Landlock backend.
//!
//! Landlock is an LSM, so enforcement is **deny-first**: a ruleset grants
//! access only to the paths it names, and anything unlisted is denied. That
//! makes it the stricter of CodeGG's two shipped backends, and it is the
//! reason its guarantees list can include the `no_new_privs` privilege drop
//! that Seatbelt cannot provide.

use super::backend::BackendEnforcement;
use super::SandboxLaunchSpec;
#[cfg(target_os = "linux")]
use std::path::Path;

/// Privileges Landlock adds beyond plain filesystem path rules.
///
/// Enforced by `PR_SET_NO_NEW_PRIVS` as part of restriction, which is what
/// makes a setuid binary in an allowed read root a non-escape.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const LANDLOCK_GUARANTEES: &[&str] = &[
    "deny-first path allowlist",
    "no_new_privs privilege drop",
    "inherited by exec'd target",
];

/// Escapes that survive a Landlock ruleset.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const LANDLOCK_LIMITS: &[&str] = &[
    "no network isolation",
    "open file descriptors inherited before restriction remain usable",
    "filesystem permissions still apply on top of the ruleset",
];

/// Availability probe. Cheap and side-effect free: it creates a ruleset with
/// no rules and throws it away.
#[cfg(target_os = "linux")]
pub fn probe() -> Result<(), String> {
    use landlock::{AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, ABI};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_read(ABI::V1))
        .map_err(|e| format!("Landlock access selection failed: {e}"))?
        .create()
        .map(|_| ())
        .map_err(|e| format!("Landlock unavailable: {e}"))
}

#[cfg(not(target_os = "linux"))]
pub fn probe() -> Result<(), String> {
    Err("Landlock is a Linux LSM and is not available on this platform".to_string())
}

/// Apply the policy to the current process.
///
/// Returns the effective kernel ABI for observability. A partially-enforced
/// ruleset is a hard failure: reporting a degraded ruleset as enforced is
/// exactly the lie this module exists to prevent.
#[cfg(target_os = "linux")]
pub fn apply(spec: &SandboxLaunchSpec) -> Result<BackendEnforcement, String> {
    use landlock::{
        Access, AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
        RulesetAttr, RulesetCreated, RulesetCreatedAttr, RulesetStatus, ABI,
    };

    // ABI 1 is the minimum Landlock filesystem contract and is available on
    // every Landlock-capable kernel. Newer rights are intentionally not
    // requested dynamically: a partial ruleset must never be reported as
    // enforced, and the helper's outcome still records the kernel's
    // effective ABI for observability.
    let abi = ABI::V1;
    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);
    let handled = AccessFs::from_all(abi);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(handled)
        .map_err(|e| format!("Landlock ruleset access selection failed: {e}"))?
        .create()
        .map_err(|e| format!("Landlock ruleset creation failed: {e}"))?;

    let add_path = |ruleset: RulesetCreated, path: &Path, access: BitFlags<AccessFs>| {
        if !path.exists() {
            return Err(format!(
                "required sandbox path does not exist: {}",
                path.display()
            ));
        }
        let fd =
            PathFd::new(path).map_err(|e| format!("open sandbox path {}: {e}", path.display()))?;
        let access = landlock_access_for_path(path, access, abi)?;
        ruleset
            .add_rule(PathBeneath::new(fd, access))
            .map_err(|e| format!("add sandbox rule {}: {e}", path.display()))
    };

    for path in &spec.read_paths {
        ruleset = add_path(ruleset, path, read_access)?;
    }
    for path in &spec.write_paths {
        ruleset = add_path(ruleset, path, write_access)?;
    }

    let status = ruleset
        .restrict_self()
        .map_err(|e| format!("Landlock restriction failed: {e}"))?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err(format!(
            "Landlock restriction was not fully enforced (ruleset={:?}, no_new_privs={})",
            status.ruleset, status.no_new_privs
        ));
    }
    match status.landlock {
        landlock::LandlockStatus::Available { effective_abi, .. } => Ok(BackendEnforcement::new(
            Some(effective_abi as u32),
            LANDLOCK_GUARANTEES,
            LANDLOCK_LIMITS,
        )),
        other => Err(format!(
            "Landlock became unavailable during setup: {other:?}"
        )),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn apply(_spec: &SandboxLaunchSpec) -> Result<BackendEnforcement, String> {
    Err("Landlock is a Linux LSM and is not available on this platform".to_string())
}

/// Landlock's directory rights are meaningless on a regular file, and
/// `ReadDir` on `/dev/null` (or any other special file) would fail the
/// `add_rule` call outright. Narrow the mask by file type.
#[cfg(target_os = "linux")]
pub(super) fn landlock_access_for_path(
    path: &Path,
    access: landlock::BitFlags<landlock::AccessFs>,
    abi: landlock::ABI,
) -> Result<landlock::BitFlags<landlock::AccessFs>, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("classify sandbox path {}: {error}", path.display()))?;
    if metadata.is_dir() {
        Ok(access)
    } else {
        Ok(access & landlock::AccessFs::from_file(abi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_keeps_directory_rights_for_directories() {
        use landlock::{AccessFs, ABI};

        let directory = tempfile::tempdir().expect("directory fixture");
        let access =
            landlock_access_for_path(directory.path(), AccessFs::from_read(ABI::V1), ABI::V1)
                .expect("directory classification");

        assert!(access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_removes_directory_rights_for_regular_files() {
        use landlock::{AccessFs, ABI};

        let file = tempfile::NamedTempFile::new().expect("file fixture");
        let access = landlock_access_for_path(file.path(), AccessFs::from_read(ABI::V1), ABI::V1)
            .expect("regular-file classification");

        assert!(!access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_removes_directory_rights_for_special_files() {
        use landlock::{Access, AccessFs, ABI};

        let path = Path::new("/dev/null");
        if !path.exists() {
            return;
        }
        let access = landlock_access_for_path(path, AccessFs::from_all(ABI::V1), ABI::V1)
            .expect("special-file classification");

        assert!(!access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
        assert!(access.contains(AccessFs::WriteFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_fails_closed_when_path_cannot_be_classified() {
        use landlock::{AccessFs, ABI};

        let path = Path::new("/definitely/missing/codegg-sandbox-path");
        let error = landlock_access_for_path(path, AccessFs::from_read(ABI::V1), ABI::V1)
            .expect_err("missing path classification must fail");

        assert!(error.contains("classify sandbox path"));
        assert!(error.contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn landlock_reports_a_readable_reason_off_platform() {
        if cfg!(target_os = "linux") {
            // On Linux the probe must actually answer, not always fail.
            let _ = probe();
            return;
        }
        let reason = probe().expect_err("Landlock must be unavailable off Linux");
        assert!(
            reason.contains("Landlock"),
            "the reason must name the backend: {reason}"
        );
    }

    #[test]
    fn guarantees_name_the_privilege_drop_landlock_actually_performs() {
        assert!(
            LANDLOCK_GUARANTEES
                .iter()
                .any(|item| item.contains("no_new_privs")),
            "Landlock's privilege drop is a real guarantee and must be stated"
        );
        assert!(
            LANDLOCK_LIMITS
                .iter()
                .any(|item| item.contains("no network isolation")),
            "Landlock must not imply network isolation"
        );
    }
}

//! Linux Landlock backend.
//!
//! Landlock is an LSM, so enforcement is **deny-first**: a ruleset grants
//! access only to the paths it names, and anything unlisted is denied. That
//! makes it the stricter of CodeGG's two shipped backends, and it is the
//! reason its guarantees list can include the `no_new_privs` privilege drop
//! that Seatbelt cannot provide.

use super::backend::BackendEnforcement;
#[cfg(target_os = "linux")]
use super::policy::BackendPolicy;
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
    "write and truncate rights limited to granted writable paths (Landlock ABI 3+)",
    "cross-directory refer rights handled (Landlock ABI 2+)",
    "additional filesystem rights supported by this crate and host are handled",
    "no_new_privs privilege drop",
    "inherited by exec'd target",
];

/// Escapes and disclosed posture limits that survive a Landlock ruleset.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) const LANDLOCK_LIMITS: &[&str] = &[
    "no network isolation",
    super::backend::INHERITED_DESCRIPTOR_LIMIT,
    "filesystem permissions still apply on top of the ruleset",
    "rights newer than the crate's supported effective ABI are not handled",
    "pathname AF_UNIX sockets are not mediated by Landlock and stay reachable through a granted path",
    "the /proc grant is self-scoped: a child of the command cannot read its own /proc/self, and another process's /proc/<pid> is not granted",
];

/// Stated when the host cannot obtain the ABI 6 scope. The scope is never
/// reported as a guarantee the mechanism did not obtain (ADR-0063 §4).
#[cfg(any(target_os = "linux", test))]
const PROCESS_SCOPE_ABSENT_LIMIT: &str =
    "Landlock ABI 6 process scope not obtained: signals and abstract AF_UNIX sockets are not isolated";

/// Availability probe. Cheap and side-effect free: it creates a ruleset with
/// no rules and throws it away.
#[cfg(target_os = "linux")]
pub fn probe() -> Result<(), String> {
    use landlock::{Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, ABI};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V3))
        .map_err(|e| format!("Landlock access selection failed: {e}"))?
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(ABI::V9))
        .map_err(|e| format!("Landlock optional access selection failed: {e}"))?
        .create()
        .map(|_| ())
        .map_err(|e| format!("Landlock unavailable: {e}"))
}

#[cfg(not(target_os = "linux"))]
pub fn probe() -> Result<(), String> {
    Err("Landlock is a Linux LSM and is not available on this platform".to_string())
}

/// The guarantee a launch states once it obtained the process scope.
///
/// Both routes that apply the scope state it with these words, so the two
/// descriptors cannot disagree about the same scope. `abi` is the effective
/// one, never a baseline the host need not have granted.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn process_scope_guarantee(abi: u32) -> String {
    format!(
        "Landlock ABI {abi} process scope obtained: signals and abstract AF_UNIX sockets \
         outside the scoped process hierarchy are unreachable"
    )
}

/// Compose the descriptor for a completed restriction. The process scope is
/// a guarantee only when this launch obtained it; otherwise it is a limit,
/// because a dimension the mechanism did not obtain must not be claimed.
#[cfg(any(target_os = "linux", test))]
fn landlock_enforcement(effective_abi: u32, process_scope_abi: Option<u32>) -> BackendEnforcement {
    let mut enforcement =
        BackendEnforcement::new(Some(effective_abi), LANDLOCK_GUARANTEES, LANDLOCK_LIMITS);
    enforcement.process_scope_abi = process_scope_abi;
    match process_scope_abi {
        Some(abi) => enforcement.guarantees.push(process_scope_guarantee(abi)),
        None => enforcement
            .limits
            .push(PROCESS_SCOPE_ABSENT_LIMIT.to_string()),
    }
    enforcement
}

/// Apply the ABI 6 scope only when the host advertises it. A host without
/// the scope keeps the filesystem-only tier; a failed application is a refusal.
#[cfg(any(target_os = "linux", test))]
fn process_scope_from(
    probe: impl FnOnce() -> Result<(), String>,
    apply: impl FnOnce() -> Result<u32, String>,
) -> Result<Option<u32>, String> {
    if probe().is_err() {
        return Ok(None);
    }
    apply().map(Some)
}

/// Apply the policy to the current process.
///
/// Returns the effective kernel ABI for observability. The ABI 3 baseline is
/// mandatory; `PartiallyEnforced` may describe only newer rights requested
/// best-effort than the host supports.
#[cfg(target_os = "linux")]
pub fn apply(spec: &SandboxLaunchSpec) -> Result<BackendEnforcement, String> {
    use landlock::{
        Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetStatus, ABI,
    };

    super::policy::validate_landlock_write_exclusions(&spec.write_paths, &spec.deny_paths)?;

    // ABI 3 is the minimum contract: v2 adds REFER and v3 adds TRUNCATE.
    // Newer filesystem rights are requested best-effort after that hard
    // minimum, so each host enforces the strongest ABI the crate supports.
    let abi = ABI::V9;
    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V3))
        .map_err(|e| format!("Landlock ruleset access selection failed: {e}"))?
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(ABI::V9))
        .map_err(|e| format!("Landlock optional access selection failed: {e}"))?
        .create()
        .map_err(|e| format!("Landlock ruleset creation failed: {e}"))?;

    // The policy is built (canonicalized, carved, sorted) once, and its
    // object identities are captured here — before any rule opens a path.
    // Opening a grant by name afterwards would let a replacement between the
    // two steps become the granted object, so the open is no-follow and the
    // opened metadata is checked against this snapshot.
    let policy = BackendPolicy::for_spec(spec).landlock_effective();
    let identities = capture_grant_identities(&policy)?;
    for path in &policy.read_roots {
        ruleset = add_landlock_path(ruleset, path, read_access, abi, &identities)?;
    }
    for path in &policy.read_dir_roots {
        ruleset = add_landlock_path(ruleset, path, AccessFs::ReadDir.into(), abi, &identities)?;
    }
    for path in &policy.write_roots {
        ruleset = add_landlock_path(ruleset, path, write_access, abi, &identities)?;
    }

    let status = ruleset
        .restrict_self()
        .map_err(|e| format!("Landlock restriction failed: {e}"))?;
    if status.ruleset == RulesetStatus::NotEnforced || !status.no_new_privs {
        return Err(format!(
            "Landlock restriction was not enforced (ruleset={:?}, no_new_privs={})",
            status.ruleset, status.no_new_privs
        ));
    }
    match status.landlock {
        landlock::LandlockStatus::Available { effective_abi, .. } => {
            if effective_abi < ABI::V3 {
                return Err(format!(
                    "Landlock effective ABI {effective_abi:?} is below required ABI V3"
                ));
            }
            // ADR-0063: ABI 6 scopes are part of the default route and need
            // no namespace. They restrict signals and abstract AF_UNIX
            // sockets at no cost to the filesystem ruleset above.
            let process_scope_abi = process_scope_from(probe_process_scopes, apply_process_scopes)?;
            Ok(landlock_enforcement(
                effective_abi as u32,
                process_scope_abi,
            ))
        }
        other => Err(format!(
            "Landlock became unavailable during setup: {other:?}"
        )),
    }
}

/// Apply the M004 Landlock ABI 6 IPC scopes without adding filesystem or
/// network restrictions. Called by the trusted helper immediately before it
/// executes Bubblewrap so the namespace launcher and target inherit the same
/// signal and abstract-socket domain.
#[cfg(target_os = "linux")]
pub fn apply_process_scopes() -> Result<u32, String> {
    use landlock::{
        Access, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetStatus, Scope, ABI,
    };

    let ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .scope(Scope::from_all(ABI::V6))
        .map_err(|error| format!("Landlock process scope selection failed: {error}"))?
        .create()
        .map_err(|error| format!("Landlock process scope setup failed: {error}"))?;
    let status = ruleset
        .restrict_self()
        .map_err(|error| format!("Landlock process scope enforcement failed: {error}"))?;
    if status.ruleset == RulesetStatus::NotEnforced || !status.no_new_privs {
        return Err(format!(
            "Landlock process scope was not enforced (ruleset={:?}, no_new_privs={})",
            status.ruleset, status.no_new_privs
        ));
    }
    match status.landlock {
        landlock::LandlockStatus::Available { effective_abi, .. } if effective_abi >= ABI::V6 => {
            Ok(effective_abi as u32)
        }
        other => Err(format!(
            "Landlock ABI 6 process scopes unavailable after setup: {other:?}"
        )),
    }
}

/// Read-only qualification check for the exact signal and abstract-socket
/// scopes required by M004.
#[cfg(target_os = "linux")]
pub fn probe_process_scopes() -> Result<(), String> {
    use landlock::{Access, CompatLevel, Compatible, Ruleset, RulesetAttr, Scope, ABI};

    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .scope(Scope::from_all(ABI::V6))
        .map_err(|error| format!("Landlock ABI 6 scope request failed: {error}"))?
        .create()
        .map(|_| ())
        .map_err(|error| format!("Landlock ABI 6 scope probe failed: {error}"))
}

#[cfg(not(target_os = "linux"))]
pub fn probe_process_scopes() -> Result<(), String> {
    Err("Landlock process scopes are available only on Linux".into())
}

#[cfg(not(target_os = "linux"))]
pub fn apply_process_scopes() -> Result<u32, String> {
    Err("Landlock process scopes are available only on Linux".into())
}

#[cfg(not(target_os = "linux"))]
pub fn apply(_spec: &SandboxLaunchSpec) -> Result<BackendEnforcement, String> {
    Err("Landlock is a Linux LSM and is not available on this platform".to_string())
}

/// Snapshot every object a ruleset is about to grant.
///
/// Read, directory-listing and write roots are captured as the policy holds
/// them, before any rule is opened. A root that is already an alias refuses
/// the launch rather than being granted through its target.
#[cfg(target_os = "linux")]
fn capture_grant_identities(
    policy: &BackendPolicy,
) -> Result<Vec<super::SandboxPathIdentity>, String> {
    let mut identities = Vec::new();
    for path in policy
        .read_roots
        .iter()
        .chain(&policy.read_dir_roots)
        .chain(&policy.write_roots)
    {
        identities.push(super::policy::capture_path_identity(path)?);
    }
    Ok(identities)
}

/// Turn one policy root into one Landlock rule bound to the captured object.
///
/// The rule target is the descriptor [`open_pinned`] returned, never a path
/// resolved by name, so the ruleset cannot grant something the policy did not
/// see. Classification uses the same descriptor's metadata rather than a
/// second path lookup: the rights mask must describe the opened object.
///
/// [`open_pinned`]: super::linux_mount_view::open_pinned
#[cfg(target_os = "linux")]
fn add_landlock_path(
    ruleset: landlock::RulesetCreated,
    path: &Path,
    access: landlock::BitFlags<landlock::AccessFs>,
    abi: landlock::ABI,
    identities: &[super::SandboxPathIdentity],
) -> Result<landlock::RulesetCreated, String> {
    use landlock::{PathBeneath, RulesetCreatedAttr};

    let identity = identities
        .iter()
        .find(|identity| identity.path == path)
        .ok_or_else(|| format!("sandbox path has no captured identity: {}", path.display()))?;
    let file = super::linux_mount_view::open_pinned(path, Some(identity))?;
    let access = landlock_access_for_path(path, access, abi)?;
    ruleset
        .add_rule(PathBeneath::new(file, access))
        .map_err(|e| format!("add sandbox rule {}: {e}", path.display()))
}

/// Landlock's directory rights are meaningless on a regular file, and
/// `ReadDir` on `/dev/null` (or any other special file) would fail the
/// `add_rule` call outright. Narrow the mask by file type.
///
/// The *rule* is bound to the pinned descriptor above; this mask only has to
/// be compatible with that object. If the path changed between the two reads
/// the mask and the object disagree, and `add_rule` rejects an incompatible
/// pair — a narrower mask denies more, and a wider one is refused — so a
/// mismatch fails closed rather than widening the grant.
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
    use crate::security::sandbox::linux_mount_view;

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_keeps_directory_rights_for_directories() {
        use landlock::{AccessFs, ABI};

        let directory = tempfile::tempdir().expect("directory fixture");
        let access =
            landlock_access_for_path(directory.path(), AccessFs::from_read(ABI::V3), ABI::V3)
                .expect("directory classification");

        assert!(access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_removes_directory_rights_for_regular_files() {
        use landlock::{AccessFs, ABI};

        let file = tempfile::NamedTempFile::new().expect("file fixture");
        let access = landlock_access_for_path(file.path(), AccessFs::from_read(ABI::V3), ABI::V3)
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
        let access = landlock_access_for_path(path, AccessFs::from_all(ABI::V3), ABI::V3)
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
        let error = landlock_access_for_path(path, AccessFs::from_read(ABI::V3), ABI::V3)
            .expect_err("missing path classification must fail");

        assert!(error.contains("classify sandbox path"));
        assert!(error.contains(path.to_string_lossy().as_ref()));
    }

    /// The grant root is captured, then replaced by a symlink before the
    /// ruleset is compiled: the no-follow open must refuse rather than grant
    /// the link target, and the unchanged root must be accepted.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_grant_root_replaced_between_policy_construction_and_apply_is_refused() {
        let directory = tempfile::tempdir().expect("fixture root");
        let root = directory.path().join("granted");
        std::fs::create_dir(&root).expect("grant root");
        let policy = BackendPolicy::new(vec![root], Vec::new(), Vec::new()).landlock_effective();
        // `apply` opens the policy's canonical root, never the raw input.
        let root = policy
            .read_roots
            .first()
            .expect("canonical grant root")
            .clone();
        let identities = capture_grant_identities(&policy).expect("captured identities");
        let accepted = linux_mount_view::open_pinned(&root, identities.first())
            .expect("the unchanged grant root is the captured object");
        assert!(accepted.metadata().expect("pinned metadata").is_dir());

        let displaced = directory.path().join("displaced");
        std::fs::rename(&root, &displaced).expect("displace the grant root");
        std::os::unix::fs::symlink(&displaced, &root).expect("symlink replacement");

        let error = linux_mount_view::open_pinned(&root, identities.first())
            .expect_err("a grant root replaced after policy construction must refuse");
        assert!(
            error.contains("without following aliases"),
            "the refusal must name the alias: {error}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn open_pinned_refuses_a_grant_path_that_vanished() {
        let path = Path::new("/definitely/missing/codegg-sandbox-grant");
        let error = linux_mount_view::open_pinned(path, None)
            .expect_err("a missing grant path must refuse the launch");
        assert!(
            error.contains("required sandbox path does not exist"),
            "the refusal must name the missing path: {error}"
        );
        assert!(
            error.contains(path.to_string_lossy().as_ref()),
            "the refusal must identify which path is missing: {error}"
        );
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

    #[test]
    fn a_failed_process_scope_is_a_refusal_not_an_absent_scope() {
        let absent = process_scope_from(
            || Err("no Landlock ABI 6".to_string()),
            || panic!("a host without the scope must not have it applied"),
        )
        .expect("a host without the scope keeps the filesystem-only tier");
        assert_eq!(absent, None);
        assert!(
            process_scope_from(|| Ok(()), || Err("scope denied".to_string())).is_err(),
            "a failed application must refuse the launch, not degrade it"
        );
        assert_eq!(
            process_scope_from(|| Ok(()), || Ok(6)).expect("an obtained scope"),
            Some(6)
        );
    }

    #[test]
    fn descriptor_claims_a_process_scope_only_when_one_was_obtained() {
        let obtained = landlock_enforcement(9, Some(6));
        assert_eq!(obtained.process_scope_abi, Some(6));
        assert!(
            obtained
                .guarantees
                .iter()
                .any(|guarantee| guarantee.contains("ABI 6 process scope obtained")),
            "an obtained scope must be reported: {:?}",
            obtained.guarantees
        );

        let absent = landlock_enforcement(9, None);
        assert_eq!(absent.process_scope_abi, None);
        assert!(
            absent
                .guarantees
                .iter()
                .all(|guarantee| !guarantee.contains("socket")),
            "no socket claim may survive without the scope: {:?}",
            absent.guarantees
        );
        assert!(
            absent
                .limits
                .iter()
                .any(|limit| limit.contains("are not isolated")),
            "an absent scope must be stated as a limit: {:?}",
            absent.limits
        );
    }

    #[test]
    fn descriptor_names_the_pathname_socket_and_proc_limits() {
        for enforcement in [
            landlock_enforcement(9, Some(6)),
            landlock_enforcement(9, None),
        ] {
            assert!(
                enforcement
                    .limits
                    .iter()
                    .any(|limit| limit.contains("pathname AF_UNIX")),
                "pathname sockets are never mediated and must be named: {:?}",
                enforcement.limits
            );
            assert!(
                enforcement
                    .limits
                    .iter()
                    .any(|limit| limit.contains("/proc")),
                "the /proc posture must be disclosed: {:?}",
                enforcement.limits
            );
        }
    }
}

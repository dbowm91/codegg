use crate::error::ToolError;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub use codegg_sandbox::LEGACY_HELPER_BACKEND;
pub use codegg_sandbox::{
    apply_backend, apply_landlock, backend, decode_sandbox_status, encode_process_scope_abi_record,
    encode_sandbox_status, escalation_for_outside_path, get_default_allowed_paths,
    get_sensitive_paths, guarantee, host_admission, landlock, platform_sandbox_capability, policy,
    probe_landlock, protected_mount_view_limits, resolve_sandbox_enforcement,
    sandbox_config_for_durable_payload, sandbox_config_for_profile,
    sandbox_config_for_profile_with_roots, sandbox_execution_path, sandbox_mode_for_profile,
    sandbox_profile_for_mode, seatbelt, validate_path_safety as validate_path_safety_library,
    verify_reported_backend, AttestedCapabilities, BackendEnforcement, BackendId, BackendPolicy,
    CapabilityFact, Refusal, RequestedGuarantees, SandboxCapability, SandboxConfig, SandboxError,
    SandboxExecutionPath, SandboxLaunchOutcome, SandboxLaunchSpec, SandboxMode,
    SandboxPathIdentity, SandboxStatusFrame, SatisfiedContract, BACKENDS, MAX_SANDBOX_SPEC_BYTES,
    MAX_SANDBOX_STATUS_BYTES, PROCESS_SCOPE_ABI_RECORD, SANDBOX_ACK_FD, SANDBOX_STATUS_FD,
    SANDBOX_STATUS_VERSION, UNCONTAINED,
};
#[cfg(target_os = "linux")]
pub use codegg_sandbox::{linux_launch, linux_mount_view, linux_seccomp};

pub(crate) fn map_sandbox_error(error: SandboxError) -> ToolError {
    match error {
        SandboxError::Permission(message) => ToolError::Permission(message),
    }
}

/// The parent resolves the helper only from the installation-owned sibling.
pub fn sandbox_helper_path() -> Result<PathBuf, String> {
    crate::install::trusted_sandbox_helper_path()
        .map_err(|error| error.replace("installation-owned", "trusted sandbox helper"))
}

#[allow(dead_code)]
pub(crate) fn resolve_trusted_helper(current: &Path) -> Result<PathBuf, String> {
    crate::install::trusted_sandbox_helper_path_for(current)
        .map_err(|error| error.replace("installation-owned", "trusted sandbox helper"))
}

/// Keep the root package's public error contract while the library stays root-free.
pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError> {
    validate_path_safety_library(path, allowed_paths).map_err(map_sandbox_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn trusted_helper_resolution_ignores_inherited_override() {
        use std::os::unix::fs::PermissionsExt;

        let install = tempfile::tempdir().expect("installation directory");
        let executable = install.path().join("codegg");
        let helper = install.path().join("codegg-sandbox-helper");
        std::fs::write(&executable, b"codegg").expect("executable fixture");
        std::fs::write(&helper, b"helper").expect("helper fixture");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("helper executable permissions");

        let variable = ["CODEGG", "SANDBOX", "HELPER"].join("_");
        let substitution = install.path().join("substituted-helper");
        std::fs::write(&substitution, b"substitution").expect("substitution fixture");
        std::env::set_var(&variable, &substitution);
        let resolved = resolve_trusted_helper(&executable).expect("trusted sibling helper");
        std::env::remove_var(variable);

        assert_eq!(resolved, helper.canonicalize().expect("canonical helper"));
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
        let mut config = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite);
        config.allowed_paths = vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];
        let spec = config
            .launch_spec(&argv[0], &argv[1..], Some(&first))
            .expect("Windows policy inputs");
        for root in [&first, &second] {
            let resolved = std::fs::canonicalize(root).expect("resolved worktree");
            assert!(
                spec.write_paths.contains(&resolved),
                "worktree root must remain a distinct write input: {resolved:?}"
            );
        }
    }
}

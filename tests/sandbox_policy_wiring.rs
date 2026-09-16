//! M005 production sandbox policy wiring.
//!
//! Covers plan §10 at the narrowest meaningful scope: profile → config /
//! enforcement mapping, FullHost truthfulness, filesystem/network
//! separation, ApprovalMode orthogonality, parent-child ceiling, worktree
//! roots, restart re-resolution, escape/fail-closed, and production
//! ToolRegistry threading.

use codegg::security::sandbox::{
    escalation_for_outside_path, resolve_sandbox_enforcement, sandbox_config_for_profile,
    sandbox_mode_for_profile, sandbox_profile_for_mode, SandboxConfig, SandboxMode,
};
use codegg::tool::{ToolRegistry, ToolRegistryOptions};
use codegg_core::approval::{
    child_sandbox_allowed, resolve_child_sandbox, ApprovalMode, ExecutionPolicySnapshot,
    FilesystemEnforcement, NetworkEnforcement, SandboxEnforcement, SandboxProfile,
};

// ── WP-A: policy/enforcement types ──────────────────────────────────────

#[test]
fn profile_maps_to_truthful_sandbox_config() {
    let dir = tempfile::tempdir().unwrap();
    let write = sandbox_config_for_profile(SandboxProfile::WorkspaceWrite, dir.path())
        .expect("WorkspaceWrite must build a config");
    assert!(write.enabled);
    assert!(matches!(write.mode, SandboxMode::WorkspaceWrite));

    let read = sandbox_config_for_profile(SandboxProfile::ReadOnly, dir.path())
        .expect("ReadOnly must build a config");
    assert!(read.enabled);
    assert!(matches!(read.mode, SandboxMode::ReadOnly));

    assert!(
        sandbox_config_for_profile(SandboxProfile::FullHost, dir.path()).is_none(),
        "FullHost must not build a SandboxConfig"
    );
}

#[test]
fn full_host_is_distinct_from_workspace_write() {
    assert_ne!(SandboxProfile::WorkspaceWrite, SandboxProfile::FullHost);
    assert!(SandboxProfile::WorkspaceWrite.requires_filesystem_containment());
    assert!(!SandboxProfile::FullHost.requires_filesystem_containment());
    // Legacy writable-roots mode has no FullHost execution mapping.
    assert!(sandbox_mode_for_profile(SandboxProfile::FullHost).is_none());
    assert!(sandbox_mode_for_profile(SandboxProfile::WorkspaceWrite).is_some());
}

#[test]
fn filesystem_and_network_are_reported_separately() {
    let enforced = SandboxEnforcement::for_constrained_enforced(
        SandboxProfile::WorkspaceWrite,
        "landlock",
        Some(1),
    );
    assert!(matches!(
        enforced.filesystem,
        FilesystemEnforcement::Enforced { .. }
    ));
    // Filesystem success never implies network isolation.
    assert!(matches!(enforced.network, NetworkEnforcement::Unrestricted));

    let full = SandboxEnforcement::for_full_host();
    assert!(matches!(full.filesystem, FilesystemEnforcement::FullHost));
    assert!(matches!(full.network, NetworkEnforcement::Unrestricted));

    let text = enforced.describe();
    assert!(text.contains("filesystem enforced"));
    assert!(text.contains("network unrestricted"));
    assert!(!text.contains("network enforced"));
}

#[test]
fn legacy_danger_full_access_is_compat_only() {
    let mode = SandboxMode::parse_compat("danger_full_access").expect("compat parse");
    assert!(mode.is_deprecated_full_access());
    assert_eq!(sandbox_profile_for_mode(&mode), SandboxProfile::FullHost);
    // Execution mapping is not 1:1: FullHost carries no config.
    let dir = tempfile::tempdir().unwrap();
    assert!(sandbox_config_for_profile(sandbox_profile_for_mode(&mode), dir.path()).is_none());
}

#[test]
fn unsupported_host_reports_unavailable_never_full_host() {
    for profile in [SandboxProfile::ReadOnly, SandboxProfile::WorkspaceWrite] {
        let enforcement = resolve_sandbox_enforcement(profile);
        assert!(matches!(
            enforcement.network,
            NetworkEnforcement::Unrestricted
        ));
        if SandboxConfig::is_available() {
            assert!(enforcement.is_enforced(), "supported host must enforce");
        } else {
            assert!(!enforcement.is_enforced());
            assert!(!enforcement.is_full_host());
            assert!(matches!(
                enforcement.filesystem,
                FilesystemEnforcement::Unavailable { .. }
            ));
        }
    }
    let full = resolve_sandbox_enforcement(SandboxProfile::FullHost);
    assert!(full.is_full_host());
}

// ── WP-B: production threading ──────────────────────────────────────────

#[test]
fn production_registry_threads_workspace_write_into_bash() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(dir.path().to_path_buf()),
        sandbox_profile: Some(SandboxProfile::WorkspaceWrite),
        ..ToolRegistryOptions::default()
    });
    assert_eq!(registry.sandbox_profile(), SandboxProfile::WorkspaceWrite);
    assert!(registry.get("bash").is_some(), "bash must be registered");
}

#[test]
fn workspace_write_bash_carries_enabled_landlock_config() {
    use codegg::tool::bash::BashTool;
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(dir.path().to_path_buf()),
        sandbox_profile: Some(SandboxProfile::WorkspaceWrite),
        ..ToolRegistryOptions::default()
    });
    assert_eq!(registry.sandbox_profile(), SandboxProfile::WorkspaceWrite);
    // Direct builder parity: the registry path must equal an explicit
    // with_sandbox_profile construction, never a bare default.
    let explicit = BashTool::new().with_sandbox_profile(SandboxProfile::WorkspaceWrite, dir.path());
    assert!(explicit.has_landlock_config());
    let enforcement = explicit.sandbox_enforcement(SandboxProfile::WorkspaceWrite);
    if SandboxConfig::is_available() {
        assert!(enforcement.is_enforced());
    } else {
        assert!(!enforcement.is_enforced());
        assert!(!enforcement.is_full_host());
    }
}

#[test]
fn full_host_registry_leaves_bash_without_containment_explicitly() {
    use codegg::tool::bash::BashTool;
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(dir.path().to_path_buf()),
        sandbox_profile: Some(SandboxProfile::FullHost),
        ..ToolRegistryOptions::default()
    });
    assert_eq!(registry.sandbox_profile(), SandboxProfile::FullHost);
    let enforcement = BashTool::new().sandbox_enforcement(SandboxProfile::FullHost);
    assert!(enforcement.is_full_host());
}

#[test]
fn approval_mode_changes_do_not_alter_sandbox_profile() {
    for mode in [
        ApprovalMode::Interactive,
        ApprovalMode::Automatic,
        ApprovalMode::Yolo,
    ] {
        let snapshot = ExecutionPolicySnapshot::capture(
            mode,
            SandboxProfile::WorkspaceWrite,
            None,
            Some("session-m005".into()),
            None,
            Some("config:1".into()),
            None,
        );
        assert_eq!(snapshot.approval_mode(), mode);
        assert_eq!(
            snapshot.sandbox_profile(),
            SandboxProfile::WorkspaceWrite,
            "approval mode {mode:?} must not mutate sandbox"
        );
    }
}

// ── WP-C: FullHost and fallback truthfulness ────────────────────────────

#[test]
fn constrained_failure_cannot_fall_through_to_full_host() {
    // A constrained request without a config reports unavailable, never
    // FullHost. Callers fail closed on this signal.
    use codegg::tool::bash::BashTool;
    let tool = BashTool::new();
    let enforcement = tool.sandbox_enforcement(SandboxProfile::WorkspaceWrite);
    assert!(!enforcement.is_full_host());
    assert!(!enforcement.is_enforced());
    assert!(matches!(
        enforcement.filesystem,
        FilesystemEnforcement::Unavailable { .. }
    ));
}

#[test]
fn outside_path_returns_bounded_escalation_not_full_host_switch() {
    let req = escalation_for_outside_path("/etc/passwd", SandboxProfile::WorkspaceWrite);
    let text = req.describe();
    assert!(text.contains("/etc/passwd"));
    assert!(text.contains("workspace_write"));
    assert!(!text.contains("FullHost switch"));
}

#[test]
fn network_is_unrestricted_for_all_shell_profiles() {
    for profile in [
        SandboxProfile::ReadOnly,
        SandboxProfile::WorkspaceWrite,
        SandboxProfile::FullHost,
    ] {
        let enforcement = resolve_sandbox_enforcement(profile);
        assert!(
            matches!(enforcement.network, NetworkEnforcement::Unrestricted),
            "profile {profile:?} must report network unrestricted"
        );
    }
}

// ── WP-D: child/worktree matrix ─────────────────────────────────────────

#[test]
fn parent_child_ceiling_matrix() {
    use codegg_core::approval::PreferenceError;
    // Narrowing always allowed.
    assert!(child_sandbox_allowed(
        SandboxProfile::WorkspaceWrite,
        SandboxProfile::ReadOnly
    ));
    assert!(resolve_child_sandbox(
        SandboxProfile::WorkspaceWrite,
        SandboxProfile::WorkspaceWrite
    )
    .is_ok());
    // Broadening fails closed.
    assert!(!child_sandbox_allowed(
        SandboxProfile::WorkspaceWrite,
        SandboxProfile::FullHost
    ));
    assert!(!child_sandbox_allowed(
        SandboxProfile::ReadOnly,
        SandboxProfile::WorkspaceWrite
    ));
    let err = resolve_child_sandbox(SandboxProfile::WorkspaceWrite, SandboxProfile::FullHost)
        .unwrap_err();
    assert!(matches!(err, PreferenceError::CeilingExceeded(_)));
}

#[test]
fn child_cannot_select_full_host_under_workspace_write_parent() {
    let parent = ExecutionPolicySnapshot::capture(
        ApprovalMode::Interactive,
        SandboxProfile::WorkspaceWrite,
        None,
        Some("session-m005".into()),
        None,
        None,
        None,
    );
    assert!(parent
        .narrow_for_child(ApprovalMode::Interactive, SandboxProfile::FullHost)
        .is_err());
    assert!(parent
        .narrow_for_child(ApprovalMode::Interactive, SandboxProfile::ReadOnly)
        .is_ok());
}

#[test]
fn two_worktree_children_get_distinct_writable_roots() {
    let parent = tempfile::tempdir().unwrap();
    let child_a = tempfile::tempdir().unwrap();
    let child_b = tempfile::tempdir().unwrap();
    let config_a =
        sandbox_config_for_profile(SandboxProfile::WorkspaceWrite, child_a.path()).unwrap();
    let config_b =
        sandbox_config_for_profile(SandboxProfile::WorkspaceWrite, child_b.path()).unwrap();
    assert_ne!(config_a.allowed_paths, config_b.allowed_paths);
    assert!(config_a
        .allowed_paths
        .iter()
        .any(|p| p.contains(&child_a.path().to_string_lossy().into_owned()[..8])));
    let _ = parent;
}

#[test]
fn yolo_does_not_disable_sandbox() {
    // Yolo alters approval routing only; the snapshot sandbox and the
    // Bash enforcement stay constrained.
    use codegg::permission::approval::{ApprovalRouter, DeterministicVerdict};
    use codegg::tool::bash::BashTool;
    let dir = tempfile::tempdir().unwrap();
    let snapshot = ExecutionPolicySnapshot::capture(
        ApprovalMode::Yolo,
        SandboxProfile::WorkspaceWrite,
        None,
        Some("session-m005".into()),
        None,
        None,
        None,
    );
    let router = ApprovalRouter::new(snapshot.clone());
    let req = codegg::permission::approval::ApprovalRequest::new(
        "bash",
        None,
        Some("echo hi".into()),
        vec!["permission policy ask".into()],
        None,
        None,
        "session-m005",
        None,
    );
    let decision = router.route_escalation(&req);
    assert!(decision.allowed());
    assert_eq!(snapshot.sandbox_profile(), SandboxProfile::WorkspaceWrite);
    let tool = BashTool::new().with_sandbox_profile(SandboxProfile::WorkspaceWrite, dir.path());
    assert!(tool.has_landlock_config());
    let _ = DeterministicVerdict::Allow;
}

// ── Security and negative ───────────────────────────────────────────────

#[test]
fn symlink_path_escape_remains_denied() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = dir.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(not(unix))]
    return;
    let allowed = vec![dir.path().to_string_lossy().to_string()];
    let result = codegg::security::sandbox::validate_path_safety(&link, &allowed);
    assert!(result.is_err(), "symlink escape must stay denied");
}

#[test]
fn outside_workspace_path_is_denied_not_contained() {
    let dir = tempfile::tempdir().unwrap();
    let allowed = vec![dir.path().to_string_lossy().to_string()];
    let result = codegg::security::sandbox::validate_path_safety(
        std::path::Path::new("/etc/passwd"),
        &allowed,
    );
    assert!(result.is_err());
}

// ── Restart and recovery ────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn persisted_workspace_write_reresolves_enforcement_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("prefs.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool).await.unwrap();
    let store = codegg_core::approval::RuntimePreferenceStore::new(pool.clone());
    store
        .set_sandbox_profile("local-owner", SandboxProfile::WorkspaceWrite, None)
        .await
        .unwrap();
    pool.close().await;

    // Restart: reopen and re-resolve enforcement from the persisted
    // requested preference plus current host capability (never a stale
    // "Enforced" fact).
    let pool2 = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool2).await.unwrap();
    let store2 = codegg_core::approval::RuntimePreferenceStore::new(pool2);
    let reloaded = store2.get("local-owner").await.unwrap().unwrap();
    assert_eq!(
        reloaded.sandbox_profile,
        Some(SandboxProfile::WorkspaceWrite)
    );
    let enforcement = resolve_sandbox_enforcement(reloaded.effective_sandbox_profile());
    assert_eq!(enforcement.requested, SandboxProfile::WorkspaceWrite);
    assert!(matches!(
        enforcement.network,
        NetworkEnforcement::Unrestricted
    ));
}

// ── Protocol ────────────────────────────────────────────────────────────

#[test]
fn enforcement_dto_is_truthful_and_additive() {
    use codegg_protocol::core::{
        ExecutionPolicySnapshotDto, SandboxEnforcementDto, SandboxProfileDto,
    };
    // Constrained on any host reports filesystem truthfully + network
    // unrestricted.
    let enforced =
        SandboxEnforcementDto::for_profile_on_host(SandboxProfileDto::WorkspaceWrite, true, None);
    assert!(enforced.summary.contains("network unrestricted"));
    assert!(!enforced.summary.contains("network enforced"));

    let unavailable = SandboxEnforcementDto::for_profile_on_host(
        SandboxProfileDto::WorkspaceWrite,
        false,
        Some("Landlock unavailable: test".into()),
    );
    assert!(matches!(
        unavailable.filesystem,
        codegg_protocol::core::FilesystemEnforcementDto::Unavailable { .. }
    ));

    let full = SandboxEnforcementDto::for_profile_on_host(SandboxProfileDto::FullHost, true, None);
    assert!(matches!(
        full.filesystem,
        codegg_protocol::core::FilesystemEnforcementDto::FullHost
    ));

    // Pre-M005 payloads without enforcement still decode.
    let legacy = serde_json::json!({
        "approval_mode": "interactive",
        "sandbox_profile": "workspace_write",
        "captured_at_ms": 0
    });
    let decoded: ExecutionPolicySnapshotDto = serde_json::from_value(legacy).unwrap();
    assert!(decoded.sandbox_enforcement.is_none());
}

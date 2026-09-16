//! M007 integration: Yolo/Automatic/FullHost user surfaces.
//!
//! Covers the frontend-neutral contract across crates without standing up
//! a daemon: protocol DTO evolution, capability-scoped remembered
//! approvals (persistence round-trip plus legacy readability), the
//! warning/confirmation matrix, and effective-state rendering. Daemon
//! handler behavior (`RuntimePolicySet` validation/CAS, reviewer
//! availability) is exercised through the store and DTO layers that the
//! handler is built on; live daemon round-trips stay out per the
//! verification policy (no live-provider/network CI).

use codegg::permission::{
    decision_scope_for_git_subcommand, decision_scope_for_shell_command, PermissionChecker,
    PermissionLevel, PermissionResult, PermissionStore,
};
use codegg::policy_surface::{
    format_policy_detail, format_policy_line, format_restore_summary, resolve_cli_policy,
    warning_for, EffectivePolicyView, WarningLevel,
};

// ── Protocol evolution ──────────────────────────────────────────────

#[test]
fn legacy_policy_snapshot_json_decodes_with_m007_defaults() {
    // Pre-M007 payload: no reviewer or enforcement fields.
    let legacy = serde_json::json!({
        "approval_mode": "automatic",
        "sandbox_profile": "workspace_write",
        "captured_at_ms": 0
    });
    let decoded: codegg_protocol::core::ExecutionPolicySnapshotDto =
        serde_json::from_value(legacy).unwrap();
    assert!(!decoded.reviewer_available);
    assert_eq!(decoded.reviewer_detail, "");
    assert_eq!(decoded.sandbox_enforcement, None);

    // New fields round-trip.
    let mut full = decoded.clone();
    full.reviewer_available = true;
    full.reviewer_detail = String::new();
    let back: codegg_protocol::core::ExecutionPolicySnapshotDto =
        serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
    assert_eq!(back, full);
}

#[test]
fn legacy_preference_json_decodes_without_model_identity() {
    // Pre-M007 payload: no last-model fields.
    let legacy = serde_json::json!({
        "principal_id": "local-owner",
        "approval_mode": "yolo",
        "sandbox_profile": "workspace_write",
        "revision": 2,
        "updated_at_ms": 0
    });
    let decoded: codegg_protocol::core::RuntimePreferenceDto =
        serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.last_provider_connection_id, None);
    assert_eq!(decoded.last_model_id, None);

    let mut full = decoded.clone();
    full.last_provider_connection_id = Some("conn-1".to_string());
    full.last_model_id = Some("model-1".to_string());
    let back: codegg_protocol::core::RuntimePreferenceDto =
        serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
    assert_eq!(back, full);
}

#[test]
fn runtime_policy_set_wire_shape_is_optional_per_dimension() {
    use codegg_protocol::core::CoreRequest;
    // Partial update: only the sandbox profile.
    let partial = serde_json::json!({
        "type": "runtime_policy_set",
        "sandbox_profile": "full_host"
    });
    let decoded: CoreRequest = serde_json::from_value(partial).unwrap();
    assert!(matches!(
        decoded,
        CoreRequest::RuntimePolicySet {
            approval_mode: None,
            sandbox_profile: Some(_),
            expected_revision: None,
        }
    ));
    // Full update with CAS revision.
    let full = serde_json::json!({
        "type": "runtime_policy_set",
        "approval_mode": "yolo",
        "sandbox_profile": "workspace_write",
        "expected_revision": 3
    });
    let decoded: CoreRequest = serde_json::from_value(full).unwrap();
    assert!(matches!(
        decoded,
        CoreRequest::RuntimePolicySet {
            approval_mode: Some(_),
            sandbox_profile: Some(_),
            expected_revision: Some(3),
        }
    ));
}

// ── Warning / confirmation matrix ───────────────────────────────────

#[test]
fn every_combination_has_a_warning_with_bounded_confirmations() {
    use codegg_core::approval::{ApprovalMode, SandboxProfile};
    for mode in [
        ApprovalMode::Interactive,
        ApprovalMode::Automatic,
        ApprovalMode::Yolo,
    ] {
        for profile in [
            SandboxProfile::ReadOnly,
            SandboxProfile::WorkspaceWrite,
            SandboxProfile::FullHost,
        ] {
            let warning = warning_for(mode, profile);
            assert!(!warning.title.is_empty());
            assert!(!warning.body.is_empty());
            assert!(warning.confirmations_required <= 2);
            if profile == SandboxProfile::FullHost {
                assert_eq!(warning.level, WarningLevel::Strong);
                assert!(warning.confirmations_required >= 1);
            }
            if mode == ApprovalMode::Yolo && profile == SandboxProfile::FullHost {
                assert_eq!(warning.confirmations_required, 2);
            }
            if mode == ApprovalMode::Interactive && !profile.is_full_host() {
                assert_eq!(warning.level, WarningLevel::Info);
                assert_eq!(warning.confirmations_required, 0);
            }
        }
    }
}

#[test]
fn automatic_modes_never_imply_full_host() {
    use codegg_core::approval::{ApprovalMode, SandboxProfile};
    for profile in [SandboxProfile::ReadOnly, SandboxProfile::WorkspaceWrite] {
        let warning = warning_for(ApprovalMode::Automatic, profile);
        let joined = warning.body.join(" ").to_lowercase();
        assert!(!joined.contains("no codegg filesystem containment"));
        assert_ne!(warning.level, WarningLevel::Strong);
    }
}

// ── Effective-state rendering ───────────────────────────────────────

fn plain_view() -> EffectivePolicyView {
    EffectivePolicyView {
        requested_mode: codegg_core::approval::ApprovalMode::Yolo,
        effective_mode: codegg_core::approval::ApprovalMode::Yolo,
        requested_profile: codegg_core::approval::SandboxProfile::WorkspaceWrite,
        effective_profile: codegg_core::approval::SandboxProfile::WorkspaceWrite,
        enforcement_summary: "workspace_write requested; filesystem unavailable (no landlock); network unrestricted (no OS isolation)".to_string(),
        reviewer_available: false,
        reviewer_detail: String::new(),
        revision: Some(3),
    }
}

#[test]
fn effective_line_distinguishes_request_from_ceiling() {
    let plain = plain_view();
    assert!(!plain.is_degraded());
    assert!(format_policy_line(&plain).contains("approval:yolo"));

    let narrowed = EffectivePolicyView {
        effective_mode: codegg_core::approval::ApprovalMode::Interactive,
        ..plain.clone()
    };
    assert!(narrowed.is_degraded());
    let detail = format_policy_detail(&narrowed).join("\n");
    assert!(detail.contains("Requested: approval=yolo"));
    assert!(detail.contains("Effective: approval=interactive"));
    assert!(detail.contains("ceiling"));
}

#[test]
fn automatic_without_reviewer_is_visibly_degraded_never_yolo() {
    let view = EffectivePolicyView {
        requested_mode: codegg_core::approval::ApprovalMode::Automatic,
        effective_mode: codegg_core::approval::ApprovalMode::Automatic,
        requested_profile: codegg_core::approval::SandboxProfile::WorkspaceWrite,
        effective_profile: codegg_core::approval::SandboxProfile::WorkspaceWrite,
        enforcement_summary: String::new(),
        reviewer_available: false,
        reviewer_detail: "no reviewer model configured".to_string(),
        revision: Some(1),
    };
    assert!(view.is_degraded());
    let line = format_policy_line(&view);
    assert!(line.contains("reviewer unavailable"));
    assert!(!line.contains("yolo"));
    let detail = format_policy_detail(&view).join("\n");
    assert!(detail.contains("defers to you"));
    assert!(detail.contains("never silent Yolo"));
}

#[test]
fn restore_summary_announces_fallbacks() {
    let view = plain_view();
    let clean = format_restore_summary(&view, &[]);
    assert!(clean.contains("approval=yolo"));
    assert!(clean.contains("revision 3"));
    assert!(!clean.contains("Fallback"));

    let with_fallback = format_restore_summary(
        &view,
        &["stored preference was narrowed by a project or administrator ceiling".to_string()],
    );
    assert!(with_fallback.contains("Fallback: stored preference was narrowed"));
}

// ── CLI mapping ─────────────────────────────────────────────────────

#[test]
fn cli_flags_map_to_the_daemon_contract() {
    use codegg_core::approval::{ApprovalMode, SandboxProfile};
    assert_eq!(resolve_cli_policy(None, None, false).unwrap(), None);
    let yolo = resolve_cli_policy(Some("yolo"), Some("workspace-write"), false).unwrap();
    assert_eq!(yolo.unwrap().approval_mode, Some(ApprovalMode::Yolo));
    let sandbox = resolve_cli_policy(None, Some("full_host"), false).unwrap();
    assert_eq!(
        sandbox.unwrap().sandbox_profile,
        Some(SandboxProfile::FullHost)
    );
    // `--yolo` is exactly `--approval-mode yolo`.
    assert_eq!(
        resolve_cli_policy(None, None, true).unwrap(),
        resolve_cli_policy(Some("yolo"), None, false).unwrap()
    );
    // Conflicting spellings are rejected, never silently preferred.
    assert!(resolve_cli_policy(Some("interactive"), None, true).is_err());
    assert!(resolve_cli_policy(Some("nope"), None, false).is_err());
    assert!(resolve_cli_policy(None, Some("nope"), false).is_err());
}

// ── Capability-scoped remembered approvals ──────────────────────────

#[test]
fn scope_normalization_is_deterministic_and_bounded() {
    assert_eq!(
        decision_scope_for_shell_command("cargo test --all"),
        Some("cmd:cargo".to_string())
    );
    assert_eq!(
        decision_scope_for_shell_command("FOO=1 sudo /usr/bin/cargo test"),
        Some("cmd:cargo".to_string())
    );
    assert_eq!(
        decision_scope_for_git_subcommand("push origin main"),
        Some("git:push".to_string())
    );
    assert_eq!(decision_scope_for_shell_command(""), None);
}

#[test]
fn scoped_store_round_trips_through_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.json");
    {
        let mut store = PermissionStore::new(Some(path.clone()));
        assert!(store.add_decision("bash", None, PermissionLevel::Ask, None,));
        // Narrow sibling row coexists with the broad row.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let checker = PermissionChecker::new(None, Some(path.clone()));
            assert!(
                checker
                    .always_allow_scoped("bash", None, Some("cmd:cargo"), None)
                    .await
            );
        });
    }
    // Reload from disk: both rows survive.
    let reloaded = PermissionStore::new(Some(path));
    assert_eq!(
        reloaded.get_decision_scoped("bash", None, Some("cmd:cargo"), None),
        Some(PermissionLevel::Allow)
    );
    // Exact scope misses fall back to the broad row (Ask here).
    assert_eq!(
        reloaded.get_decision_scoped("bash", None, Some("cmd:rm"), None),
        Some(PermissionLevel::Ask)
    );
}

#[test]
fn legacy_permission_file_without_scope_stays_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.json");
    std::fs::write(
        &path,
        r#"[{"tool":"bash","path":null,"level":"allow","created_at":0,"signature":"","session_id":null}]"#,
    )
    .unwrap();
    let store = PermissionStore::new(Some(path));
    // Legacy broad grant authorizes every scope (conservative compat).
    assert_eq!(
        store.get_decision_scoped("bash", None, Some("cmd:cargo"), None),
        Some(PermissionLevel::Allow)
    );
    assert_eq!(
        store.get_decision("bash", None, None),
        Some(PermissionLevel::Allow)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cargo_allow_does_not_authorize_unrelated_destructive_shell() {
    let checker = PermissionChecker::new(None, None);
    assert!(
        checker
            .always_allow_scoped("bash", None, Some("cmd:cargo"), None)
            .await
    );
    // The remembered family is honored.
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("cargo test --all"), None)
            .await,
        PermissionResult::Allow
    ));
    // An unrelated catastrophic command still escalates.
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("rm -rf /"), None)
            .await,
        PermissionResult::Ask(_)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn remote_mutation_scope_does_not_imply_arbitrary_shell() {
    let checker = PermissionChecker::new(None, None);
    assert!(
        checker
            .always_allow_scoped("git", None, Some("git:commit"), None)
            .await
    );
    assert!(matches!(
        checker
            .check_with_args("git", None, Some("commit -m x"), None)
            .await,
        PermissionResult::Allow
    ));
    assert!(!matches!(
        checker
            .check_with_args("git", None, Some("push origin main"), None)
            .await,
        PermissionResult::Allow
    ));
}

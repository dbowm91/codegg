//! M003 integration: ApprovalRouter + durable mode state.
//!
//! Covers the plan §10 integration/restart/contention/security/migration
//! bullets at the narrowest meaningful scope: router routing, human-wait
//! UX preservation, Yolo/Automatic semantics, durable preference + decision
//! restart, CAS contention, child ceilings, and protocol compatibility.

use codegg::bus::{PermissionDecision, PermissionRegistry};
use codegg::permission::approval::{
    source as approval_source, ApprovalMode, ApprovalRequest, ApprovalRouter, DeterministicVerdict,
    ExecutionPolicySnapshot, SandboxProfile,
};
use codegg::permission::{PermissionChecker, PermissionLevel, PermissionResult};
use codegg_core::approval::{child_mode_allowed, RuntimePreferenceStore};

fn approval_request() -> ApprovalRequest {
    ApprovalRequest::new(
        "edit",
        Some("/tmp/work/file.txt".into()),
        Some("edit summary".into()),
        vec!["permission policy ask".into()],
        None,
        Some("config:1".into()),
        "session-it",
        None,
    )
}

fn snapshot_for(mode: ApprovalMode) -> ExecutionPolicySnapshot {
    ExecutionPolicySnapshot::capture(
        mode,
        SandboxProfile::WorkspaceWrite,
        None,
        Some("session-it".into()),
        Some("build".into()),
        Some("config:1".into()),
        None,
    )
}

#[test]
fn safe_tool_remains_no_prompt() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let checker = PermissionChecker::new(None, None);
        let result = checker.check("read", None, Some("session-it")).await;
        assert!(matches!(result, PermissionResult::Allow));
    });
}

#[tokio::test(flavor = "current_thread")]
async fn destructive_ask_in_interactive_emits_one_human_request() {
    let snapshot = snapshot_for(ApprovalMode::Interactive);
    let router = ApprovalRouter::new(snapshot);
    let request = approval_request();
    let perm_id = "it-perm-1";

    let mut rx = codegg::bus::global::GlobalEventBus::subscribe();
    let request_clone = request.clone();
    let responder = tokio::spawn(async move {
        // Wait for the single PermissionPending, then allow.
        let mut seen = 0;
        loop {
            match rx.recv().await {
                Ok(codegg::bus::events::AppEvent::PermissionPending { perm_id: pid, .. })
                    if pid == perm_id =>
                {
                    seen += 1;
                    assert_eq!(seen, 1, "exactly one human request expected");
                    let sent = PermissionRegistry::respond_scoped(
                        &request_clone.session_id,
                        perm_id,
                        PermissionDecision::AllowOnce,
                    );
                    assert!(sent);
                    return;
                }
                Ok(_) => continue,
                Err(_) => return,
            }
        }
    });

    let outcome = router
        .request_human_approval(perm_id, &request, Some(serde_json::json!({})))
        .await;
    assert!(outcome.allow);
    assert!(!outcome.persist);
    responder.await.unwrap();
    // Pending request is unregistered after resolution.
    assert!(!PermissionRegistry::is_registered_scoped(
        &request.session_id,
        perm_id
    ));
}

#[test]
fn same_escalate_in_yolo_allows_with_yolo_receipt() {
    let router = ApprovalRouter::new(snapshot_for(ApprovalMode::Yolo));
    let request = approval_request();
    let decision = router.route_escalation(&request);
    assert!(matches!(
        decision,
        codegg::permission::approval::ApprovalDecision::Allow { .. }
    ));
    assert_eq!(decision.source(), approval_source::YOLO);
}

#[test]
fn security_deny_remains_denied_in_yolo() {
    let router = ApprovalRouter::new(snapshot_for(ApprovalMode::Yolo));
    let decision = router.decide_deterministic(&DeterministicVerdict::Deny {
        reason: "critical command".into(),
        source: approval_source::SECURITY_DENY.into(),
    });
    assert!(!decision.allowed());
    assert_ne!(decision.source(), approval_source::YOLO);
}

#[test]
fn automatic_placeholder_defers_safely_never_allows() {
    for rollout in [false, true] {
        let router = ApprovalRouter::new(snapshot_for(ApprovalMode::Automatic))
            .with_automatic_rollout(rollout);
        let decision = router.route_escalation(&approval_request());
        assert!(!decision.allowed());
        assert_eq!(decision.source(), approval_source::AUTOMATIC_DEFER);
    }
}

#[test]
fn hard_deny_always_wins() {
    for mode in [
        ApprovalMode::Interactive,
        ApprovalMode::Automatic,
        ApprovalMode::Yolo,
    ] {
        let router = ApprovalRouter::new(snapshot_for(mode));
        let decision = router.decide_deterministic(&DeterministicVerdict::Deny {
            reason: "explicit deny".into(),
            source: approval_source::PERMISSION_DENY.into(),
        });
        assert!(!decision.allowed());
    }
}

#[test]
fn child_cannot_select_broader_mode_than_parent() {
    assert!(!child_mode_allowed(
        ApprovalMode::Interactive,
        ApprovalMode::Yolo
    ));
    let parent = snapshot_for(ApprovalMode::Interactive);
    assert!(parent
        .narrow_for_child(ApprovalMode::Yolo, SandboxProfile::WorkspaceWrite)
        .is_err());
}

#[test]
fn yolo_cannot_override_explicit_deny() {
    // Explicit Deny in the store wins even for Yolo: the deterministic
    // layer returns Deny before routing, so the router never sees an
    // escalation to auto-allow.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let checker = PermissionChecker::new(None, None);
        checker
            .always_deny("edit", Some("/tmp/work/file.txt"), Some("session-it"))
            .await;
        let result = checker
            .check("edit", Some("/tmp/work/file.txt"), Some("session-it"))
            .await;
        assert!(matches!(result, PermissionResult::Deny));
    });
}

#[test]
fn mode_change_races_pending_approval_uses_captured_snapshot() {
    // Capture Interactive, then toggle live mode to Yolo: the captured
    // snapshot still routes as Interactive (defer, not auto-allow).
    let captured = snapshot_for(ApprovalMode::Interactive);
    let router = ApprovalRouter::new(captured);
    let live = snapshot_for(ApprovalMode::Yolo);
    assert_eq!(live.approval_mode(), ApprovalMode::Yolo);
    let decision = router.route_escalation(&approval_request());
    assert!(
        !decision.allowed(),
        "captured Interactive must not become Yolo"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn approval_preference_survives_daemon_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("prefs.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool).await.unwrap();
    let store = RuntimePreferenceStore::new(pool.clone());
    let written = store
        .set_approval_mode("local-owner", ApprovalMode::Yolo, None)
        .await
        .unwrap();
    assert_eq!(written.revision, 1);
    let sandboxed = store
        .set_sandbox_profile("local-owner", SandboxProfile::ReadOnly, Some(1))
        .await
        .unwrap();
    assert_eq!(sandboxed.revision, 2);
    pool.close().await;

    // Restart: reopen the same file and reload.
    let pool2 = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool2).await.unwrap();
    let store2 = RuntimePreferenceStore::new(pool2);
    let reloaded = store2.get("local-owner").await.unwrap().unwrap();
    assert_eq!(reloaded.approval_mode, Some(ApprovalMode::Yolo));
    assert_eq!(reloaded.sandbox_profile, Some(SandboxProfile::ReadOnly));
    assert_eq!(reloaded.revision, 2);
}

#[tokio::test(flavor = "current_thread")]
async fn always_decisions_survive_production_checker_reconstruction() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("permissions.json");
    let checker = PermissionChecker::new(None, Some(store_path.clone()));
    assert!(checker.is_persistent().await);
    let persisted = checker
        .always_allow("edit", Some("/tmp/work/file.txt"), Some("session-it"))
        .await;
    assert!(persisted);
    assert!(store_path.exists());
    // Reconstruct as production paths do (canonical path, not None).
    let rebuilt = PermissionChecker::new(None, Some(store_path));
    let result = rebuilt
        .check("edit", Some("/tmp/work/file.txt"), Some("session-it"))
        .await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test(flavor = "current_thread")]
async fn corrupt_permission_json_fails_conservatively() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("permissions.json");
    std::fs::write(&store_path, "{ not valid json").unwrap();
    let checker = PermissionChecker::new(None, Some(store_path));
    // Corrupt store is ignored (existing conservative behavior): mutating
    // default remains Ask, never auto-allow.
    let result = checker.check("edit", None, None).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn preference_cas_conflicts_instead_of_last_write_wins() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool).await.unwrap();
    let store = RuntimePreferenceStore::new(pool);
    let first = store
        .set_approval_mode("frontend-a", ApprovalMode::Interactive, None)
        .await
        .unwrap();
    // Second frontend with stale revision fails; it must reload.
    let err = store
        .set_approval_mode("frontend-a", ApprovalMode::Yolo, Some(first.revision + 9))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        codegg_core::approval::PreferenceError::Conflict { .. }
    ));
}

#[test]
fn pre_preference_db_opens_cleanly_with_defaults() {
    // Additive table: a fresh migrate has zero rows but valid reads.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        codegg_core::session::schema::migrate(&pool).await.unwrap();
        let store = RuntimePreferenceStore::new(pool);
        assert!(store.get("nobody").await.unwrap().is_none());
        let snapshot = ExecutionPolicySnapshot::default_snapshot();
        assert_eq!(snapshot.approval_mode(), ApprovalMode::Interactive);
    });
}

#[test]
fn protocol_snapshot_is_additive_for_older_clients() {
    // Older payloads without the new optional fields still decode.
    let legacy = serde_json::json!({
        "approval_mode": "interactive",
        "sandbox_profile": "workspace_write",
        "captured_at_ms": 0
    });
    let decoded: codegg_protocol::core::ExecutionPolicySnapshotDto =
        serde_json::from_value(legacy).unwrap();
    assert_eq!(
        decoded.approval_mode,
        codegg_protocol::core::ApprovalModeDto::Interactive
    );
    // New DTOs round-trip.
    let full = codegg_protocol::core::ExecutionPolicySnapshotDto {
        approval_mode: codegg_protocol::core::ApprovalModeDto::Yolo,
        sandbox_profile: codegg_protocol::core::SandboxProfileDto::WorkspaceWrite,
        principal_id: Some("local-owner".into()),
        session_id: Some("s".into()),
        agent_id: None,
        policy_revision: Some("config:1".into()),
        reviewer_config_id: None,
        captured_at_ms: 1,
        sandbox_enforcement: None,
    };
    let json = serde_json::to_value(&full).unwrap();
    let back: codegg_protocol::core::ExecutionPolicySnapshotDto =
        serde_json::from_value(json).unwrap();
    assert_eq!(back, full);
}

#[test]
fn permission_level_ask_maps_to_router_escalate() {
    // `Ask` is the natural deterministic Escalate result.
    assert_eq!(PermissionLevel::Ask.as_str(), "ask");
    let router = ApprovalRouter::new(snapshot_for(ApprovalMode::Yolo));
    let decision = router.route_escalation(&approval_request());
    assert!(decision.allowed());
}

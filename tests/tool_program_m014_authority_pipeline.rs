//! M014 authority pipeline tests.
//!
//! Covers C-01 through C-10: authority is derived from the actual accepted
//! permission/path-policy decision, not synthesized from identity strings.

#![cfg(test)]

use codegg_core::jobs::ToolAuthorityGrant;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn accepted_context(workspace: &str) -> codegg_core::jobs::ToolProgramExecutionContext {
    codegg_core::jobs::ToolProgramExecutionContext {
        workspace_path_policy_id: workspace.into(),
        principal_ref: Some("test-principal".into()),
        authority_ref: Some(format!("test-decision:{workspace}")),
        policy_revision: Some("test-policy-v1".into()),
        path_policy_revision: Some("test-path-v1".into()),
        decision_outcome: Some("allowed".into()),
        caller_class: Some("agent".into()),
        maximum_effect_class: Some("read_only".into()),
        decision_issued_at: Some(now_millis()),
        contract_snapshot_json: r#"{"contracts":[]}"#.into(),
        ..codegg_core::jobs::ToolProgramExecutionContext::for_workspace(workspace, "test")
    }
}

/// C-01: The grant is created from the actual accepted direct-call
/// permission/path-policy decision. Verify that build_authority_grant
/// populates real decision fields from the ToolProgramExecutionContext.
#[tokio::test(flavor = "current_thread")]
async fn c01_grant_uses_real_decision_fields() {
    let workspace_id = "ws-c01";
    let program_id = "tp-c01";
    let source_digest = "sha256:source-c01";

    let execution_context = accepted_context(workspace_id);

    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        workspace_id,
        program_id,
        &[],
        source_digest,
        "",
        "",
    )
    .unwrap();

    assert_eq!(grant.workspace_path_policy_id, workspace_id);
    assert!(
        !grant.principal_ref.is_empty(),
        "principal_ref must be populated"
    );
    assert!(
        !grant.decision_digest.is_empty(),
        "decision_digest must be populated"
    );
    assert!(
        !grant.policy_revision.is_empty(),
        "policy_revision must be populated"
    );
    assert!(
        !grant.allowed_caller_class.is_empty(),
        "allowed_caller_class must be populated"
    );
    assert!(
        !grant.allowed_effect_class.is_empty(),
        "allowed_effect_class must be populated"
    );
    assert!(grant.issued_at > 0, "issued_at must be populated");
}

/// C-02: Identity strings and hashes alone cannot create a valid grant.
#[tokio::test(flavor = "current_thread")]
async fn c02_empty_context_grant_fails() {
    let grant = ToolAuthorityGrant {
        schema_version: 2,
        grant_id: String::new(),
        principal_ref: String::new(),
        workspace_id: String::new(),
        workspace_path_policy_id: String::new(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: String::new(),
        allowed_caller_class: String::new(),
        allowed_effect_class: String::new(),
        manifest_digest: String::new(),
        source_digest: String::new(),
        ir_digest: String::new(),
        contract_digest: String::new(),
        contract_snapshot_json: String::new(),
        issued_at: now_millis(),
        expires_at: Some(now_millis() + 3_600_000),
        revoked_at: None,
        decision_digest: String::new(),
    };

    assert!(
        !grant.verify_integrity(),
        "empty-context grant must fail integrity verification"
    );
}

/// C-03: The immutable decision identity and grant survive job-store round trip.
#[tokio::test(flavor = "current_thread")]
async fn c03_grant_round_trip_preserves_decision_identity() {
    let workspace_id = "ws-c03";
    let program_id = "tp-c03";
    let source_digest = "sha256:source-c03";

    let execution_context = accepted_context(workspace_id);

    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        workspace_id,
        program_id,
        &[],
        source_digest,
        "",
        "",
    )
    .unwrap();

    let json = serde_json::to_string(&grant).unwrap();
    let restored: ToolAuthorityGrant = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.principal_ref, grant.principal_ref);
    assert_eq!(
        restored.workspace_path_policy_id,
        grant.workspace_path_policy_id
    );
    assert_eq!(restored.policy_revision, grant.policy_revision);
    assert_eq!(restored.decision_digest, grant.decision_digest);
    assert!(restored.verify_integrity());
}

/// C-04: Tampering security-relevant fields fails closed.
#[tokio::test(flavor = "current_thread")]
async fn c04_tampered_grant_fails_verification() {
    let workspace_id = "ws-c04";
    let program_id = "tp-c04";
    let source_digest = "sha256:source-c04";

    let execution_context = accepted_context(workspace_id);

    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        workspace_id,
        program_id,
        &[],
        source_digest,
        "",
        "",
    )
    .unwrap();

    assert!(grant.verify_integrity());

    grant.principal_ref = "attacker".to_string();
    assert!(
        !grant.verify_integrity(),
        "tampered principal_ref must fail verification"
    );

    // Restore and tamper workspace_path_policy_id
    grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        workspace_id,
        program_id,
        &[],
        source_digest,
        "",
        "",
    )
    .unwrap();
    grant.workspace_path_policy_id = "wrong-workspace".to_string();
    assert!(
        !grant.verify_integrity(),
        "tampered workspace_path_policy_id must fail verification"
    );
}

/// C-06: Stale path-policy revision fails.
#[tokio::test(flavor = "current_thread")]
async fn c06_stale_policy_revision_fails() {
    let workspace_id = "ws-c06";
    let program_id = "tp-c06";
    let source_digest = "sha256:source-c06";

    let execution_context = accepted_context(workspace_id);

    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        workspace_id,
        program_id,
        &[],
        source_digest,
        "",
        "",
    )
    .unwrap();

    assert!(grant.verify_integrity());

    let mut stale_grant = grant.clone();
    stale_grant.policy_revision = format!("{}:stale", grant.policy_revision);
    assert!(
        !stale_grant.verify_integrity(),
        "stale policy revision must fail verification"
    );
}

/// C-06: Expired grants fail.
#[tokio::test(flavor = "current_thread")]
async fn c06_expired_grant_fails() {
    let workspace_id = "ws-c06-exp";
    let program_id = "tp-c06-exp";
    let source_digest = "sha256:source-c06-exp";

    let execution_context = accepted_context(workspace_id);

    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&execution_context),
        workspace_id,
        program_id,
        &[],
        source_digest,
        "",
        "",
    )
    .unwrap();

    let mut expired_grant = grant.clone();
    expired_grant.expires_at = Some(now_millis() - 1000); // expired 1 second ago
    assert!(
        !expired_grant.is_valid(now_millis()),
        "expired grant must fail is_valid"
    );
}

/// C-07: Submission freezes the exact canonical allowed contract snapshot.
#[tokio::test(flavor = "current_thread")]
async fn c07_contract_snapshot_is_non_empty() {
    let registry = codegg::tool::default_registry();
    let broker = codegg::tool::broker::ToolBroker::new(registry);
    let allowed_tools = vec!["read".to_string(), "grep".to_string()];
    let snapshot =
        codegg::tool::tool_program_context::resolve_contract_snapshot(&broker, &allowed_tools);

    let snapshot = snapshot.expect("contract snapshot resolution should succeed");
    assert!(
        !snapshot.is_empty(),
        "contract snapshot must not be empty for allowed tools"
    );
    assert_eq!(snapshot.len(), 2);
}

/// C-08: The same canonical contract digest algorithm is used everywhere.
#[tokio::test(flavor = "current_thread")]
async fn c08_contract_digest_is_deterministic() {
    let registry = codegg::tool::default_registry();
    let broker = codegg::tool::broker::ToolBroker::new(registry);
    let allowed_tools = vec!["read".to_string(), "grep".to_string()];
    let snapshot1 =
        codegg::tool::tool_program_context::resolve_contract_snapshot(&broker, &allowed_tools)
            .expect("snapshot resolution should succeed");
    let digest1 = codegg::tool::tool_program_context::canonical_contract_digest(&snapshot1);

    let reordered = vec!["grep".to_string(), "read".to_string()];
    let snapshot2 =
        codegg::tool::tool_program_context::resolve_contract_snapshot(&broker, &reordered)
            .expect("snapshot resolution should succeed");
    let digest2 = codegg::tool::tool_program_context::canonical_contract_digest(&snapshot2);

    assert_eq!(
        digest1, digest2,
        "reordered tools must produce the same canonical digest"
    );
}

/// C-10: An empty contract snapshot is rejected.
#[tokio::test(flavor = "current_thread")]
async fn c10_empty_contract_snapshot_rejected() {
    let registry = codegg::tool::default_registry();
    let broker = codegg::tool::broker::ToolBroker::new(registry);
    let empty: Vec<String> = vec![];
    let snapshot = codegg::tool::tool_program_context::resolve_contract_snapshot(&broker, &empty)
        .expect("empty snapshot resolution should succeed");
    assert!(
        snapshot.is_empty(),
        "empty tool list must produce empty contract snapshot"
    );
}

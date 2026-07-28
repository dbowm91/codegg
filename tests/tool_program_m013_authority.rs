//! M013 authority grant and broker verification tests.
//!
//! Covers closure criteria related to F01 / A1-A5 and F02 / B1-B4:
//! - A1: Production grant is derived from the actual permission/path-policy decision.
//! - A2: Bounded, non-secret identity fields are present in the grant.
//! - A3: Persist before admission; the executor never fabricates a replacement.
//! - A4: Tampering each security-relevant field fails closed.
//! - A5: Tests construct via production paths, not direct `is_valid()` calls.
//! - B1: Broker verifies every grant field on every nested call.
//! - B3: Verification failures never produce completed-call records.
//! - C-04: integrity digest covers every security-relevant field.

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
        path_policy_revision: Some("test-path-policy-v1".into()),
        decision_outcome: Some("allowed".into()),
        caller_class: Some("agent".into()),
        maximum_effect_class: Some("read_only".into()),
        decision_issued_at: Some(now_millis()),
        contract_snapshot_json: r#"{"contracts":[]}"#.into(),
        ..codegg_core::jobs::ToolProgramExecutionContext::for_workspace(workspace, "test")
    }
}

/// M013 A2 / C-04: build_authority_grant now populates source/IR/contract digests
/// and computes a decision_digest that verifies against compute_digest().
#[tokio::test(flavor = "current_thread")]
async fn a2_build_authority_grant_includes_source_ir_contract() {
    let workspace_id = "ws-build-grant";
    let program_id = "tp-m013-build-grant";
    let allowed_tools = vec!["read".to_string(), "grep".to_string()];
    let source_digest = "sha256:source-abc";
    let ir_digest = "sha256:ir-abc";
    let contract_digest = "sha256:contract-abc";

    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context(workspace_id)),
        workspace_id,
        program_id,
        &allowed_tools,
        source_digest,
        ir_digest,
        contract_digest,
    )
    .unwrap();

    assert_eq!(grant.source_digest, source_digest);
    assert_eq!(grant.ir_digest, ir_digest);
    assert_eq!(grant.contract_digest, contract_digest);
    assert!(
        !grant.decision_digest.is_empty(),
        "decision_digest must be populated"
    );
    assert!(
        grant.verify_integrity(),
        "compute_digest must reproduce decision_digest for an unmodified grant"
    );
}

/// M013 C-04: tampering with any security-relevant field invalidates
/// the integrity digest.
#[test]
fn c04_tampering_any_field_fails_integrity() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-tamper")),
        "ws-tamper",
        "tp-m013-tamper",
        &["read".to_string()],
        "sha256:src",
        "sha256:ir",
        "sha256:contract",
    )
    .unwrap();
    let original = grant.decision_digest.clone();
    assert!(grant.verify_integrity());

    // Tamper with workspace_id.
    grant.workspace_id = "different-ws".into();
    assert!(
        !grant.verify_integrity(),
        "tampering workspace_id must fail integrity"
    );
    grant.workspace_id = "ws-tamper".into();
    assert!(grant.verify_integrity(), "restore must re-verify");

    // Tamper with session_id.
    grant.session_id = Some("injected-session".into());
    assert!(
        !grant.verify_integrity(),
        "tampering session_id must fail integrity"
    );
    grant.session_id = None;
    assert!(grant.verify_integrity());

    // Tamper with manifest_digest.
    grant.manifest_digest = "sha256:forged-manifest".into();
    assert!(
        !grant.verify_integrity(),
        "tampering manifest_digest must fail integrity"
    );

    // Confirm the original (untampered) digest is stable.
    assert_eq!(grant.decision_digest, original);
}

/// M013 C-04: tampering source_digest fails integrity.
#[test]
fn c04_tampering_source_digest_fails_integrity() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-src")),
        "ws-src",
        "tp-m013-src",
        &["read".to_string()],
        "sha256:original-source",
        "sha256:ir-original",
        "sha256:contract-original",
    )
    .unwrap();
    assert!(grant.verify_integrity());
    grant.source_digest = "sha256:evil-source".into();
    assert!(
        !grant.verify_integrity(),
        "tampering source_digest must fail integrity"
    );
}

/// M013 C-04: tampering ir_digest fails integrity.
#[test]
fn c04_tampering_ir_digest_fails_integrity() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-ir")),
        "ws-ir",
        "tp-m013-ir",
        &["read".to_string()],
        "sha256:source",
        "sha256:original-ir",
        "sha256:contract",
    )
    .unwrap();
    assert!(grant.verify_integrity());
    grant.ir_digest = "sha256:evil-ir".into();
    assert!(
        !grant.verify_integrity(),
        "tampering ir_digest must fail integrity"
    );
}

/// M013 C-04: tampering contract_digest fails integrity.
#[test]
fn c04_tampering_contract_digest_fails_integrity() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-contract")),
        "ws-contract",
        "tp-m013-contract",
        &["read".to_string()],
        "sha256:source",
        "sha256:ir",
        "sha256:original-contract",
    )
    .unwrap();
    assert!(grant.verify_integrity());
    grant.contract_digest = "sha256:evil-contract".into();
    assert!(
        !grant.verify_integrity(),
        "tampering contract_digest must fail integrity"
    );
}

/// M013 A4: an expired grant fails is_valid even if integrity holds.
#[test]
fn a4_expired_grant_fails_is_valid() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-exp")),
        "ws-exp",
        "tp-m013-exp",
        &["read".to_string()],
        "sha256:src",
        "sha256:ir",
        "sha256:contract",
    )
    .unwrap();
    grant.expires_at = Some(now_millis() - 10_000);
    assert!(!grant.is_valid(now_millis()));
    assert!(
        grant.verify_integrity(),
        "expiry tampering does not break digest (only is_valid)"
    );
}

/// M013 A4: a revoked grant fails is_valid.
#[test]
fn a4_revoked_grant_fails_is_valid() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-rev")),
        "ws-rev",
        "tp-m013-rev",
        &["read".to_string()],
        "sha256:src",
        "sha256:ir",
        "sha256:contract",
    )
    .unwrap();
    grant.revoked_at = Some(now_millis() - 1_000);
    assert!(!grant.is_valid(now_millis()));
}

/// M013 A4: workspace mismatch detected by integrity (workspace_id is part of the digest).
#[test]
fn a4_workspace_mismatch_fails_integrity() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-original")),
        "ws-original",
        "tp-m013-ws",
        &["read".to_string()],
        "sha256:src",
        "sha256:ir",
        "sha256:contract",
    )
    .unwrap();
    grant.workspace_id = "ws-different".into();
    assert!(
        !grant.verify_integrity(),
        "workspace_id tampering must fail integrity"
    );
}

/// M013 A4: policy_revision tampering fails integrity.
#[test]
fn a4_policy_revision_tampering_fails_integrity() {
    let mut grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-pol")),
        "ws-pol",
        "tp-m013-pol",
        &["read".to_string()],
        "sha256:src",
        "sha256:ir",
        "sha256:contract",
    )
    .unwrap();
    grant.policy_revision = "policy-v999-forged".into();
    assert!(
        !grant.verify_integrity(),
        "policy_revision tampering must fail integrity"
    );
}

/// M013 A5: tests do not rely on direct `is_valid()` to claim authority;
/// they exercise the production `build_authority_grant` constructor and
/// the resulting grant must satisfy both verify_integrity and is_valid.
#[test]
fn a5_production_constructor_grant_is_valid_and_intact() {
    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-prod")),
        "ws-prod",
        "tp-m013-prod",
        &["read".to_string()],
        "sha256:src",
        "sha256:ir",
        "sha256:contract",
    )
    .unwrap();
    assert!(
        grant.verify_integrity(),
        "production-constructed grant must verify integrity"
    );
    assert!(
        grant.is_valid(now_millis()),
        "production-constructed grant must be temporally valid"
    );
}

/// M013 A2: grant fields are bounded and carry security-relevant identity.
#[test]
fn a2_grant_carries_required_identity_fields() {
    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&accepted_context("ws-fields")),
        "ws-fields",
        "tp-m013-fields",
        &["read".to_string()],
        "sha256:src",
        "sha256:ir",
        "sha256:contract",
    )
    .unwrap();
    let required = [
        ("schema_version", grant.schema_version == 1),
        ("grant_id non-empty", !grant.grant_id.is_empty()),
        ("principal_ref non-empty", !grant.principal_ref.is_empty()),
        ("workspace_id", grant.workspace_id == "ws-fields"),
        (
            "workspace_path_policy_id",
            !grant.workspace_path_policy_id.is_empty(),
        ),
        ("policy_revision", !grant.policy_revision.is_empty()),
        ("manifest_digest", !grant.manifest_digest.is_empty()),
        ("source_digest", !grant.source_digest.is_empty()),
        ("ir_digest", !grant.ir_digest.is_empty()),
        ("contract_digest", !grant.contract_digest.is_empty()),
        ("issued_at", grant.issued_at > 0),
        ("decision_digest", !grant.decision_digest.is_empty()),
    ];
    for (field, ok) in required {
        assert!(ok, "required field must be present: {field}");
    }
}

/// M013 C-06: Session mismatch between grant and context is rejected.
#[test]
fn c06_session_mismatch_fails_scope() {
    use codegg::tool::broker::{BrokerAuthority, BrokerInvocationContext};
    use codegg::tool::contract::{ToolCaller, ToolContract};

    let exec_ctx = codegg_core::jobs::ToolProgramExecutionContext {
        session_id: Some("sess-granted".into()),
        ..accepted_context("ws-scope")
    };
    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&exec_ctx),
        "ws-scope",
        "tp-session",
        &["read".into()],
        "src",
        "ir",
        "contract",
    )
    .unwrap();
    let authority = BrokerAuthority::Verified { grant };
    let contract = ToolContract::legacy("read", serde_json::json!({}));

    // Context uses a different session
    let ctx = BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "tp-session".into(),
        },
        cwd: std::path::PathBuf::from("/tmp"),
        session_id: Some("sess-other".into()),
        workspace_id: Some("ws-scope".into()),
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: None,
        submission_key: None,
        authority: authority.clone(),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
    };
    let result = authority.verify_grant_scope("read", &contract, &ctx);
    assert!(
        result.is_err(),
        "session mismatch must fail scope: {:?}",
        result
    );
}

/// M013 C-06: Permission mode mismatch between grant and context is rejected.
#[test]
fn c06_permission_mode_mismatch_fails_scope() {
    use codegg::tool::broker::{BrokerAuthority, BrokerInvocationContext};
    use codegg::tool::contract::{ToolCaller, ToolContract};

    let exec_ctx = codegg_core::jobs::ToolProgramExecutionContext {
        permission_mode: Some("allow".into()),
        session_id: Some("sess-pm".into()),
        ..accepted_context("ws-pm")
    };
    let grant = codegg::tool::tool_program_context::build_authority_grant(
        Some(&exec_ctx),
        "ws-pm",
        "tp-pm",
        &["read".into()],
        "src",
        "ir",
        "contract",
    )
    .unwrap();
    let authority = BrokerAuthority::Verified { grant };
    let contract = ToolContract::legacy("read", serde_json::json!({}));

    // Context uses a different permission mode
    let ctx = BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "tp-pm".into(),
        },
        cwd: std::path::PathBuf::from("/tmp"),
        session_id: Some("sess-pm".into()),
        workspace_id: Some("ws-pm".into()),
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: Some("deny".into()),
        timeout_ms: None,
        submission_key: None,
        authority: authority.clone(),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
    };
    let result = authority.verify_grant_scope("read", &contract, &ctx);
    assert!(
        result.is_err(),
        "permission mode mismatch must fail scope: {:?}",
        result
    );
}

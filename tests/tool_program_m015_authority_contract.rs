//! M015 accepted-decision authority and frozen-contract convergence.

use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::tool_program_context::{
    build_authority_grant, canonical_contract_digest, resolve_contract_snapshot, to_core_context,
};
use codegg::tool::{ToolBroker, ToolRegistry};
use std::path::PathBuf;

fn accepted_context() -> ToolExecutionContext {
    let now = chrono::Utc::now().timestamp_millis();
    ToolExecutionContext {
        backend: ToolBackendKind::Native,
        session_id: Some("session-m015".into()),
        cwd: PathBuf::from("."),
        permission_mode: Some("allow".into()),
        timeout_ms: Some(30_000),
        invocation_key: Some("invocation-m015".into()),
        turn_id: Some("turn-m015".into()),
        agent_id: Some("agent-m015".into()),
        parent_job_id: None,
        parent_attempt_id: None,
        provider_name: None,
        backend_policy: Some("native_only".into()),
        cancellation: None,
        deadline: None,
        decision_id: Some("decision-m015".into()),
        decision_outcome: Some("allowed".into()),
        workspace_path_policy_id: Some("path-policy-m015".into()),
        workspace_path_policy_revision: Some("path-revision-m015".into()),
        permission_policy_revision: Some("permission-revision-m015".into()),
        principal_identity: Some("principal-m015".into()),
        caller_class: Some("agent".into()),
        max_effect_class: Some("read_only".into()),
        decision_issued_at: Some(now),
        decision_expires_at: Some(now + 60_000),
        decision_revoked_at: None,
        program_contract_snapshot: None,
    }
}

#[test]
fn accepted_decision_identity_is_the_grant_identity() {
    let direct = accepted_context();
    let core = to_core_context(Some(&direct), "workspace-m015", "program-m015")
        .expect("accepted decision must convert");
    let grant = build_authority_grant(
        Some(&core),
        "workspace-m015",
        "program-m015",
        &["read".into()],
        "sha256:source",
        "sha256:ir",
        "sha256:contracts",
    )
    .expect("accepted decision must produce a grant");

    assert_eq!(grant.grant_id, "decision-m015");
    assert_eq!(grant.principal_ref, "principal-m015");
    assert_eq!(grant.issued_at, direct.decision_issued_at.unwrap());
}

#[test]
fn identity_strings_without_an_accepted_decision_do_not_authorize() {
    let mut direct = accepted_context();
    direct.decision_id = None;
    assert!(to_core_context(Some(&direct), "workspace-m015", "program-m015").is_err());

    direct.decision_id = Some("decision-m015".into());
    direct.decision_outcome = Some("denied".into());
    assert!(to_core_context(Some(&direct), "workspace-m015", "program-m015").is_err());
}

#[test]
fn stale_expired_revoked_and_workspace_mismatched_decisions_fail_closed() {
    let mut direct = accepted_context();
    direct.decision_expires_at = Some(chrono::Utc::now().timestamp_millis() - 1);
    assert!(to_core_context(Some(&direct), "workspace-m015", "program-m015").is_err());

    let mut direct = accepted_context();
    direct.decision_revoked_at = Some(chrono::Utc::now().timestamp_millis());
    assert!(to_core_context(Some(&direct), "workspace-m015", "program-m015").is_err());

    let mut direct = accepted_context();
    direct.workspace_path_policy_id = Some(String::new());
    assert!(to_core_context(Some(&direct), "workspace-m015", "program-m015").is_err());
}

#[test]
fn one_canonical_snapshot_digest_is_order_independent() {
    let registry = ToolRegistry::with_defaults();
    let broker = ToolBroker::new(&registry);
    let first = resolve_contract_snapshot(&broker, &["read".into(), "grep".into()]).unwrap();
    let second = resolve_contract_snapshot(&broker, &["grep".into(), "read".into()]).unwrap();
    assert_eq!(
        canonical_contract_digest(&first).unwrap(),
        canonical_contract_digest(&second).unwrap()
    );
}

#[test]
fn unknown_runtime_contract_is_rejected_instead_of_becoming_empty_snapshot() {
    let registry = ToolRegistry::with_defaults();
    let broker = ToolBroker::new(&registry);
    assert!(resolve_contract_snapshot(&broker, &["not-a-runtime-tool".into()]).is_err());
}

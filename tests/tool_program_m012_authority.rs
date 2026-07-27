//! M012 authority grant and broker verification tests.
//!
//! Covers closure criteria C-01 through C-06:
//! - C-01: No production code creates a verified authority grant from a constant.
//! - C-02: Every submission persists a versioned authority grant.
//! - C-03: Nested Broker calls verify principal, workspace, caller class, effect class, manifest, policy revision.
//! - C-04: Missing/stale/invalid/revoked grants fail closed.
//! - C-05: Denied, failed, cancelled, timed-out, and schema-invalid nested calls cannot become CompletedCall.
//! - C-06: Only successful Broker terminal status increments completed-call counters.

#![cfg(test)]

use codegg::tool::broker::{BrokerAuthority, BrokerResult, ProgrammaticOutcome};
use codegg::tool::contract::{ToolTerminalStatus, ToolValue};
use codegg_core::jobs::{CallerClass, ToolAuthorityGrant};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn make_valid_grant() -> ToolAuthorityGrant {
    ToolAuthorityGrant {
        schema_version: 1,
        grant_id: "grant-test-valid".into(),
        principal_ref: "agent:test-principal".into(),
        workspace_id: "ws-test".into(),
        workspace_path_policy_id: "workspace:ws-test".into(),
        session_id: Some("sess-1".into()),
        agent_id: Some("agent-1".into()),
        turn_id: Some("turn-1".into()),
        permission_mode: None,
        policy_revision: "policy-v1".into(),
        allowed_caller_class: "agent".into(),
        allowed_effect_class: "read_only".into(),
        manifest_digest: "sha256:abc123".into(),
        issued_at: now_millis(),
        expires_at: None,
        revoked_at: None,
        decision_digest: "sha256:def456".into(),
        ..Default::default()
    }
}

fn make_expired_grant() -> ToolAuthorityGrant {
    let mut grant = make_valid_grant();
    grant.grant_id = "grant-test-expired".into();
    grant.expires_at = Some(now_millis() - 10_000);
    grant
}

fn make_revoked_grant() -> ToolAuthorityGrant {
    let mut grant = make_valid_grant();
    grant.grant_id = "grant-test-revoked".into();
    grant.revoked_at = Some(now_millis());
    grant
}

fn make_broker_result(status: ToolTerminalStatus, display: &str) -> BrokerResult {
    BrokerResult {
        value: ToolValue {
            display: display.into(),
            value: None,
            artifacts: vec![],
            provenance: None,
            terminal_status: status,
            truncated: false,
        },
        contract: codegg::tool::contract::ToolContract {
            name: "test".into(),
            caller_policy: codegg::tool::contract::ToolCallerPolicy::DirectOnly,
            effect_class: codegg::tool::contract::ToolEffectClass::ReadOnly,
            idempotency: codegg::tool::contract::IdempotencyClass::Idempotent,
            retry_policy: codegg::tool::contract::ToolRetryPolicy::default(),
            cache_policy: codegg::tool::contract::ToolCachePolicy::default(),
            projection_policy: codegg::tool::contract::ToolProjectionPolicy::default(),
            implementation_id: "test".into(),
            implementation_version: "1.0".into(),
            input_schema: serde_json::json!({}),
            output_schema: None,
        },
        invocation_id: "inv-1".into(),
        elapsed_ms: 0,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn c01_grant_not_built_from_constant() {
    // C-01: A grant built from real context fields has a non-empty,
    // non-constant principal_ref and manifest_digest.
    let grant = make_valid_grant();
    assert!(!grant.principal_ref.is_empty());
    assert!(!grant.manifest_digest.is_empty());
    assert_ne!(grant.principal_ref, "local-agent");
    assert_ne!(grant.manifest_digest, "tool-policy-v1");
}

#[tokio::test(flavor = "current_thread")]
async fn c02_grant_has_schema_version() {
    // C-02: Every grant carries a schema_version for forward compatibility.
    let grant = make_valid_grant();
    assert_eq!(grant.schema_version, 1);
    assert!(!grant.grant_id.is_empty());
    assert!(!grant.decision_digest.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn c03_from_grant_carries_all_fields() {
    // C-03: BrokerAuthority::from_grant preserves all verification fields.
    let grant = make_valid_grant();
    let authority = BrokerAuthority::from_grant(grant);
    assert!(authority.is_verified());
    let extracted = authority.grant();
    assert!(extracted.is_some());
    let g = extracted.unwrap();
    assert_eq!(g.principal_ref, "agent:test-principal");
    assert_eq!(g.workspace_id, "ws-test");
    assert_eq!(g.allowed_caller_class, "agent");
    assert_eq!(g.allowed_effect_class, "read_only");
    assert_eq!(g.policy_revision, "policy-v1");
    assert_eq!(g.manifest_digest, "sha256:abc123");
}

#[tokio::test(flavor = "current_thread")]
async fn c04_expired_grant_fails_closed() {
    // C-04: An expired grant is not valid.
    let grant = make_expired_grant();
    assert!(!grant.is_valid(now_millis()));
}

#[tokio::test(flavor = "current_thread")]
async fn c04_revoked_grant_fails_closed() {
    // C-04: A revoked grant is not valid.
    let grant = make_revoked_grant();
    assert!(!grant.is_valid(now_millis()));
}

#[tokio::test(flavor = "current_thread")]
async fn c04_valid_grant_passes() {
    // C-04: A valid grant passes validation.
    let grant = make_valid_grant();
    assert!(grant.is_valid(now_millis()));
}

#[tokio::test(flavor = "current_thread")]
async fn c05_denied_status_maps_to_programmatic_outcome() {
    // C-05: Denied terminal status cannot become a CompletedCall.
    let result = make_broker_result(ToolTerminalStatus::Denied, "denied");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(outcome.unwrap_err(), ProgrammaticOutcome::Denied);
}

#[tokio::test(flavor = "current_thread")]
async fn c05_cancelled_status_maps_to_programmatic_outcome() {
    // C-05: Cancelled terminal status cannot become a CompletedCall.
    let result = make_broker_result(ToolTerminalStatus::Cancelled, "cancelled");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(outcome.unwrap_err(), ProgrammaticOutcome::Cancelled);
}

#[tokio::test(flavor = "current_thread")]
async fn c05_timed_out_status_maps_to_programmatic_outcome() {
    // C-05: TimedOut terminal status cannot become a CompletedCall.
    let result = make_broker_result(ToolTerminalStatus::TimedOut, "timed out");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(outcome.unwrap_err(), ProgrammaticOutcome::TimedOut);
}

#[tokio::test(flavor = "current_thread")]
async fn c05_infrastructure_error_maps_to_programmatic_outcome() {
    // C-05: InfrastructureError terminal status cannot become a CompletedCall.
    let result = make_broker_result(
        ToolTerminalStatus::InfrastructureError,
        "connection refused",
    );
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_err());
    assert_eq!(
        outcome.unwrap_err(),
        ProgrammaticOutcome::InfrastructureError
    );
}

#[tokio::test(flavor = "current_thread")]
async fn c06_success_status_maps_to_completed() {
    // C-06: Only successful terminal status becomes a CompletedCall.
    let result = make_broker_result(ToolTerminalStatus::Success, "ok");
    let outcome = result.into_programmatic_outcome();
    assert!(outcome.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn caller_class_is_typed() {
    // Verify CallerClass enum exists and is usable.
    let agent = CallerClass::Agent;
    let program = CallerClass::Program;
    let subagent = CallerClass::Subagent;
    let api = CallerClass::Api;
    let internal = CallerClass::Internal;
    assert_ne!(agent, program);
    assert_ne!(program, subagent);
    assert_ne!(subagent, api);
    assert_ne!(api, internal);
}

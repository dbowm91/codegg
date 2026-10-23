//! C002 (M005): the delegated Tool Program child job inherits the accepted
//! execution ceiling.
//!
//! Before C002 the delegated child payload hard-coded
//! `network_intent: LegacyUnrestricted` and `sandbox_profile: None`, so a
//! principal that had accepted offline execution still got a delegated child
//! job with host network. These tests pin the corrected behaviour:
//!
//!   accepted decision
//!     -> ToolProgramExecutionContext (durable carrier)
//!       -> durable round-trip (JSON)
//!         -> delegated child job payload
//!
//! The network intent and the filesystem profile are **independent** axes.
//! Neither one implies the other, and the child may narrow the parent's
//! ceiling but never widen it.

use codegg_core::approval::{ApprovalMode, ExecutionPolicySnapshot, NetworkIntent, SandboxProfile};
use codegg_core::jobs::JobPayload;
use codegg_core::tool_program::child_job::{
    BuildJobConfig, ChildJobConfig, FormatJobConfig, LintJobConfig, TestJobConfig,
};

use codegg::permission::{PermissionDecisionReceipt, PermissionLevel};
use codegg::scheduler::tool_program_executor::child_job_payload;
use codegg::tool::authorization::ExecutionAuthorizationContext;
use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::tool_program_context::to_core_context;

const REVISION: &str = "policy-r1";

fn accepted_authorization(
    profile: SandboxProfile,
    intent: NetworkIntent,
) -> ExecutionAuthorizationContext {
    let revision = Some(REVISION.to_owned());
    ExecutionAuthorizationContext {
        snapshot: ExecutionPolicySnapshot::capture(
            ApprovalMode::Interactive,
            profile,
            Some("principal-1".into()),
            Some("session-1".into()),
            None,
            revision.clone(),
            None,
        )
        .with_network_intent(intent)
        .expect("the fixture only narrows the captured intent, never widens it"),
        receipt: PermissionDecisionReceipt {
            decision_id: "decision-1".into(),
            outcome: PermissionLevel::Allow,
            source: "fixture".into(),
            issued_at: 1,
            policy_revision: revision,
        },
        command_preflight: None,
        scratch_scope: None,
    }
}

/// A tool execution context that carries a complete accepted decision.
fn accepted_tool_context(
    profile: SandboxProfile,
    intent: NetworkIntent,
    with_authorization: bool,
) -> ToolExecutionContext {
    let mut context = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    context.decision_id = Some("decision-1".into());
    context.decision_outcome = Some("allowed".into());
    context.principal_identity = Some("principal-1".into());
    context.workspace_path_policy_id = Some("workspace-path-policy-1".into());
    context.workspace_path_policy_revision = Some("path-rev-1".into());
    context.permission_policy_revision = Some(REVISION.to_owned());
    context.permission_mode = Some("interactive".into());
    context.caller_class = Some("model".into());
    context.max_effect_class = Some("ToolProgram".into());
    context.decision_issued_at = Some(chrono::Utc::now().timestamp_millis());
    context.invocation_key = Some("invocation-1".into());
    context.sandbox_profile = Some(profile.as_str().to_owned());
    if with_authorization {
        context.execution_authorization = Some(accepted_authorization(profile, intent));
    }
    context
}

fn test_config() -> ChildJobConfig {
    ChildJobConfig::Test(TestJobConfig::default())
}

/// The delegated child payload's policy, read back out of the durable payload.
fn child_policy(payload: &JobPayload) -> (Option<SandboxProfile>, NetworkIntent) {
    match payload {
        JobPayload::Test {
            sandbox_profile,
            network_intent,
            ..
        }
        | JobPayload::ManagedArgv {
            sandbox_profile,
            network_intent,
            ..
        } => (*sandbox_profile, *network_intent),
        other => panic!("unexpected delegated child payload: {other:?}"),
    }
}

/// The core defect: a parent that accepted offline execution must not hand a
/// delegated child job host network.
#[test]
fn delegated_child_job_inherits_the_parents_offline_intent() {
    let context = accepted_tool_context(
        SandboxProfile::WorkspaceWrite,
        NetworkIntent::OfflineRequired,
        true,
    );
    let core = to_core_context(Some(&context), "workspace-1", "program-1")
        .expect("the accepted decision must produce a core context");

    assert_eq!(
        core.network_intent,
        NetworkIntent::OfflineRequired,
        "the accepted network intent must be captured into the durable context"
    );
    assert_eq!(
        core.sandbox_profile,
        Some(SandboxProfile::WorkspaceWrite),
        "the accepted filesystem profile must be captured into the durable context"
    );

    // The durable round-trip: the ceiling travels with the job, so a restart
    // or a replay cannot widen it.
    let encoded = serde_json::to_string(&core).expect("the core context must serialize");
    let decoded: codegg_core::jobs::ToolProgramExecutionContext =
        serde_json::from_str(&encoded).expect("the core context must deserialize");
    assert_eq!(decoded.network_intent, NetworkIntent::OfflineRequired);
    assert_eq!(
        decoded.sandbox_profile,
        Some(SandboxProfile::WorkspaceWrite)
    );

    let (_, payload, _) = child_job_payload(
        &test_config(),
        &Some("/tmp/workspace".to_string()),
        decoded.sandbox_profile,
        decoded.network_intent,
    );
    let (profile, intent) = child_policy(&payload);
    assert_eq!(
        intent,
        NetworkIntent::OfflineRequired,
        "the delegated child job must inherit the parent's offline intent"
    );
    assert_eq!(
        profile,
        Some(SandboxProfile::WorkspaceWrite),
        "the delegated child job must inherit the parent's filesystem ceiling"
    );
}

/// Every delegated child operation carries the ceiling, not just `test`.
#[test]
fn every_delegated_child_operation_carries_the_ceiling() {
    let configs = vec![
        ChildJobConfig::Test(TestJobConfig::default()),
        ChildJobConfig::Build(BuildJobConfig::default()),
        ChildJobConfig::Lint(LintJobConfig::default()),
        ChildJobConfig::Format(FormatJobConfig::default()),
    ];
    for config in configs {
        let (_, payload, _) = child_job_payload(
            &config,
            &Some("/tmp/workspace".to_string()),
            Some(SandboxProfile::WorkspaceWrite),
            NetworkIntent::OfflineRequired,
        );
        let (profile, intent) = child_policy(&payload);
        assert_eq!(
            intent,
            NetworkIntent::OfflineRequired,
            "every delegated child operation must inherit the offline intent"
        );
        assert_eq!(
            profile,
            Some(SandboxProfile::WorkspaceWrite),
            "every delegated child operation must inherit the filesystem ceiling"
        );
    }
}

/// The axes are independent: an offline intent is not satisfied by a narrower
/// filesystem profile, and a narrower profile never implies an offline intent.
#[test]
fn the_two_axes_are_independent() {
    // Offline intent with a FullHost profile: the owner refuses this
    // combination rather than running it uncontained, so the child must still
    // carry the offline intent.
    let (_, payload, _) = child_job_payload(
        &test_config(),
        &Some("/tmp/workspace".to_string()),
        Some(SandboxProfile::FullHost),
        NetworkIntent::OfflineRequired,
    );
    let (profile, intent) = child_policy(&payload);
    assert_eq!(intent, NetworkIntent::OfflineRequired);
    assert_eq!(profile, Some(SandboxProfile::FullHost));

    // A constrained profile with no offline intent stays unconstrained on the
    // network: the profile must not be read as an offline request.
    let (_, payload, _) = child_job_payload(
        &test_config(),
        &Some("/tmp/workspace".to_string()),
        Some(SandboxProfile::ReadOnly),
        NetworkIntent::LegacyUnrestricted,
    );
    let (profile, intent) = child_policy(&payload);
    assert_eq!(intent, NetworkIntent::LegacyUnrestricted);
    assert_eq!(profile, Some(SandboxProfile::ReadOnly));
}

/// A caller with no accepted authorization object keeps the historical
/// meaning: no ceiling, unrestricted network. The absence of a decision must
/// not be invented into an offline one, and the legacy payload must still
/// decode.
#[test]
fn a_missing_authorization_keeps_the_historical_meaning() {
    let context = accepted_tool_context(
        SandboxProfile::WorkspaceWrite,
        NetworkIntent::LegacyUnrestricted,
        false,
    );
    let core = to_core_context(Some(&context), "workspace-1", "program-1")
        .expect("the accepted decision must produce a core context");
    assert_eq!(core.network_intent, NetworkIntent::LegacyUnrestricted);
    assert_eq!(core.sandbox_profile, Some(SandboxProfile::WorkspaceWrite));
}

/// A payload persisted before M005 has neither field. It must still decode,
/// with the historical meaning preserved.
#[test]
fn a_pre_m005_payload_still_decodes() {
    let legacy = serde_json::json!({
        "schema_version": 1,
        "workspace_path_policy_id": "workspace-path-policy-1",
        "session_id": "session-1",
        "principal_ref": "principal-1",
        "authority_ref": "decision-1",
        "permission_mode": "interactive",
        "policy_revision": "policy-r1",
        "path_policy_revision": "path-rev-1",
        "decision_outcome": "allowed",
        "caller_class": "model",
        "maximum_effect_class": "ToolProgram",
        "decision_issued_at": 1,
        "contract_snapshot_json": "",
        "backend_policy": "native_only",
        "correlation_id": "correlation-1",
    });
    let decoded: codegg_core::jobs::ToolProgramExecutionContext =
        serde_json::from_value(legacy).expect("a pre-M005 payload must still decode");
    assert_eq!(
        decoded.network_intent,
        NetworkIntent::LegacyUnrestricted,
        "a pre-M005 payload keeps the historical unrestricted meaning"
    );
    assert_eq!(
        decoded.sandbox_profile, None,
        "a pre-M005 payload carries no filesystem ceiling"
    );

    let (_, payload, _) = child_job_payload(
        &test_config(),
        &Some("/tmp/workspace".to_string()),
        decoded.sandbox_profile,
        decoded.network_intent,
    );
    let (profile, intent) = child_policy(&payload);
    assert_eq!(intent, NetworkIntent::LegacyUnrestricted);
    assert_eq!(profile, None);
}

/// An authorization object that is present but invalid is an integrity
/// failure, not a missing decision: it must be refused rather than silently
/// downgraded to the legacy meaning.
#[test]
fn an_invalid_authorization_is_refused() {
    let mut context = accepted_tool_context(
        SandboxProfile::WorkspaceWrite,
        NetworkIntent::OfflineRequired,
        true,
    );
    let authorization = context.execution_authorization.as_mut().expect("fixture");
    // A receipt whose policy revision does not match the snapshot.
    authorization.receipt.policy_revision = Some("policy-r-other".into());

    let error = to_core_context(Some(&context), "workspace-1", "program-1")
        .expect_err("an invalid authorization must be refused");
    assert!(
        error.contains("not usable"),
        "the refusal must name the invalid authorization: {error}"
    );
}

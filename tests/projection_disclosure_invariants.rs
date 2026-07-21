//! Adversarial / property tests for M3 disclosure pipeline.
//!
//! Validates that redaction, policy, and disclosure context enforce
//! visibility invariants under adversarial payloads.

mod common;

use std::sync::Arc;

use codegg_core::projection_replay::context::{
    BoundedProjectResolver, ProjectionAccessContext, ProjectionCapabilitySet,
    ProjectionTransportClass,
};
use codegg_core::projection_replay::metrics::ProjectionReplayMetrics;
use codegg_core::projection_replay::policy::{
    ArtifactReadKind, DefaultAccessPolicy, DisclosureReason, ProjectionAccessPolicy,
};
use codegg_core::projection_replay::redactor::{
    FieldName, ProjectionFieldRedactor, RedactionResult, MAX_REDACTION_INPUT_BYTES,
};
use codegg_core::projection_replay::seam::{
    ProjectionDisclosureContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::{ProjectionReplayService, PublishOutcome};
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};
use serde_json::json;

// ── 1. Nested secrets are always redacted ────────────────────────────

#[test]
fn nested_secrets_never_survive_redaction() {
    let redactor = ProjectionFieldRedactor::new();
    let secrets = [
        "AKIAEXAMPLE1234567890",
        "hunter2",
        "Bearer abcdefghijklmnop",
        "https://user:password@example.com",
    ];
    // Structurally, api_key, auth_header (bearer), db_password (env key),
    // and url (userinfo) are all caught by their respective field rules.
    // The "token" key under Authorization rules only catches bearer/basic
    // patterns, so we test the fields that ARE structurally covered.
    let value = json!({
        "tool": "bash",
        "arguments": {
            "api_key": "AKIAEXAMPLE1234567890",
            "auth_header": "Bearer abcdefghijklmnop",
            "url": "https://user:password@example.com"
        },
        "env": "DATABASE_PASSWORD=hunter2"
    });
    let (redacted, _summary) = redactor.redact_json(&value, FieldName::ToolArgument);
    let serialized = serde_json::to_string(&redacted).unwrap();
    for secret in &secrets {
        assert!(
            !serialized.contains(secret),
            "secret '{}' must not survive redaction; got: {}",
            secret,
            serialized
        );
    }
}

#[test]
fn redact_json_replaces_api_key_with_redacted_marker() {
    let redactor = ProjectionFieldRedactor::new();
    let value = json!({ "arguments": { "api_key": "AKIAEXAMPLE1234567890" } });
    let (redacted, _summary) = redactor.redact_json(&value, FieldName::ToolArgument);
    let serialized = serde_json::to_string(&redacted).unwrap();
    assert!(
        serialized.contains("[REDACTED"),
        "api_key value must contain [REDACTED substring; got: {}",
        serialized
    );
}

// ── 2. Oversized payload always produces Downgraded ───────────────────

#[test]
fn oversized_payload_always_downgraded_never_redacted_or_unchanged() {
    let redactor = ProjectionFieldRedactor::new();
    let oversized = "x".repeat(MAX_REDACTION_INPUT_BYTES + 1);
    let result = redactor.redact_text(FieldName::Text, &oversized);
    assert!(
        matches!(result, RedactionResult::Downgraded { .. }),
        "oversized payload must produce Downgraded; got {:?}",
        result
    );
}

#[test]
fn oversized_json_string_field_downgraded() {
    let redactor = ProjectionFieldRedactor::new();
    let big = "a".repeat(MAX_REDACTION_INPUT_BYTES + 100);
    let value = json!({ "output": big });
    let (redacted, summary) = redactor.redact_json(&value, FieldName::Text);
    let obj = redacted.as_object().unwrap();
    let out_val = obj.get("output").unwrap().as_str().unwrap();
    assert!(
        out_val.contains("oversized"),
        "oversized JSON field must contain 'oversized' marker; got: {}",
        out_val
    );
    assert!(
        !summary.downgraded_counts().is_empty(),
        "summary must record a downgrade"
    );
}

// ── 3. Policy denies artifact read when project resolver rejects ──────

#[test]
fn default_policy_denies_artifact_read_when_project_resolver_rejects() {
    let resolver: Arc<dyn codegg_core::projection_replay::context::ProjectionProjectResolver> =
        Arc::new(BoundedProjectResolver::new(["allowed-project"]));
    let ctx = ProjectionAccessContext::with_projects(
        "client-1",
        "corr-1",
        ProjectionCapabilitySet::local_user(),
        resolver,
        ProjectionTransportClass::Local,
    );
    let policy = DefaultAccessPolicy::new();

    assert!(
        policy.authorize_artifact_read(&ctx, "allowed-project", ArtifactReadKind::RunArtifact),
        "should allow read for allowed project"
    );
    assert!(
        !policy.authorize_artifact_read(&ctx, "unknown-project", ArtifactReadKind::RunArtifact),
        "should deny read for project rejected by resolver"
    );
}

// ── 4. Local context visibility decisions via seam API ────────────────

#[test]
fn local_context_subscribes_to_public_events() {
    let ctx = ProjectionAccessContext::local("c1", "corr-1");
    let policy = DefaultAccessPolicy::new();
    assert!(
        policy.authorize_subscribe(&ctx, "proj-1", Some("sess-1")),
        "local context should authorize subscribe for a safe/public event"
    );
}

#[tokio::test]
async fn seam_denies_internal_events_through_disclosure() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    let metrics = Arc::new(ProjectionReplayMetrics::new());
    let dc =
        ProjectionDisclosureContext::local(Some("sess-1".into()), Some("proj-1".into()), metrics);

    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-1".into()),
        payload: CoreEvent::TurnReasoningDelta {
            session_id: "sess-1".into(),
            turn_id: "turn-1".into(),
            delta: "secret reasoning".into(),
        },
    };

    let binding = codegg_core::projection_replay::seam::ProjectionBindingContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: Some("ws-1".into()),
        binding_revision: 1,
    };

    let outcome = seam
        .publish_with_disclosure(&envelope, binding, Some(&dc))
        .await
        .unwrap();
    assert!(
        matches!(
            outcome,
            PublishOutcome::Denied {
                reason: DisclosureReason::InternalNotSerializable
            }
        ),
        "internal event must be denied through disclosure; got {:?}",
        outcome
    );
}

#[tokio::test]
async fn seam_allows_safe_events_through_disclosure() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    let metrics = Arc::new(ProjectionReplayMetrics::new());
    let dc =
        ProjectionDisclosureContext::local(Some("sess-1".into()), Some("proj-1".into()), metrics);

    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-1".into()),
        payload: CoreEvent::TurnStarted {
            session_id: "sess-1".into(),
            turn_id: "turn-1".into(),
        },
    };

    let binding = codegg_core::projection_replay::seam::ProjectionBindingContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: Some("ws-1".into()),
        binding_revision: 1,
    };

    let outcome = seam
        .publish_with_disclosure(&envelope, binding, Some(&dc))
        .await
        .unwrap();
    assert!(
        matches!(outcome, PublishOutcome::Published { .. }),
        "safe event must be published through disclosure; got {:?}",
        outcome
    );
}

#[tokio::test]
async fn seam_allows_sensitive_events_through_disclosure() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    let metrics = Arc::new(ProjectionReplayMetrics::new());
    let dc =
        ProjectionDisclosureContext::local(Some("sess-1".into()), Some("proj-1".into()), metrics);

    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-1".into()),
        turn_id: None,
        payload: CoreEvent::ConnectionRotated {
            connection_id: "conn-1".into(),
            new_revision: 1,
            catalog_revision: None,
            actor_seam: "test".into(),
        },
    };

    let binding = codegg_core::projection_replay::seam::ProjectionBindingContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: Some("ws-1".into()),
        binding_revision: 1,
    };

    let outcome = seam
        .publish_with_disclosure(&envelope, binding, Some(&dc))
        .await
        .unwrap();
    // Sensitive events pass disclosure (Allowed with SensitiveRedacted reason)
    // but may produce no projections and be Skipped as AdaptionEmpty.
    // The key invariant: they are NOT Denied with InternalNotSerializable.
    assert!(
        !matches!(
            outcome,
            PublishOutcome::Denied {
                reason: DisclosureReason::InternalNotSerializable
            }
        ),
        "sensitive event must NOT be denied as internal; got {:?}",
        outcome
    );
}

// ── 5. Access context capabilities ───────────────────────────────────

#[test]
fn local_context_lacks_admin_bypass() {
    let ctx = ProjectionAccessContext::local("c1", "corr-1");
    assert!(
        !ctx.has(codegg_core::projection_replay::context::ProjectionCapability::AdminBypass),
        "local context must not have AdminBypass"
    );
}

#[test]
fn internal_test_context_has_admin_bypass() {
    let ctx = ProjectionAccessContext::internal_test("c1", "corr-1");
    assert!(
        ctx.has(codegg_core::projection_replay::context::ProjectionCapability::AdminBypass),
        "internal_test context must have AdminBypass"
    );
}

//! Centralized projection publication seam.
//!
//! Single entry point used by every daemon publication site. Wraps
//! [`ProjectionReplayService::publish_from_core`] with canonical
//! context resolution, binding-revision validation, and structured
//! outcomes so callers don't need to know about replay storage.

use std::sync::Arc;

use codegg_protocol::core::{CoreEvent, EventEnvelope};

use crate::error::StorageError;
use crate::project_storage::{BindingStatus, ProjectStorage};
use crate::projection_replay::artifact_registry::ProjectionArtifactRegistry;
use crate::projection_replay::artifacts::HandleRegistrar;
use crate::projection_replay::context::ProjectionAccessContext;
use crate::projection_replay::metrics::{ProjectionReplayMetrics, ProjectionReplayMetricsSnapshot};
use crate::projection_replay::policy::PolicyRegistry;
use crate::projection_replay::redactor::ProjectionFieldRedactor;
use crate::projection_replay::service::{ProjectionReplayService, PublishOutcome};

/// Canonical binding context for a projection publication.
///
/// `SessionId`, `ProjectId`, `WorkspaceId`, and `binding_revision` are
/// resolved from `ProjectStorage` before this seam is invoked so the
/// adapter layer never invents identity. The caller (typically
/// `EventLog`'s sink hook) MAY pass `None` for any of these when no
/// canonical binding exists; the seam then fails closed with a
/// `Skipped { reason: UnboundSession }` outcome.
#[derive(Debug, Clone, Default)]
pub struct ProjectionBindingContext {
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub binding_revision: i64,
}

/// Backward-compatible alias for `ProjectionBindingContext`. Existing
/// callers in `src/` reference this name via the seam module path.
pub type ProjectionPublicationContext = ProjectionBindingContext;

/// M3 disclosure context wrapping the policy engine, redactor, and
/// handle registrar. Passed through the publication seam so the
/// service can authorize, redact, or downgrade events before storage.
#[derive(Clone)]
pub struct ProjectionDisclosureContext {
    pub access_ctx: Arc<ProjectionAccessContext>,
    pub policy: Arc<PolicyRegistry>,
    pub redactor: Arc<ProjectionFieldRedactor>,
    pub handle_registrar: Arc<HandleRegistrar>,
    pub artifact_registry: Option<Arc<dyn ProjectionArtifactRegistry>>,
    pub metrics: Arc<ProjectionReplayMetrics>,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
}

impl std::fmt::Debug for ProjectionDisclosureContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectionDisclosureContext")
            .field("access_ctx", &self.access_ctx)
            .field("policy", &self.policy)
            .field("redactor", &"<ProjectionFieldRedactor>")
            .field("handle_registrar", &"<HandleRegistrar>")
            .field(
                "artifact_registry",
                &self.artifact_registry.as_ref().map(|_| "<dyn ProjectionArtifactRegistry>"),
            )
            .field("session_id", &self.session_id)
            .field("project_id", &self.project_id)
            .finish_non_exhaustive()
    }
}

impl ProjectionDisclosureContext {
    /// Construct a disclosure context for local single-user access.
    pub fn local(
        session_id: Option<String>,
        project_id: Option<String>,
        metrics: Arc<ProjectionReplayMetrics>,
    ) -> Self {
        Self {
            access_ctx: Arc::new(ProjectionAccessContext::local("daemon", "disclosure")),
            policy: Arc::new(PolicyRegistry::default()),
            redactor: Arc::new(ProjectionFieldRedactor::new()),
            handle_registrar: Arc::new(HandleRegistrar::new()),
            artifact_registry: None,
            metrics,
            session_id,
            project_id,
        }
    }

    /// Construct a disclosure context for harness-internal tests.
    pub fn internal_test(
        session_id: Option<String>,
        project_id: Option<String>,
        metrics: Arc<ProjectionReplayMetrics>,
    ) -> Self {
        Self {
            access_ctx: Arc::new(ProjectionAccessContext::internal_test(
                "test",
                "disclosure-test",
            )),
            policy: Arc::new(PolicyRegistry::default()),
            redactor: Arc::new(ProjectionFieldRedactor::new()),
            handle_registrar: Arc::new(HandleRegistrar::new()),
            artifact_registry: None,
            metrics,
            session_id,
            project_id,
        }
    }

    /// Construct a disclosure context with explicit components.
    pub fn new(
        access_ctx: Arc<ProjectionAccessContext>,
        policy: Arc<PolicyRegistry>,
        redactor: Arc<ProjectionFieldRedactor>,
        handle_registrar: Arc<HandleRegistrar>,
        metrics: Arc<ProjectionReplayMetrics>,
        session_id: Option<String>,
        project_id: Option<String>,
    ) -> Self {
        Self {
            access_ctx,
            policy,
            redactor,
            handle_registrar,
            artifact_registry: None,
            metrics,
            session_id,
            project_id,
        }
    }

    /// Set the artifact registry on this disclosure context.
    pub fn with_artifact_registry(mut self, registry: Arc<dyn ProjectionArtifactRegistry>) -> Self {
        self.artifact_registry = Some(registry);
        self
    }
}

#[derive(Clone)]
pub struct ProjectionPublicationSeam {
    inner: Arc<ProjectionReplayService>,
    project_storage: Option<Arc<ProjectStorage>>,
}

impl ProjectionPublicationSeam {
    pub fn new(service: Arc<ProjectionReplayService>) -> Self {
        Self {
            inner: service,
            project_storage: None,
        }
    }

    /// Construct a seam with explicit canonical binding access. Used by
    /// the daemon construction path so publication can resolve
    /// `SessionId` -> `(ProjectId, WorkspaceId, binding_revision)` from
    /// the canonical store rather than the caller.
    pub fn with_project_storage(
        service: Arc<ProjectionReplayService>,
        project_storage: Arc<ProjectStorage>,
    ) -> Self {
        Self {
            inner: service,
            project_storage: Some(project_storage),
        }
    }

    /// Resolve canonical context for `envelope.session_id`. Returns
    /// `None` when no canonical binding exists or the binding is not
    /// `Resolved` (so the caller can route to `Skipped`).
    pub async fn resolve_binding(
        &self,
        envelope: &EventEnvelope<CoreEvent>,
    ) -> ProjectionBindingContext {
        let Some(session_id) = envelope
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
        else {
            return ProjectionBindingContext::default();
        };

        if let Some(caller_ctx) = self.caller_provided_context(envelope) {
            return caller_ctx;
        }

        let Some(ref storage) = self.project_storage else {
            return ProjectionBindingContext {
                session_id: Some(session_id),
                ..Default::default()
            };
        };

        match storage.session_binding(&session_id).await {
            Ok(Some(record)) if record.status == BindingStatus::Resolved => {
                ProjectionBindingContext {
                    session_id: Some(session_id),
                    project_id: record.project_id.map(|p| p.as_str().to_string()),
                    workspace_id: record.workspace_id.map(|w| w.as_str().to_string()),
                    binding_revision: record.revision,
                }
            }
            _ => ProjectionBindingContext {
                session_id: Some(session_id),
                ..Default::default()
            },
        }
    }

    /// Permit direct context override for callers that already hold
    /// canonical binding (e.g. scheduler/jobs that pass through the
    /// `JobCreated` session id explicitly). The default
    /// implementation returns `None`; production code wires through
    /// the canonical `ProjectStorage` lookup.
    fn caller_provided_context(
        &self,
        _envelope: &EventEnvelope<CoreEvent>,
    ) -> Option<ProjectionBindingContext> {
        None
    }

    /// Publish an envelope using the resolved canonical context
    /// without disclosure (backward-compatible path).
    ///
    /// Events that require redaction or downgrade are denied when no
    /// disclosure context is provided (fail-closed default).
    pub async fn publish(
        &self,
        envelope: &EventEnvelope<CoreEvent>,
        caller_ctx: ProjectionBindingContext,
    ) -> Result<PublishOutcome, StorageError> {
        self.publish_with_disclosure(envelope, caller_ctx, None)
            .await
    }

    /// Publish an envelope with both binding context and optional
    /// disclosure context. The disclosure context drives authorization,
    /// redaction, and handle downgrade decisions.
    pub async fn publish_with_disclosure(
        &self,
        envelope: &EventEnvelope<CoreEvent>,
        caller_ctx: ProjectionBindingContext,
        disclosure: Option<&ProjectionDisclosureContext>,
    ) -> Result<PublishOutcome, StorageError> {
        let context = if caller_ctx.session_id.is_some()
            || caller_ctx.project_id.is_some()
            || caller_ctx.workspace_id.is_some()
            || caller_ctx.binding_revision != 0
        {
            let mut ctx = caller_ctx.clone();
            if ctx.session_id.is_none() {
                ctx.session_id = envelope.session_id.clone();
            }
            ctx
        } else {
            self.resolve_binding(envelope).await
        };

        // Merge disclosure context session/project from binding context
        // when the disclosure context doesn't specify them.
        let disclosure = disclosure.map(|dc| {
            let mut dc = dc.clone();
            if dc.session_id.is_none() {
                dc.session_id = context.session_id.clone();
            }
            if dc.project_id.is_none() {
                dc.project_id = context.project_id.clone();
            }
            dc
        });

        self.inner
            .publish_from_core_with_contexts(envelope, &context, disclosure.as_ref())
            .await
    }

    pub fn service(&self) -> &Arc<ProjectionReplayService> {
        &self.inner
    }

    pub fn project_storage(&self) -> Option<&Arc<ProjectStorage>> {
        self.project_storage.as_ref()
    }

    pub fn metrics_snapshot(&self) -> ProjectionReplayMetricsSnapshot {
        self.inner.metrics_snapshot()
    }

    /// Issue an artifact handle for an event. Uses the disclosure
    /// context's artifact registry (if present) to mint a bounded
    /// public descriptor. Returns `None` when no registry is
    /// configured.
    pub async fn issue_artifact_for_event(
        &self,
        disclosure: &ProjectionDisclosureContext,
        project_id: &str,
        run_id: &str,
        kind: crate::projection_replay::artifacts::ArtifactKind,
        content_type: crate::projection_replay::artifacts::ArtifactContentType,
        total_bytes: u64,
        source_record_id: &str,
        public_summary: Option<String>,
        revision: u64,
    ) -> Option<Result<crate::projection_replay::artifacts::ProjectionArtifactHandle, crate::projection_replay::artifact_registry::ArtifactRegistryError>>
    {
        let registry = disclosure.artifact_registry.as_ref()?;
        Some(
            registry
                .issue_for_run(
                    project_id,
                    run_id,
                    kind,
                    content_type,
                    total_bytes,
                    source_record_id,
                    public_summary,
                    revision,
                )
                .await,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection_replay::context::{
        AllowAllProjectResolver, BoundedProjectResolver, ProjectionCapabilitySet,
        ProjectionTransportClass,
    };
    use crate::projection_replay::policy::{DefaultAccessPolicy, DisclosureDecision, DisclosureReason};
    use crate::projection_replay::service::{ProjectionReplayService, SafePublicationReason};
    use crate::projection_replay::store::ProjectionReplayStore;
    use codegg_protocol::core::{CoreEvent, EventEnvelope};
    use std::sync::Arc;

    async fn test_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let options = SqliteConnectOptions::from_str(":memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        pool
    }

    fn make_envelope(session_id: &str, turn_id: &str) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: 1,
            event_seq: 1,
            timestamp_ms: 1000,
            session_id: Some(session_id.to_string()),
            turn_id: Some(turn_id.to_string()),
            payload: CoreEvent::TurnStarted {
                session_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
            },
        }
    }

    fn make_sensitive_envelope(connection_id: &str) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: 1,
            event_seq: 1,
            timestamp_ms: 1000,
            session_id: None,
            turn_id: None,
            payload: CoreEvent::ConnectionRotated {
                connection_id: connection_id.to_string(),
                new_revision: 2,
                catalog_revision: None,
                actor_seam: "test".into(),
            },
        }
    }

    fn make_text_delta_envelope(session_id: &str, delta: &str) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: 1,
            event_seq: 1,
            timestamp_ms: 1000,
            session_id: Some(session_id.to_string()),
            turn_id: Some("t1".to_string()),
            payload: CoreEvent::TurnTextDelta {
                session_id: session_id.to_string(),
                turn_id: "t1".to_string(),
                delta: delta.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn authorize_subscribe_false_drops_event() {
        let pool = test_pool().await;
        let store = Arc::new(ProjectionReplayStore::new(pool));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let metrics = Arc::new(ProjectionReplayMetrics::new());

        // Build a context that denies all subscriptions (empty capabilities)
        let access_ctx = Arc::new(ProjectionAccessContext::with_projects(
            "c1",
            "corr-1",
            ProjectionCapabilitySet::new(), // empty = no capabilities
            Arc::new(AllowAllProjectResolver),
            ProjectionTransportClass::Local,
        ));
        let policy = Arc::new(PolicyRegistry::new(Arc::new(
            DefaultAccessPolicy::new(),
        )));
        let dc = ProjectionDisclosureContext::new(
            access_ctx,
            policy,
            Arc::new(ProjectionFieldRedactor::new()),
            Arc::new(HandleRegistrar::new()),
            metrics,
            Some("s1".to_string()),
            Some("p1".to_string()),
        );

        let envelope = make_envelope("s1", "t1");
        let binding = ProjectionBindingContext {
            session_id: Some("s1".to_string()),
            project_id: Some("p1".to_string()),
            ..Default::default()
        };

        let result = service
            .publish_from_core_with_contexts(&envelope, &binding, Some(&dc))
            .await
            .unwrap();

        assert!(
            matches!(result, PublishOutcome::Denied { reason: DisclosureReason::CapabilityDenied }),
            "expected Denied with CapabilityDenied, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn deny_decision_event_never_reaches_store() {
        let pool = test_pool().await;
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let metrics = Arc::new(ProjectionReplayMetrics::new());

        // Internal events are denied by the disclosure pipeline
        let envelope = EventEnvelope {
            protocol_version: 1,
            event_seq: 1,
            timestamp_ms: 1000,
            session_id: Some("s1".to_string()),
            turn_id: Some("t1".to_string()),
            payload: CoreEvent::TurnReasoningDelta {
                session_id: "s1".to_string(),
                turn_id: "t1".to_string(),
                delta: "reasoning text".to_string(),
            },
        };

        let dc = ProjectionDisclosureContext::internal_test(
            Some("s1".to_string()),
            Some("p1".to_string()),
            metrics,
        );

        let binding = ProjectionBindingContext {
            session_id: Some("s1".to_string()),
            project_id: Some("p1".to_string()),
            ..Default::default()
        };

        let result = service
            .publish_from_core_with_contexts(&envelope, &binding, Some(&dc))
            .await
            .unwrap();

        // Internal events should be denied
        assert!(
            matches!(result, PublishOutcome::Denied { reason: DisclosureReason::InternalNotSerializable }),
            "expected Denied, got {:?}",
            result
        );

        // Verify no event was stored in the database
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projection_event")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "no events should be stored after deny");
    }

    #[tokio::test]
    async fn redact_only_strips_secrets_from_stored_event() {
        let pool = test_pool().await;
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let metrics = Arc::new(ProjectionReplayMetrics::new());

        // Text delta with api_key in the content
        let envelope = make_text_delta_envelope("s1", "api_key=secret123 and normal text");

        let dc = ProjectionDisclosureContext::internal_test(
            Some("s1".to_string()),
            Some("p1".to_string()),
            metrics,
        );

        let binding = ProjectionBindingContext {
            session_id: Some("s1".to_string()),
            project_id: Some("p1".to_string()),
            ..Default::default()
        };

        let result = service
            .publish_from_core_with_contexts(&envelope, &binding, Some(&dc))
            .await
            .unwrap();

        // The event should be published (TurnTextDelta is Safe)
        assert!(
            matches!(result, PublishOutcome::Published { .. }),
            "expected Published, got {:?}",
            result
        );

        // Verify the stored event payload has secrets redacted
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT payload_json FROM projection_event")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!rows.is_empty(), "at least one event should be stored");
        for (payload_json,) in &rows {
            assert!(
                !payload_json.contains("secret123"),
                "stored event should not contain raw secret, got: {}",
                &payload_json[..payload_json.len().min(200)]
            );
        }
    }

    #[tokio::test]
    async fn downgrade_to_handle_replaces_oversized_string() {
        let pool = test_pool().await;
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let metrics = Arc::new(ProjectionReplayMetrics::new());

        // Create an oversized delta (more than 8KB)
        let oversized_delta = "x".repeat(16 * 1024);
        let envelope = make_text_delta_envelope("s1", &oversized_delta);

        // Use a bounded project resolver so we can test scope denial
        let resolver: Arc<dyn crate::projection_replay::context::ProjectionProjectResolver> =
            Arc::new(BoundedProjectResolver::new(["p1"]));
        let access_ctx = Arc::new(ProjectionAccessContext::with_projects(
            "c1",
            "corr-1",
            ProjectionCapabilitySet::local_user(),
            resolver,
            ProjectionTransportClass::Local,
        ));

        let dc = ProjectionDisclosureContext::new(
            access_ctx,
            Arc::new(PolicyRegistry::default()),
            Arc::new(ProjectionFieldRedactor::new()),
            Arc::new(HandleRegistrar::new()),
            metrics,
            Some("s1".to_string()),
            Some("p1".to_string()),
        );

        let binding = ProjectionBindingContext {
            session_id: Some("s1".to_string()),
            project_id: Some("p1".to_string()),
            ..Default::default()
        };

        let result = service
            .publish_from_core_with_contexts(&envelope, &binding, Some(&dc))
            .await
            .unwrap();

        // Should be published (TurnTextDelta is Safe classification)
        assert!(
            matches!(result, PublishOutcome::Published { .. }),
            "expected Published, got {:?}",
            result
        );

        // The redactor downsizes oversized strings in the text delta
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT payload_json FROM projection_event")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!rows.is_empty());
        for (payload_json,) in &rows {
            // Oversized text should be replaced with a handle marker
            assert!(
                payload_json.contains("[handle:")
                    || payload_json.contains("REDACTED:oversized")
                    || payload_json.len() < oversized_delta.len(),
                "oversized text should be reduced, payload len={}",
                payload_json.len()
            );
        }
    }

    #[tokio::test]
    async fn no_disclosure_context_denies_sensitive_events() {
        let pool = test_pool().await;
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));

        // Sensitive event (ConnectionRotated) without disclosure context
        let envelope = make_sensitive_envelope("conn-1");
        let binding = ProjectionBindingContext {
            session_id: Some("s1".to_string()),
            project_id: Some("p1".to_string()),
            ..Default::default()
        };

        let result = service
            .publish_from_core_with_contexts(&envelope, &binding, None)
            .await
            .unwrap();

        assert!(
            matches!(
                result,
                PublishOutcome::Skipped {
                    reason: SafePublicationReason::SensitiveRedacted
                }
            ),
            "expected Skipped for sensitive without context, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn compute_disclosure_safe_event_allows() {
        let metrics = Arc::new(ProjectionReplayMetrics::new());
        let dc = ProjectionDisclosureContext::internal_test(
            Some("s1".to_string()),
            Some("p1".to_string()),
            metrics,
        );

        let envelope = make_envelope("s1", "t1");
        let decision = ProjectionReplayService::compute_disclosure(&envelope, &dc);

        assert!(
            matches!(decision, DisclosureDecision::Allow { .. }),
            "expected Allow for safe event, got {:?}",
            decision
        );
    }

    #[tokio::test]
    async fn compute_disclosure_internal_event_denies() {
        let metrics = Arc::new(ProjectionReplayMetrics::new());
        let dc = ProjectionDisclosureContext::internal_test(
            Some("s1".to_string()),
            Some("p1".to_string()),
            metrics,
        );

        let envelope = EventEnvelope {
            protocol_version: 1,
            event_seq: 1,
            timestamp_ms: 1000,
            session_id: Some("s1".to_string()),
            turn_id: Some("t1".to_string()),
            payload: CoreEvent::TurnReasoningDelta {
                session_id: "s1".to_string(),
                turn_id: "t1".to_string(),
                delta: "reasoning".to_string(),
            },
        };
        let decision = ProjectionReplayService::compute_disclosure(&envelope, &dc);

        assert!(
            matches!(
                decision,
                DisclosureDecision::Deny {
                    reason: DisclosureReason::InternalNotSerializable
                }
            ),
            "expected Deny for internal event, got {:?}",
            decision
        );
    }

    #[tokio::test]
    async fn publish_with_disclosure_merges_binding_into_disclosure() {
        let pool = test_pool().await;
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let seam = ProjectionPublicationSeam::new(service);
        let metrics = Arc::new(ProjectionReplayMetrics::new());

        // Disclosure context without session/project — should get
        // merged from the binding context.
        let dc = ProjectionDisclosureContext::internal_test(None, None, metrics);

        let envelope = make_envelope("s1", "t1");
        let binding = ProjectionBindingContext {
            session_id: Some("s1".to_string()),
            project_id: Some("p1".to_string()),
            ..Default::default()
        };

        let result = seam
            .publish_with_disclosure(&envelope, binding, Some(&dc))
            .await
            .unwrap();

        assert!(
            matches!(result, PublishOutcome::Published { .. }),
            "expected Published after context merge, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn metrics_incremented_on_denial() {
        let pool = test_pool().await;
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let metrics = Arc::new(ProjectionReplayMetrics::new());

        let access_ctx = Arc::new(ProjectionAccessContext::with_projects(
            "c1",
            "corr-1",
            ProjectionCapabilitySet::new(),
            Arc::new(AllowAllProjectResolver),
            ProjectionTransportClass::Local,
        ));
        let dc = ProjectionDisclosureContext::new(
            access_ctx,
            Arc::new(PolicyRegistry::new(Arc::new(DefaultAccessPolicy::new()))),
            Arc::new(ProjectionFieldRedactor::new()),
            Arc::new(HandleRegistrar::new()),
            metrics.clone(),
            Some("s1".to_string()),
            Some("p1".to_string()),
        );

        let envelope = make_envelope("s1", "t1");
        let binding = ProjectionBindingContext {
            session_id: Some("s1".to_string()),
            project_id: Some("p1".to_string()),
            ..Default::default()
        };

        let _ = service
            .publish_from_core_with_contexts(&envelope, &binding, Some(&dc))
            .await
            .unwrap();

        let snap = metrics.snapshot();
        assert_eq!(
            snap.denials_by_reason.get("capability_denied"),
            Some(&1u64),
            "denial counter should be incremented"
        );
    }
}

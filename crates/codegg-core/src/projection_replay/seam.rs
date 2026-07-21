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
use crate::projection_replay::metrics::ProjectionReplayMetricsSnapshot;
use crate::projection_replay::service::{ProjectionReplayService, PublishOutcome};

/// Canonical context required to publish a projection event.
///
/// `SessionId`, `ProjectId`, `WorkspaceId`, and `binding_revision` are
/// resolved from `ProjectStorage` before this seam is invoked so the
/// adapter layer never invents identity. The caller (typically
/// `EventLog`'s sink hook) MAY pass `None` for any of these when no
/// canonical binding exists; the seam then fails closed with a
/// `Skipped { reason: UnboundSession }` outcome.
#[derive(Debug, Clone, Default)]
pub struct ProjectionPublicationContext {
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub binding_revision: i64,
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
    pub async fn resolve_context(
        &self,
        envelope: &EventEnvelope<CoreEvent>,
    ) -> ProjectionPublicationContext {
        let Some(session_id) = envelope
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
        else {
            return ProjectionPublicationContext::default();
        };

        if let Some(caller_ctx) = self.caller_provided_context(envelope) {
            return caller_ctx;
        }

        let Some(ref storage) = self.project_storage else {
            return ProjectionPublicationContext {
                session_id: Some(session_id),
                ..Default::default()
            };
        };

        match storage.session_binding(&session_id).await {
            Ok(Some(record)) if record.status == BindingStatus::Resolved => {
                ProjectionPublicationContext {
                    session_id: Some(session_id),
                    project_id: record.project_id.map(|p| p.as_str().to_string()),
                    workspace_id: record.workspace_id.map(|w| w.as_str().to_string()),
                    binding_revision: record.revision,
                }
            }
            _ => ProjectionPublicationContext {
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
    ) -> Option<ProjectionPublicationContext> {
        None
    }

    /// Publish an envelope using the resolved canonical context.
    ///
    /// The seam:
    /// 1. If the caller passed non-empty context (any field set),
    ///    honor that context directly.
    /// 2. Otherwise, resolve canonical context from `ProjectStorage`
    ///    using `envelope.session_id`.
    /// 3. Validate `session_id` is non-empty AND `project_id` is
    ///    resolved; otherwise return `Skipped { UnboundSession }`.
    /// 4. Delegate to
    ///    [`ProjectionReplayService::publish_from_core_with_context`].
    /// 5. Return a structured outcome the caller can log or ignore.
    pub async fn publish(
        &self,
        envelope: &EventEnvelope<CoreEvent>,
        caller_ctx: ProjectionPublicationContext,
    ) -> Result<PublishOutcome, StorageError> {
        let context = if caller_ctx.session_id.is_some()
            || caller_ctx.project_id.is_some()
            || caller_ctx.workspace_id.is_some()
            || caller_ctx.binding_revision != 0
        {
            // Caller provided explicit context; merge with envelope
            // session id when caller did not specify one.
            let mut ctx = caller_ctx.clone();
            if ctx.session_id.is_none() {
                ctx.session_id = envelope.session_id.clone();
            }
            ctx
        } else {
            self.resolve_context(envelope).await
        };
        self.inner
            .publish_from_core_with_context(envelope, &context)
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
}

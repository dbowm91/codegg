//! Disclosure policy engine (M3).
//!
//! One canonical pipeline runs for every event that is about to enter
//! shared projection storage or transport:
//!
//! 1. Typed source classification
//! 2. Access/scope policy evaluation against the
//!    [`ProjectionAccessContext`]
//! 3. Structural field transform
//! 4. Bounded heuristic text scan
//! 5. Size policy: inline / summary / handle / deny
//! 6. Normalized projection DTO
//! 7. Final serialized-byte validation
//! 8. Persist / checkpoint / live delivery
//!
//! The result is a [`DisclosureDecision`] that callers map to
//! durable rows or transport events. The decision never preserves
//! the raw denied/redacted value in its diagnostics, error chain,
//! debug formatting, metrics label, or tracing field.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::projection_replay::context::{ProjectionAccessContext, ProjectionCapability};

/// Stable, non-secret reason code for a disclosure decision.
///
/// Reason codes appear in metrics and diagnostics; their values
/// MUST be bounded and MUST NOT carry redacted payload content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureReason {
    /// Caller context lacks the capability for this scope.
    CapabilityDenied,
    /// Caller context cannot reach the requested project/session.
    ScopeDenied,
    /// The source event is internal and may not be serialized.
    InternalNotSerializable,
    /// The source event contains a sensitive value that has been
    /// replaced with a safe marker.
    SensitiveRedacted,
    /// A heuristic scan flagged a secret-like pattern; the value
    /// was downgraded to a safe summary.
    HeuristicSecretDetected,
    /// The serialized payload exceeded the bound; the value was
    /// downgraded to a summary or handle.
    OversizedDowngraded,
    /// The caller context only owns this client-local event; it is
    /// not eligible for shared storage.
    ClientLocalRestricted,
    /// The source variant is not yet known to the policy matrix;
    /// the value is denied.
    UnknownVariantDenied,
    /// The serializer failed; the value fails closed.
    SerializationFailed,
    /// The redaction pipeline failed; the value fails closed.
    RedactionFailed,
}

impl DisclosureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DisclosureReason::CapabilityDenied => "capability_denied",
            DisclosureReason::ScopeDenied => "scope_denied",
            DisclosureReason::InternalNotSerializable => "internal_not_serializable",
            DisclosureReason::SensitiveRedacted => "sensitive_redacted",
            DisclosureReason::HeuristicSecretDetected => "heuristic_secret_detected",
            DisclosureReason::OversizedDowngraded => "oversized_downgraded",
            DisclosureReason::ClientLocalRestricted => "client_local_restricted",
            DisclosureReason::UnknownVariantDenied => "unknown_variant_denied",
            DisclosureReason::SerializationFailed => "serialization_failed",
            DisclosureReason::RedactionFailed => "redaction_failed",
        }
    }
}

/// Disclosure policy decision.
///
/// Callers map the decision to durable rows or transport events.
/// A `Deny` or `ErrorFailClosed` outcome is never replaced with the
/// raw payload, and reason codes never include the original value.
#[derive(Debug, Clone)]
pub enum DisclosureDecision {
    /// The (possibly transformed) value is allowed for the caller.
    Allow {
        transformed: serde_json::Value,
        reason: Option<DisclosureReason>,
    },
    /// The value is allowed only for the owning client. It is never
    /// written to a shared durable row.
    ClientLocal {
        transformed: serde_json::Value,
        owner_client_id: String,
    },
    /// The value was downgraded to a bounded public summary; the
    /// original is not retained.
    Summarize {
        summary: serde_json::Value,
        reason: DisclosureReason,
    },
    /// The value was replaced with an opaque handle. The original
    /// lives behind an authorized read API.
    Handle {
        public_metadata: serde_json::Value,
        handle_id: String,
        reason: DisclosureReason,
    },
    /// The value is denied. No transformed payload is produced and
    /// the raw value is never returned to the caller.
    Deny { reason: DisclosureReason },
    /// The pipeline failed (serialization, redaction, etc.). The
    /// value fails closed; raw content is never returned.
    ErrorFailClosed { reason: DisclosureReason },
}

impl DisclosureDecision {
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            DisclosureDecision::Allow { .. }
                | DisclosureDecision::Summarize { .. }
                | DisclosureDecision::Handle { .. }
        )
    }

    pub fn reason_code(&self) -> DisclosureReason {
        match self {
            DisclosureDecision::Allow {
                reason: Some(r), ..
            } => *r,
            DisclosureDecision::Allow { reason: None, .. } => DisclosureReason::CapabilityDenied,
            DisclosureDecision::ClientLocal { .. } => DisclosureReason::ClientLocalRestricted,
            DisclosureDecision::Summarize { reason, .. } => *reason,
            DisclosureDecision::Handle { reason, .. } => *reason,
            DisclosureDecision::Deny { reason } => *reason,
            DisclosureDecision::ErrorFailClosed { reason } => *reason,
        }
    }
}

/// Policy engine contract.
///
/// A future team/role model can implement this trait without
/// changing the projection DTOs or replay storage.
pub trait ProjectionAccessPolicy: Send + Sync {
    /// Authorize the caller to subscribe to a given projection
    /// stream scope. Returns `true` if the request should be
    /// accepted.
    fn authorize_subscribe(
        &self,
        ctx: &ProjectionAccessContext,
        project_id: &str,
        session_id: Option<&str>,
    ) -> bool;

    /// Authorize the caller to read a given artifact handle.
    fn authorize_artifact_read(
        &self,
        ctx: &ProjectionAccessContext,
        project_id: &str,
        kind: ArtifactReadKind,
    ) -> bool;
}

/// Capability that authorizes a given artifact read kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactReadKind {
    RunArtifact,
    ToolOutput,
    DiffOrLog,
}

/// Default policy engine: pure capability + scope check, no
/// role-named logic. The daemon constructs one of these at
/// startup; tests construct bespoke ones via
/// [`DefaultAccessPolicy::with_admin_bypass`].
#[derive(Default, Clone)]
pub struct DefaultAccessPolicy {
    allow_admin_bypass: bool,
}

impl std::fmt::Debug for DefaultAccessPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultAccessPolicy")
            .field("allow_admin_bypass", &self.allow_admin_bypass)
            .finish()
    }
}

impl DefaultAccessPolicy {
    pub fn new() -> Self {
        Self {
            allow_admin_bypass: false,
        }
    }

    /// Build a policy that honors `AdminBypass` capability. Only
    /// the harness-internal test transport should call this.
    pub fn with_admin_bypass() -> Self {
        Self {
            allow_admin_bypass: true,
        }
    }
}

impl ProjectionAccessPolicy for DefaultAccessPolicy {
    fn authorize_subscribe(
        &self,
        ctx: &ProjectionAccessContext,
        project_id: &str,
        session_id: Option<&str>,
    ) -> bool {
        ctx.authorize_scope(project_id, session_id)
    }

    fn authorize_artifact_read(
        &self,
        ctx: &ProjectionAccessContext,
        project_id: &str,
        kind: ArtifactReadKind,
    ) -> bool {
        if !ctx.project_resolver.is_allowed(project_id) {
            return false;
        }
        let cap = match kind {
            ArtifactReadKind::RunArtifact => ProjectionCapability::ReadRunArtifact,
            ArtifactReadKind::ToolOutput => ProjectionCapability::ReadToolOutput,
            ArtifactReadKind::DiffOrLog => ProjectionCapability::ReadDiffOrLog,
        };
        if !ctx.has(cap) {
            return false;
        }
        true
    }
}

/// Process-wide policy holder.
///
/// Wraps an `Arc<dyn ProjectionAccessPolicy>` so future authority
/// modules can hot-swap the policy without rebuilding the service.
#[derive(Clone)]
pub struct PolicyRegistry {
    policy: Arc<dyn ProjectionAccessPolicy>,
}

impl std::fmt::Debug for PolicyRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyRegistry").finish_non_exhaustive()
    }
}

impl PolicyRegistry {
    pub fn new(policy: Arc<dyn ProjectionAccessPolicy>) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &Arc<dyn ProjectionAccessPolicy> {
        &self.policy
    }
}

impl Default for PolicyRegistry {
    fn default() -> Self {
        Self::new(Arc::new(DefaultAccessPolicy::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection_replay::context::{
        AllowAllProjectResolver, BoundedProjectResolver, ProjectionCapabilitySet,
        ProjectionTransportClass,
    };

    #[test]
    fn default_policy_denies_subscribe_when_capability_missing() {
        let ctx = ProjectionAccessContext::with_projects(
            "c1",
            "corr-1",
            ProjectionCapabilitySet::new(),
            Arc::new(AllowAllProjectResolver),
            ProjectionTransportClass::Local,
        );
        let policy = DefaultAccessPolicy::new();
        assert!(!policy.authorize_subscribe(&ctx, "p1", Some("s1")));
    }

    #[test]
    fn default_policy_authorizes_local_user_for_allowed_project() {
        let ctx = ProjectionAccessContext::local("c1", "corr-1");
        let policy = DefaultAccessPolicy::new();
        assert!(policy.authorize_subscribe(&ctx, "p1", Some("s1")));
    }

    #[test]
    fn default_policy_denies_artifact_read_outside_project() {
        let resolver: Arc<dyn crate::projection_replay::context::ProjectionProjectResolver> =
            Arc::new(BoundedProjectResolver::new(["p1"]));
        let ctx = ProjectionAccessContext::with_projects(
            "c1",
            "corr-1",
            ProjectionCapabilitySet::local_user(),
            resolver,
            ProjectionTransportClass::Local,
        );
        let policy = DefaultAccessPolicy::new();
        assert!(policy.authorize_artifact_read(&ctx, "p1", ArtifactReadKind::RunArtifact));
        assert!(!policy.authorize_artifact_read(&ctx, "p2", ArtifactReadKind::RunArtifact));
    }

    #[test]
    fn decision_codes_are_bounded() {
        let d = DisclosureDecision::Deny {
            reason: DisclosureReason::CapabilityDenied,
        };
        assert_eq!(d.reason_code().as_str(), "capability_denied");
    }
}

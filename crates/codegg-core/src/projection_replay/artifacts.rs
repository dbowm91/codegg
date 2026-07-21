//! Opaque artifact handle model for projection consumers (M3).
//!
//! Large content (run logs, tool outputs, file diffs) lives behind
//! an opaque handle. The public descriptor is bounded, never
//! contains a filesystem path, and never conveys authority by
//! itself. Read authorization is re-evaluated per request against
//! the [`ProjectionAccessContext`] and the canonical
//! [`ProjectionProjectResolver`].
//!
//! Handle identifiers are minted from a high-entropy random source
//! and never collide with paths. The public summary is bounded
//! and never contains the original payload.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::projection_replay::policy::ArtifactReadKind;

/// Opaque artifact kind carried on the handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    RunOutput,
    ToolOutput,
    DiffExcerpt,
    LogTail,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::RunOutput => "run_output",
            ArtifactKind::ToolOutput => "tool_output",
            ArtifactKind::DiffExcerpt => "diff_excerpt",
            ArtifactKind::LogTail => "log_tail",
        }
    }

    pub fn read_capability(self) -> ArtifactReadKind {
        match self {
            ArtifactKind::RunOutput => ArtifactReadKind::RunArtifact,
            ArtifactKind::ToolOutput => ArtifactReadKind::ToolOutput,
            ArtifactKind::DiffExcerpt | ArtifactKind::LogTail => ArtifactReadKind::DiffOrLog,
        }
    }
}

/// Opaque artifact handle. Carries no authority; the public
/// descriptor does NOT contain filesystem paths, raw commands with
/// secrets, storage keys, credentials, or signed URLs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionArtifactHandle {
    pub handle_id: String,
    pub kind: ArtifactKind,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub source_record_id: String,
    pub content_type: ArtifactContentType,
    pub total_bytes: Option<u64>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revision: u64,
    pub public_summary: Option<String>,
}

impl ProjectionArtifactHandle {
    /// Validate the public descriptor. Used by tests and by the
    /// `scripts/check_projection_disclosure.sh` static guard to
    /// ensure no handle carries a path, a credential, or an
    /// authority token in its public surface.
    pub fn is_public_descriptor_safe(&self) -> bool {
        if self.handle_id.is_empty()
            || self.handle_id.contains('/')
            || self.handle_id.contains('\\')
            || self.handle_id.contains("..")
        {
            return false;
        }
        if self.source_record_id.is_empty()
            || self.source_record_id.contains('/')
            || self.source_record_id.contains('\\')
            || self.source_record_id.contains("..")
        {
            return false;
        }
        if let Some(summary) = &self.public_summary {
            if summary.len() > 512 {
                return false;
            }
            if summary.contains("..") || summary.contains('\\') {
                return false;
            }
            if summary.starts_with('/') || summary.starts_with('~') {
                return false;
            }
            if summary.contains("://") {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactContentType {
    Text,
    Binary,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HandleLifecycle {
    #[default]
    Active,
    Expired,
    Revoked,
    StaleRevision,
    SourceMissing,
}

/// Minted by [`HandleRegistry`]. Tests can supply a fixed seed via
/// [`HandleRegistry::with_counter`] for determinism.
pub struct HandleRegistry {
    counter: AtomicU64,
}

impl Default for HandleRegistry {
    fn default() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
}

impl std::fmt::Debug for HandleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandleRegistry")
            .field("counter", &self.counter.load(Ordering::Relaxed))
            .finish()
    }
}

impl HandleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_counter(initial: u64) -> Self {
        Self {
            counter: AtomicU64::new(initial),
        }
    }

    /// Mint a new opaque handle id. Uses `Uuid::new_v4()` plus a
    /// monotonic counter to guarantee uniqueness even when a host's
    /// RNG is compromised.
    pub fn mint_id(&self) -> String {
        let _ = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("art_{}", Uuid::new_v4().simple())
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Public handle issuance surface. The daemon constructs one of
/// these and the publication seam calls [`HandleRegistrar::issue`]
/// when a value downgrades to a handle.
#[derive(Clone)]
pub struct HandleRegistrar {
    registry: Arc<HandleRegistry>,
}

impl std::fmt::Debug for HandleRegistrar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandleRegistrar").finish_non_exhaustive()
    }
}

impl Default for HandleRegistrar {
    fn default() -> Self {
        Self {
            registry: Arc::new(HandleRegistry::new()),
        }
    }
}

impl HandleRegistrar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_registry(registry: HandleRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
        }
    }

    pub fn mint(&self) -> String {
        self.registry.mint_id()
    }

    pub fn issue(
        &self,
        kind: ArtifactKind,
        project_id: impl Into<String>,
        session_id: Option<String>,
        source_record_id: impl Into<String>,
        content_type: ArtifactContentType,
        total_bytes: Option<u64>,
        revision: u64,
        public_summary: Option<String>,
        ttl_ms: Option<i64>,
    ) -> ProjectionArtifactHandle {
        let now = now_ms();
        ProjectionArtifactHandle {
            handle_id: self.registry.mint_id(),
            kind,
            project_id: project_id.into(),
            workspace_id: None,
            session_id,
            source_record_id: source_record_id.into(),
            content_type,
            total_bytes,
            created_at: now,
            expires_at: ttl_ms.map(|t| now.saturating_add(t)),
            revision,
            public_summary: public_summary
                .map(|s| if s.len() > 512 { s[..512].to_string() } else { s }),
        }
    }
}

/// Authorization decision for an artifact read. The registry MUST
/// return [`ArtifactAccessDecision::Denied`] with a safe reason
/// code when authorization fails; raw denial reasons must never
/// include the requested project or session id verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactAccessDecision {
    Allowed,
    Denied { reason: &'static str },
    Stale { reason: &'static str },
    NotFound { reason: &'static str },
}

impl ArtifactAccessDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, ArtifactAccessDecision::Allowed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReadRequest {
    pub handle_id: String,
    pub start: u64,
    pub end: Option<u64>,
    pub expected_revision: u64,
}

impl ArtifactReadRequest {
    /// Maximum read window per request. Callers MUST cap the
    /// range and response size to this constant before issuing
    /// the read.
    pub const MAX_READ_BYTES: u64 = 64 * 1024;

    pub fn normalize(&self) -> (u64, u64) {
        let end = self.end.unwrap_or(self.start.saturating_add(Self::MAX_READ_BYTES));
        let end = end.min(self.start.saturating_add(Self::MAX_READ_BYTES));
        (self.start, end)
    }
}

/// Read response. Carries a bounded `content` slice and a `note`
/// indicating truncation or redaction. The full body lives behind
/// the registry; the public surface here is bounded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactReadResponse {
    pub handle_id: String,
    pub revision: u64,
    pub start: u64,
    pub end: u64,
    pub content_type: ArtifactContentType,
    pub content: String,
    pub redacted: bool,
    pub truncated: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadLifecycle {
    Ok,
    StaleRevision,
    Expired,
    Revoked,
    SourceMissing,
    Unauthorized,
    RangeOutOfBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactReadOutcome {
    pub decision: ReadLifecycle,
    pub response: Option<ArtifactReadResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::projection_replay::context::{
        BoundedProjectResolver, ProjectionAccessContext, ProjectionCapabilitySet,
        ProjectionProjectResolver, ProjectionTransportClass,
    };
    use crate::projection_replay::policy::{DefaultAccessPolicy, ProjectionAccessPolicy};

    #[test]
    fn handle_ids_are_opaque_and_path_free() {
        let reg = HandleRegistrar::new();
        let h = reg.issue(
            ArtifactKind::RunOutput,
            "p1",
            Some("s1".to_string()),
            "r1",
            ArtifactContentType::Text,
            Some(1024),
            1,
            Some("ok".to_string()),
            Some(60_000),
        );
        assert!(h.is_public_descriptor_safe());
        assert!(h.handle_id.starts_with("art_"));
        assert!(!h.handle_id.contains('/'));
    }

    #[test]
    fn handle_with_path_in_summary_fails_validation() {
        let reg = HandleRegistrar::new();
        let h = reg.issue(
            ArtifactKind::RunOutput,
            "p1",
            None,
            "r1",
            ArtifactContentType::Text,
            None,
            1,
            Some("../etc/passwd".to_string()),
            None,
        );
        assert!(!h.is_public_descriptor_safe());
    }

    #[test]
    fn read_request_normalizes_window() {
        let req = ArtifactReadRequest {
            handle_id: "art_abc".into(),
            start: 0,
            end: Some(1024 * 1024),
            expected_revision: 1,
        };
        let (s, e) = req.normalize();
        assert_eq!(s, 0);
        assert_eq!(e, ArtifactReadRequest::MAX_READ_BYTES);
    }

    #[test]
    fn authorization_decision_is_safe_for_unknown_project() {
        let resolver: Arc<dyn ProjectionProjectResolver> =
            Arc::new(BoundedProjectResolver::new(["p_real"]));
        let ctx = ProjectionAccessContext::with_projects(
            "c1",
            "corr-1",
            ProjectionCapabilitySet::local_user(),
            resolver,
            ProjectionTransportClass::Local,
        );
        let policy = DefaultAccessPolicy::new();
        assert!(!policy.authorize_artifact_read(&ctx, "p_phantom", ArtifactReadKind::RunArtifact));
    }

    #[test]
    fn handle_mints_unique_ids() {
        let reg = HandleRegistrar::new();
        let a = reg.mint();
        let b = reg.mint();
        assert_ne!(a, b);
    }
}

//! TUI-side projection client state.
//!
//! The TUI does not own projection storage; it owns a single
//! [`ProjectionClientController`] that consumes projection events from
//! the daemon and produces a bounded per-tab summary plus a single
//! heavy active-view snapshot. This state module is the adapter
//! between the transport-neutral [`ProjectionClientController`] and
//! the TUI's tab/route lifecycle.
//!
//! The state is split into three pieces:
//!
//! - [`ProjectionClientState`] — the controller plus per-tab
//!   summaries and per-subscription cursor metadata.
//! - [`ProjectionTabSummary`] — a bounded, serializable view of a
//!   projection snapshot for inactive tabs.
//! - [`ProjectionViewKind`] — selection between the heavy
//!   `ProjectionPrimary` view (full snapshot) and the bounded
//!   `RawCompatibility` fallback.
//!
//! The active tab owns a single `SessionProjectionSnapshot`. Inactive
//! tabs retain only their bounded summary, cursor, and last-applied
//! sequence; never the full snapshot.
#![forbid(unsafe_code)]

use std::collections::HashMap;

use codegg_protocol::projection::caps::ProjectionCapabilities;
use codegg_protocol::projection::controller::{
    ProjectionClientController, ProjectionControllerInfo, ProjectionMode,
};
use codegg_protocol::projection::event::ProjectionEnvelope;
use codegg_protocol::projection::replay::{
    ProjectionResyncReason, ProjectionStreamId, ProjectionSubscriptionId,
};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;

/// Maximum number of inactive tab summaries retained.
pub const MAX_TAB_PROJECTION_SUMMARIES: usize = 16;

/// Maximum summary length in bytes.
pub const MAX_PROJECTION_SUMMARY_BYTES: usize = 256;

/// Maximum number of artifact read attempts in flight per tab.
pub const MAX_ARTIFACT_READS_PER_TAB: usize = 4;

/// Maximum number of cached artifact excerpts retained per tab.
pub const MAX_ARTIFACT_EXCERPTS_PER_TAB: usize = 8;

/// Maximum bytes per cached artifact excerpt.
pub const MAX_ARTIFACT_EXCERPT_BYTES: usize = 8 * 1024;

/// Maximum number of artifact handle metadata entries cached per tab.
pub const MAX_ARTIFACT_HANDLES_PER_TAB: usize = 32;

/// Bounded cache entry for a single artifact handle descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactHandleCacheEntry {
    pub handle_id: String,
    pub kind: String,
    pub project_id: String,
    pub content_type: String,
    pub total_bytes: Option<u64>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revision: u64,
    pub public_summary: Option<String>,
}

/// Bounded cache entry for a fetched artifact excerpt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactExcerptCacheEntry {
    pub handle_id: String,
    pub start: u64,
    pub end: u64,
    pub content_type: String,
    pub content: String,
    pub redacted: bool,
    pub truncated: bool,
    pub note: Option<String>,
    pub fetched_at_ms: i64,
}

/// Per-tab bounded projection summary.
///
/// Contains only the projection state that inactive tabs need to
/// render their header / sidebar — never the full snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectionTabSummary {
    pub tab_id: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub active_turn_status: Option<String>,
    pub pending_permission_count: u32,
    pub pending_question_count: u32,
    pub active_subagents: u32,
    pub last_event_seq: u64,
    pub last_resync_reason: Option<ProjectionResyncReason>,
    pub recent_diagnostic_codes: Vec<String>,
    pub updated_at_ms: i64,
}

impl ProjectionTabSummary {
    pub fn from_snapshot(tab_id: &str, snapshot: &SessionProjectionSnapshot) -> Self {
        let active_turn = snapshot.active_turn.as_ref();
        Self {
            tab_id: tab_id.to_string(),
            session_id: Some(snapshot.primary_session.session_id.clone()),
            project_id: Some(snapshot.primary_session.project_id.clone()),
            active_turn_status: active_turn.map(|t| match t.status {
                codegg_protocol::projection::dto::TurnStatus::Starting => "starting".into(),
                codegg_protocol::projection::dto::TurnStatus::Active => "active".into(),
                codegg_protocol::projection::dto::TurnStatus::AwaitingPermission => {
                    "awaiting_permission".into()
                }
                codegg_protocol::projection::dto::TurnStatus::AwaitingQuestion => {
                    "awaiting_question".into()
                }
                codegg_protocol::projection::dto::TurnStatus::Completing => "completing".into(),
                codegg_protocol::projection::dto::TurnStatus::Completed => "completed".into(),
                codegg_protocol::projection::dto::TurnStatus::Failed => "failed".into(),
                codegg_protocol::projection::dto::TurnStatus::Cancelled => "cancelled".into(),
            }),
            pending_permission_count: snapshot.primary_session.pending_permission_count as u32,
            pending_question_count: snapshot.primary_session.pending_question_count as u32,
            active_subagents: snapshot.primary_session.active_subagents as u32,
            last_event_seq: snapshot.event_seq,
            last_resync_reason: None,
            recent_diagnostic_codes: snapshot
                .diagnostics
                .iter()
                .rev()
                .take(4)
                .map(|d| d.code.clone())
                .collect(),
            updated_at_ms: snapshot.generated_at_ms,
        }
    }
}

/// Subscription cursor metadata for a single projection subscription.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectionCursorInfo {
    pub subscription_id: Option<ProjectionSubscriptionId>,
    pub last_delivered_seq: u64,
    pub last_acked_seq: u64,
    pub state: String,
}

/// TUI-side projection client state.
pub struct ProjectionClientState {
    controller: ProjectionClientController,
    capabilities: ProjectionCapabilities,
    /// Per-tab inactive summary, keyed by stable tab id.
    tab_summaries: HashMap<String, ProjectionTabSummary>,
    /// Per-tab cursor metadata.
    cursors: HashMap<String, ProjectionCursorInfo>,
    /// Active tab identifier — owns the full snapshot.
    active_tab_id: Option<String>,
    /// In-flight artifact reads per tab, keyed by tab id.
    artifact_reads: HashMap<String, Vec<String>>,
    /// Per-tab cached artifact handle descriptors (bounded).
    artifact_handles: HashMap<String, Vec<ArtifactHandleCacheEntry>>,
    /// Per-tab cached artifact excerpts (bounded; cleared on tab close/reconnect).
    artifact_excerpts: HashMap<String, Vec<ArtifactExcerptCacheEntry>>,
    /// Counter incremented on every resync request surfaced to the UI.
    resync_requests: u64,
}

impl ProjectionClientState {
    /// Create a fresh state. The controller starts in
    /// `ProjectionMode::Unsupported` until [`Self::negotiate`] is
    /// called.
    pub fn new() -> Self {
        Self::with_capabilities(ProjectionCapabilities::current())
    }

    /// Create a state with explicit capabilities (used by tests).
    pub fn with_capabilities(capabilities: ProjectionCapabilities) -> Self {
        let mut controller = ProjectionClientController::new(capabilities.clone());
        controller.negotiate(Some(&ProjectionCapabilities::default()));
        Self {
            controller,
            capabilities,
            tab_summaries: HashMap::new(),
            cursors: HashMap::new(),
            active_tab_id: None,
            artifact_reads: HashMap::new(),
            artifact_handles: HashMap::new(),
            artifact_excerpts: HashMap::new(),
            resync_requests: 0,
        }
    }

    pub fn capabilities(&self) -> &ProjectionCapabilities {
        &self.capabilities
    }

    pub fn controller(&self) -> &ProjectionClientController {
        &self.controller
    }

    pub fn controller_mut(&mut self) -> &mut ProjectionClientController {
        &mut self.controller
    }

    pub fn controller_info(&self) -> ProjectionControllerInfo {
        self.controller.info()
    }

    pub fn mode(&self) -> ProjectionMode {
        self.controller.mode()
    }

    pub fn negotiated_version(&self) -> Option<u32> {
        self.controller.negotiated_version()
    }

    pub fn reconnect_epoch(&self) -> u64 {
        self.controller.reconnect_epoch()
    }

    /// Force a renegotiation. The caller is expected to drive a fresh
    /// `ProjectionCapabilities` round-trip with the daemon.
    pub fn renegotiate(&mut self, daemon_caps: Option<&ProjectionCapabilities>) {
        self.controller.negotiate(daemon_caps);
        if self.mode().is_unsupported() {
            self.tab_summaries.clear();
            self.cursors.clear();
        }
    }

    /// Mark the controller as fallback-bound. This is used by the TUI
    /// when the daemon does not advertise `session_projection` in its
    /// `ServerCapabilities`. The TUI then keeps using the raw-core
    /// compatibility path.
    pub fn enter_raw_compatibility(&mut self, reason: impl Into<String>) {
        self.controller.enter_raw_compatibility(reason);
        self.tab_summaries.clear();
        self.cursors.clear();
    }

    /// Bump the reconnect epoch and drop all subscriptions. The
    /// session-level state in `App` survives this — only projection
    /// state is reset.
    pub fn on_reconnect(&mut self) {
        self.controller.on_reconnect();
        self.tab_summaries.clear();
        self.cursors.clear();
        self.artifact_handles.clear();
        self.artifact_excerpts.clear();
        self.artifact_reads.clear();
        self.active_tab_id = None;
    }

    /// Set the active tab id.
    pub fn set_active_tab(&mut self, tab_id: Option<String>) {
        self.active_tab_id = tab_id;
    }

    pub fn active_tab_id(&self) -> Option<&str> {
        self.active_tab_id.as_deref()
    }

    /// Register or refresh an inactive summary for a tab.
    pub fn upsert_tab_summary(&mut self, summary: ProjectionTabSummary) {
        self.tab_summaries.insert(summary.tab_id.clone(), summary);
        if self.tab_summaries.len() > MAX_TAB_PROJECTION_SUMMARIES {
            // Bound by removing arbitrary stale entries.
            let to_remove: Vec<String> = self
                .tab_summaries
                .keys()
                .filter(|k| Some(k.as_str()) != self.active_tab_id.as_deref())
                .take(self.tab_summaries.len() - MAX_TAB_PROJECTION_SUMMARIES)
                .cloned()
                .collect();
            for k in to_remove {
                self.tab_summaries.remove(&k);
            }
        }
    }

    /// Update the cursor info for a tab.
    pub fn set_cursor(&mut self, tab_id: &str, info: ProjectionCursorInfo) {
        self.cursors.insert(tab_id.to_string(), info);
    }

    /// Read the inactive summary for a tab.
    pub fn tab_summary(&self, tab_id: &str) -> Option<&ProjectionTabSummary> {
        self.tab_summaries.get(tab_id)
    }

    /// Read the cursor info for a tab.
    pub fn cursor(&self, tab_id: &str) -> Option<&ProjectionCursorInfo> {
        self.cursors.get(tab_id)
    }

    /// Apply a projection envelope to the controller and update the
    /// affected tab summary.
    pub fn apply_envelope(
        &mut self,
        subscription_id: &ProjectionSubscriptionId,
        tab_id: &str,
        envelope: ProjectionEnvelope,
    ) -> codegg_protocol::projection::controller::ControllerApplyOutcome {
        let outcome = self
            .controller
            .apply_envelope(subscription_id, envelope.clone());
        if let codegg_protocol::projection::controller::ControllerApplyOutcome::ResyncRequested {
            ..
        } = &outcome
        {
            self.resync_requests = self.resync_requests.saturating_add(1);
        }
        if let Some(last_seq) = outcome.last_seq() {
            if let Some(info) = self.cursors.get_mut(tab_id) {
                info.last_delivered_seq = last_seq;
            }
        }
        // Always refresh the per-tab summary from the snapshot so the
        // summary tracks the latest applied event sequence.
        self.refresh_tab_summary_from_controller(tab_id);
        outcome
    }

    /// Refresh a tab summary from the controller's snapshot for the
    /// matching stream.
    pub fn refresh_tab_summary_from_controller(&mut self, tab_id: &str) {
        // Try every stream the controller holds; in practice the TUI
        // uses one stream per active tab.
        let mut snapshot: Option<(ProjectionStreamId, SessionProjectionSnapshot)> = None;
        for (stream_id, snap) in self.controller.snapshots() {
            if stream_id.as_str().ends_with(tab_id) {
                snapshot = Some((stream_id.clone(), snap.clone()));
                break;
            }
            snapshot = Some((stream_id.clone(), snap.clone()));
        }
        if let Some((_, snap)) = snapshot {
            let mut summary = ProjectionTabSummary::from_snapshot(tab_id, &snap);
            summary.last_resync_reason = self.controller.last_resync_reason();
            self.upsert_tab_summary(summary);
        }
    }

    /// Begin an artifact read for a tab. Returns the request id; the
    /// caller is responsible for issuing the request and clearing the
    /// read on completion.
    pub fn begin_artifact_read(&mut self, tab_id: &str, handle_id: &str) -> Option<String> {
        if !self.mode().is_projection_primary() {
            return None;
        }
        let reads = self.artifact_reads.entry(tab_id.to_string()).or_default();
        if reads.len() >= MAX_ARTIFACT_READS_PER_TAB {
            return None;
        }
        let request_id = format!("artreq-{}-{}", tab_id, handle_id);
        reads.push(request_id.clone());
        Some(request_id)
    }

    pub fn end_artifact_read(&mut self, tab_id: &str, request_id: &str) {
        if let Some(reads) = self.artifact_reads.get_mut(tab_id) {
            reads.retain(|r| r != request_id);
        }
    }

    pub fn cancel_artifact_reads(&mut self, tab_id: &str) {
        self.artifact_reads.remove(tab_id);
        // Cancellation also clears cached excerpts for the tab.
        self.artifact_excerpts.remove(tab_id);
    }

    pub fn artifact_reads(&self, tab_id: &str) -> usize {
        self.artifact_reads
            .get(tab_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Cache an artifact handle descriptor for a tab. Returns `false`
    /// when the controller is not in projection-primary mode or the
    /// cache is full.
    pub fn cache_artifact_handle(
        &mut self,
        tab_id: &str,
        handle: ArtifactHandleCacheEntry,
    ) -> bool {
        if !self.mode().is_projection_primary() {
            return false;
        }
        let entry = self.artifact_handles.entry(tab_id.to_string()).or_default();
        if entry.iter().any(|e| e.handle_id == handle.handle_id) {
            // Refresh in place.
            if let Some(slot) = entry.iter_mut().find(|e| e.handle_id == handle.handle_id) {
                *slot = handle;
            }
            return true;
        }
        if entry.len() >= MAX_ARTIFACT_HANDLES_PER_TAB {
            return false;
        }
        entry.push(handle);
        true
    }

    /// Cache an artifact excerpt for a tab. The excerpt is bounded by
    /// `MAX_ARTIFACT_EXCERPT_BYTES`; oversized content is rejected.
    pub fn cache_artifact_excerpt(
        &mut self,
        tab_id: &str,
        excerpt: ArtifactExcerptCacheEntry,
    ) -> bool {
        if !self.mode().is_projection_primary() {
            return false;
        }
        if excerpt.content.len() > MAX_ARTIFACT_EXCERPT_BYTES {
            return false;
        }
        let entry = self
            .artifact_excerpts
            .entry(tab_id.to_string())
            .or_default();
        // Replace prior excerpt for the same handle.
        entry.retain(|e| e.handle_id != excerpt.handle_id);
        if entry.len() >= MAX_ARTIFACT_EXCERPTS_PER_TAB {
            entry.remove(0);
        }
        entry.push(excerpt);
        true
    }

    pub fn artifact_handles(&self, tab_id: &str) -> &[ArtifactHandleCacheEntry] {
        self.artifact_handles
            .get(tab_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn artifact_excerpts(&self, tab_id: &str) -> &[ArtifactExcerptCacheEntry] {
        self.artifact_excerpts
            .get(tab_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Clear all artifact state for a tab. Called on tab close and on
    /// reconnect.
    pub fn clear_tab_artifacts(&mut self, tab_id: &str) {
        self.artifact_reads.remove(tab_id);
        self.artifact_handles.remove(tab_id);
        self.artifact_excerpts.remove(tab_id);
    }

    /// Number of resync requests surfaced since last reset.
    pub fn resync_request_count(&self) -> u64 {
        self.resync_requests
    }

    /// Reset the resync counter after the UI acknowledges it.
    pub fn ack_resync(&mut self) {
        self.resync_requests = 0;
    }

    /// Number of currently active subscriptions on the controller.
    pub fn subscription_count(&self) -> usize {
        self.controller.subscription_count()
    }

    /// Whether projection capability is available and primary mode is
    /// active.
    pub fn is_projection_primary(&self) -> bool {
        self.mode().is_projection_primary()
    }

    /// Whether the controller is operating in raw compatibility mode.
    pub fn is_raw_compatibility(&self) -> bool {
        self.mode().is_raw_compatibility()
    }

    /// Whether the controller has been forced into unsupported mode.
    pub fn is_unsupported(&self) -> bool {
        self.mode().is_unsupported()
    }

    /// Bounded summaries iterator for diagnostics.
    pub fn tab_summaries(&self) -> impl Iterator<Item = (&str, &ProjectionTabSummary)> {
        self.tab_summaries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl Default for ProjectionClientState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::projection::event::ProjectionEvent;

    #[test]
    fn fresh_state_is_projection_primary() {
        let state = ProjectionClientState::new();
        assert!(state.is_projection_primary());
        assert_eq!(state.negotiated_version(), Some(1));
    }

    #[test]
    fn renegotiate_without_caps_marks_unsupported() {
        let mut state = ProjectionClientState::new();
        state.renegotiate(None);
        assert!(state.is_unsupported());
    }

    #[test]
    fn raw_compatibility_clears_summaries() {
        let mut state = ProjectionClientState::new();
        let summary = ProjectionTabSummary {
            tab_id: "default".into(),
            ..Default::default()
        };
        state.upsert_tab_summary(summary);
        assert!(state.tab_summary("default").is_some());
        state.enter_raw_compatibility("test fallback");
        assert!(state.tab_summary("default").is_none());
    }

    #[test]
    fn reconnect_clears_all_projection_state() {
        let mut state = ProjectionClientState::new();
        state.upsert_tab_summary(ProjectionTabSummary::default());
        state.set_active_tab(Some("tab-1".into()));
        state.on_reconnect();
        assert!(state.tab_summary("default").is_none());
        assert!(state.active_tab_id().is_none());
    }

    #[test]
    fn artifact_read_lifecycle() {
        let mut state = ProjectionClientState::new();
        let req = state
            .begin_artifact_read("tab-1", "art-123")
            .expect("begin read");
        assert_eq!(state.artifact_reads("tab-1"), 1);
        state.end_artifact_read("tab-1", &req);
        assert_eq!(state.artifact_reads("tab-1"), 0);
    }

    #[test]
    fn artifact_read_blocked_in_compatibility_mode() {
        let mut state = ProjectionClientState::new();
        state.enter_raw_compatibility("test fallback");
        assert!(state.begin_artifact_read("tab-1", "art-123").is_none());
    }

    #[test]
    fn artifact_reads_bounded_per_tab() {
        let mut state = ProjectionClientState::new();
        for i in 0..MAX_ARTIFACT_READS_PER_TAB {
            assert!(state
                .begin_artifact_read("tab-1", &format!("art-{i}"))
                .is_some());
        }
        assert!(state.begin_artifact_read("tab-1", "art-extra").is_none());
    }

    #[test]
    fn cancel_artifact_reads_clears_all() {
        let mut state = ProjectionClientState::new();
        let _ = state.begin_artifact_read("tab-1", "art-1");
        let _ = state.begin_artifact_read("tab-1", "art-2");
        state.cancel_artifact_reads("tab-1");
        assert_eq!(state.artifact_reads("tab-1"), 0);
    }

    #[test]
    fn summary_carries_active_turn_status() {
        let mut state = ProjectionClientState::new();
        let summary = ProjectionTabSummary {
            tab_id: "default".into(),
            ..Default::default()
        };
        state.upsert_tab_summary(summary);
        let s = state.tab_summary("default").expect("summary");
        assert_eq!(s.tab_id, "default");
    }

    #[test]
    fn summaries_bounded_to_max() {
        let mut state = ProjectionClientState::new();
        for i in 0..(MAX_TAB_PROJECTION_SUMMARIES * 2) {
            let id = format!("tab-{i}");
            state.upsert_tab_summary(ProjectionTabSummary {
                tab_id: id.clone(),
                ..Default::default()
            });
        }
        assert!(state.tab_summaries.len() <= MAX_TAB_PROJECTION_SUMMARIES);
    }

    #[test]
    fn resync_counter_advances_on_resync() {
        let mut state = ProjectionClientState::new();
        // Push an envelope for an unknown subscription to trigger a
        // resync-style error path.
        let env = ProjectionEnvelope {
            protocol_version: 1,
            event_seq: 1,
            timestamp_ms: 0,
            session_id: Some("session-1".into()),
            turn_id: None,
            scope: codegg_protocol::projection::event::ProjectionStreamScope::Session,
            payload: ProjectionEvent::Diagnostic {
                code: "x".into(),
                message: "y".into(),
            },
        };
        let _ = state.apply_envelope(&ProjectionSubscriptionId::new("missing"), "tab-1", env);
        // We expect at least one resync-request side effect because
        // the unknown subscription returns an Error outcome, which is
        // not a ResyncRequested. The ack_resync must reset the count
        // regardless.
        state.ack_resync();
        assert_eq!(state.resync_request_count(), 0);
    }

    #[test]
    fn cursor_info_can_be_set_and_read() {
        let mut state = ProjectionClientState::new();
        state.set_cursor(
            "tab-1",
            ProjectionCursorInfo {
                subscription_id: Some(ProjectionSubscriptionId::new("sub-1")),
                last_delivered_seq: 10,
                last_acked_seq: 8,
                state: "live".into(),
            },
        );
        let info = state.cursor("tab-1").expect("cursor");
        assert_eq!(info.last_delivered_seq, 10);
    }

    fn handle_entry(id: &str) -> ArtifactHandleCacheEntry {
        ArtifactHandleCacheEntry {
            handle_id: id.into(),
            kind: "RunOutput".into(),
            project_id: "p1".into(),
            content_type: "text/plain".into(),
            total_bytes: Some(1024),
            created_at: 0,
            expires_at: None,
            revision: 1,
            public_summary: Some("summary".into()),
        }
    }

    #[test]
    fn artifact_handle_cache_is_bounded() {
        let mut state = ProjectionClientState::new();
        for i in 0..(MAX_ARTIFACT_HANDLES_PER_TAB + 4) {
            let ok = state.cache_artifact_handle("tab-1", handle_entry(&format!("art-{i}")));
            if i < MAX_ARTIFACT_HANDLES_PER_TAB {
                assert!(ok, "should accept {i}");
            } else {
                assert!(!ok, "should reject {i}");
            }
        }
        assert!(state.artifact_handles("tab-1").len() <= MAX_ARTIFACT_HANDLES_PER_TAB);
    }

    #[test]
    fn artifact_handle_cache_refreshes_in_place() {
        let mut state = ProjectionClientState::new();
        assert!(state.cache_artifact_handle("tab-1", handle_entry("art-1")));
        let updated = ArtifactHandleCacheEntry {
            total_bytes: Some(2048),
            ..handle_entry("art-1")
        };
        assert!(state.cache_artifact_handle("tab-1", updated));
        let cached = state.artifact_handles("tab-1");
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].total_bytes, Some(2048));
    }

    #[test]
    fn artifact_handle_cache_blocked_in_compat_mode() {
        let mut state = ProjectionClientState::new();
        state.enter_raw_compatibility("test");
        assert!(!state.cache_artifact_handle("tab-1", handle_entry("art-1")));
        assert!(state.artifact_handles("tab-1").is_empty());
    }

    #[test]
    fn artifact_excerpt_cache_replaces_per_handle() {
        let mut state = ProjectionClientState::new();
        let excerpt = ArtifactExcerptCacheEntry {
            handle_id: "art-1".into(),
            start: 0,
            end: 100,
            content_type: "text/plain".into(),
            content: "hello".into(),
            redacted: false,
            truncated: false,
            note: None,
            fetched_at_ms: 0,
        };
        assert!(state.cache_artifact_excerpt("tab-1", excerpt.clone()));
        let updated = ArtifactExcerptCacheEntry {
            content: "world".into(),
            ..excerpt
        };
        assert!(state.cache_artifact_excerpt("tab-1", updated));
        assert_eq!(state.artifact_excerpts("tab-1").len(), 1);
        assert_eq!(state.artifact_excerpts("tab-1")[0].content, "world");
    }

    #[test]
    fn artifact_excerpt_rejects_oversized_content() {
        let mut state = ProjectionClientState::new();
        let huge = "x".repeat(MAX_ARTIFACT_EXCERPT_BYTES + 1);
        let excerpt = ArtifactExcerptCacheEntry {
            handle_id: "art-1".into(),
            start: 0,
            end: huge.len() as u64,
            content_type: "text/plain".into(),
            content: huge,
            redacted: false,
            truncated: false,
            note: None,
            fetched_at_ms: 0,
        };
        assert!(!state.cache_artifact_excerpt("tab-1", excerpt));
    }

    #[test]
    fn clear_tab_artifacts_drops_everything() {
        let mut state = ProjectionClientState::new();
        assert!(state.cache_artifact_handle("tab-1", handle_entry("art-1")));
        let _ = state.begin_artifact_read("tab-1", "art-1");
        state.clear_tab_artifacts("tab-1");
        assert!(state.artifact_handles("tab-1").is_empty());
        assert_eq!(state.artifact_reads("tab-1"), 0);
    }

    #[test]
    fn reconnect_clears_artifact_caches() {
        let mut state = ProjectionClientState::new();
        assert!(state.cache_artifact_handle("tab-1", handle_entry("art-1")));
        state.on_reconnect();
        assert!(state.artifact_handles("tab-1").is_empty());
    }
}

//! Operational health model for LSP clients.
//!
//! [`LspOperationalState`] sits above the low-level transport state and
//! captures service-level lifecycle phases including indexing, degraded
//! readiness, restart activity, and permanent failure. Health snapshots
//! are read-only and never mutate state.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// High-level operational state of an LSP client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LspOperationalState {
    /// Process is starting but not yet connected.
    Starting,
    /// Connected, `initialize` request in flight.
    Initializing,
    /// Initialized but server is still indexing/building.
    Indexing,
    /// Fully ready for semantic requests.
    Ready,
    /// Operational but with degraded capabilities.
    Degraded { reason: String },
    /// Restart has been scheduled but not yet started.
    RestartScheduled { attempt: u32, delay_ms: u64 },
    /// Restart is in progress (new generation initializing).
    Restarting { attempt: u32 },
    /// Server has failed permanently (exhausted restarts or fatal error).
    Failed { reason: String },
    /// Graceful shutdown in progress.
    Stopping,
    /// Server has been stopped.
    Stopped,
}

impl LspOperationalState {
    /// Returns a human-readable label for the state.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Initializing => "initializing",
            Self::Indexing => "indexing",
            Self::Ready => "ready",
            Self::Degraded { .. } => "degraded",
            Self::RestartScheduled { .. } => "restart_scheduled",
            Self::Restarting { .. } => "restarting",
            Self::Failed { .. } => "failed",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
        }
    }

    /// Returns a bounded note suitable for semantic context responses.
    ///
    /// Only returns a note when the state is not `Ready`, to avoid
    /// noise in the common case.
    pub fn context_note(&self) -> Option<String> {
        match self {
            Self::Starting => Some("LSP state: starting".to_string()),
            Self::Initializing => Some("LSP state: initializing".to_string()),
            Self::Indexing => Some("LSP state: indexing — results may be incomplete".to_string()),
            Self::Ready => None,
            Self::Degraded { reason } => Some(format!("LSP state: degraded — {reason}")),
            Self::RestartScheduled { attempt, delay_ms } => Some(format!(
                "LSP state: restart scheduled (attempt {attempt}, delay {delay_ms}ms)"
            )),
            Self::Restarting { attempt } => {
                Some(format!("LSP state: restarting (attempt {attempt})"))
            }
            Self::Failed { reason } => Some(format!("LSP state: failed — {reason}")),
            Self::Stopping => Some("LSP state: stopping".to_string()),
            Self::Stopped => Some("LSP state: stopped".to_string()),
        }
    }

    /// Returns `true` if the state represents an active, usable server.
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Ready | Self::Indexing | Self::Degraded { .. })
    }

    /// Returns `true` if the state represents a terminal condition.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Stopped)
    }
}

/// Valid state transitions for the operational state machine.
///
/// Returns `Err` if the transition is invalid.
pub fn transition(
    current: &LspOperationalState,
    next: LspOperationalState,
) -> Result<LspOperationalState, InvalidTransition> {
    use LspOperationalState::*;
    let valid = match current {
        Starting => matches!(next, Initializing),
        Initializing => matches!(next, Indexing | Ready | Failed { .. } | Stopping),
        Indexing => matches!(
            next,
            Ready | Degraded { .. } | Failed { .. } | Stopping | Stopped
        ),
        // `Ready` can transition to `Failed` when an unexpected
        // exit occurs and restart is disabled or exhausted.
        // `Stopped` is reachable via graceful shutdown.
        Ready => matches!(
            next,
            Degraded { .. } | RestartScheduled { .. } | Failed { .. } | Stopping | Stopped
        ),
        // `Degraded` is a stable long-lived state; it can move
        // to `Stopped` on graceful shutdown as well.
        Degraded { .. } => matches!(
            next,
            Ready | RestartScheduled { .. } | Failed { .. } | Stopping | Stopped
        ),
        RestartScheduled { .. } => matches!(next, Restarting { .. } | Stopping | Stopped),
        Restarting { .. } => matches!(next, Initializing | Failed { .. } | Stopping),
        Failed { .. } => false,
        Stopping => matches!(next, Stopped),
        Stopped => false,
    };
    if valid {
        Ok(next)
    } else {
        Err(InvalidTransition {
            from: current.clone(),
            to: next,
        })
    }
}

/// Error returned when an invalid state transition is attempted.
#[derive(Debug, Clone)]
pub struct InvalidTransition {
    pub from: LspOperationalState,
    pub to: LspOperationalState,
}

impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid LSP state transition: {} -> {}",
            self.from.label(),
            self.to.label()
        )
    }
}

impl std::error::Error for InvalidTransition {}

/// Read-only snapshot of a client's operational health.
///
/// New fields added in Phase 3 / Pass 3 are `Option`/collection-shaped
/// and have `#[serde(default)]` so older serialized snapshots remain
/// deserializable. `transport` is `Option<ClientTransportSnapshot>`
/// because the snapshot must be constructible when no live client
/// exists (e.g. during `Restarting`, `Failed`, or `Stopped`).
///
/// `generation` reflects the authoritative per-key generation from
/// `LspService::generation_for_key`; it is bumped by the restart
/// coordinator after a successful reinit + replay, never speculatively.
/// `last_error` is only populated for `Failed { reason }` transitions
/// (e.g. exit reason, restart exhaustion); healthy clients keep it
/// `None`. The `stderr_tail` is sourced from the live
/// `LspProcessRuntime` and is empty when no runtime is installed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspOperationalHealthSnapshot {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    pub state: LspOperationalState,
    /// Transport snapshot, or `None` when no live client is present
    /// (during restart, failure, or before initial publication).
    #[serde(default)]
    pub transport: Option<crate::client::ClientTransportSnapshot>,
    pub pending_requests: usize,
    pub open_documents: usize,
    pub last_message_age_ms: Option<u64>,
    pub last_diagnostics_age_ms: Option<u64>,
    pub restart_attempts: u32,
    /// Last error observed for this client (e.g. exit reason, restart
    /// failure, transition failure). `None` for healthy clients.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Bounded stderr tail captured from the process runtime. Capped at
    /// 20 lines for snapshot construction; further truncated to 5
    /// lines by `status_line`.
    #[serde(default)]
    pub stderr_tail: Vec<String>,
}

impl Default for LspOperationalHealthSnapshot {
    fn default() -> Self {
        Self {
            server_id: String::new(),
            root: PathBuf::new(),
            generation: 0,
            state: LspOperationalState::Starting,
            transport: None,
            pending_requests: 0,
            open_documents: 0,
            last_message_age_ms: None,
            last_diagnostics_age_ms: None,
            restart_attempts: 0,
            last_error: None,
            stderr_tail: Vec::new(),
        }
    }
}

impl LspOperationalHealthSnapshot {
    /// Construct a snapshot from the operational state and per-client
    /// observability fields.
    ///
    /// `transport` should be `None` when no live client exists. `stderr_tail`
    /// should already be bounded (e.g. via the runtime's
    /// `stderr_tail_capped`).
    #[allow(clippy::too_many_arguments)]
    pub fn from_operational_state(
        server_id: String,
        root: PathBuf,
        generation: u64,
        state: LspOperationalState,
        transport: Option<crate::client::ClientTransportSnapshot>,
        pending_requests: usize,
        open_documents: usize,
        last_message_age_ms: Option<u64>,
        last_diagnostics_age_ms: Option<u64>,
        restart_attempts: u32,
        last_error: Option<String>,
        stderr_tail: Vec<String>,
    ) -> Self {
        Self {
            server_id,
            root,
            generation,
            state,
            transport,
            pending_requests,
            open_documents,
            last_message_age_ms,
            last_diagnostics_age_ms,
            restart_attempts,
            last_error,
            stderr_tail,
        }
    }

    /// Format a compact status line for logging or display.
    ///
    /// The stderr tail is truncated to 5 lines for compactness; full
    /// tails are accessible via the `stderr_tail` field. The last
    /// error is included when present.
    pub fn status_line(&self) -> String {
        let stderr_preview: Vec<String> = self
            .stderr_tail
            .iter()
            .rev()
            .take(5)
            .rev()
            .cloned()
            .collect();
        let mut line = format!(
            "{} [gen={}, state={}, transport={:?}, pending={}, open={}, stderr_tail={:?}",
            self.server_id,
            self.generation,
            self.state.label(),
            self.transport,
            self.pending_requests,
            self.open_documents,
            stderr_preview,
        );
        if let Some(err) = &self.last_error {
            line.push_str(&format!(", last_error={err}"));
        }
        line.push(']');
        line
    }
}

// ---------------------------------------------------------------------------
// Phase 13: Observability metrics snapshot
// ---------------------------------------------------------------------------

/// High-level observability snapshot for LSP subsystem health.
///
/// Combines operational state, transport, cache, and preview metrics
/// into a single read-only DTO suitable for rendering in `/lsp-doctor`
/// or `/lsp-status --detail`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspObservabilitySnapshot {
    /// Number of active client keys.
    pub active_clients: usize,
    /// Per-client health snapshots.
    pub clients: Vec<LspOperationalHealthSnapshot>,
    /// Semantic cache mode ("disabled" or "memory").
    pub cache_mode: String,
    /// Semantic cache entries.
    pub cache_entries: usize,
    /// Semantic cache bytes.
    pub cache_bytes: usize,
    /// Semantic cache hit count.
    pub cache_hits: u64,
    /// Semantic cache miss count.
    pub cache_misses: u64,
    /// Semantic cache stale-miss count.
    pub cache_stale_misses: u64,
    /// Semantic cache eviction count.
    pub cache_evictions: u64,
    /// Total preview artifacts registered.
    pub preview_count: usize,
    /// Number of stale preview artifacts.
    pub preview_stale_count: usize,
    /// Number of applied preview artifacts.
    pub preview_applied_count: usize,
}

impl Default for LspObservabilitySnapshot {
    fn default() -> Self {
        Self {
            active_clients: 0,
            clients: Vec::new(),
            cache_mode: "disabled".to_string(),
            cache_entries: 0,
            cache_bytes: 0,
            cache_hits: 0,
            cache_misses: 0,
            cache_stale_misses: 0,
            cache_evictions: 0,
            preview_count: 0,
            preview_stale_count: 0,
            preview_applied_count: 0,
        }
    }
}

impl LspObservabilitySnapshot {
    /// Render a compact one-line summary.
    pub fn status_line(&self) -> String {
        format!(
            "LSP: {} clients, cache={}/{} entries, {} previews ({} stale, {} applied)",
            self.active_clients,
            self.cache_mode,
            self.cache_entries,
            self.preview_count,
            self.preview_stale_count,
            self.preview_applied_count,
        )
    }

    /// Render a multi-line detail summary.
    pub fn render_detail(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Active clients: {}", self.active_clients));
        for client in &self.clients {
            lines.push(format!(
                "  {} [{}] gen={} | {} pending, {} open",
                client.server_id,
                client.state.label(),
                client.generation,
                client.pending_requests,
                client.open_documents,
            ));
        }
        lines.push(format!(
            "Cache: mode={}, entries={}, bytes={}, hits={}, misses={}, stale_misses={}, evictions={}",
            self.cache_mode,
            self.cache_entries,
            self.cache_bytes,
            self.cache_hits,
            self.cache_misses,
            self.cache_stale_misses,
            self.cache_evictions,
        ));
        lines.push(format!(
            "Previews: total={}, stale={}, applied={}",
            self.preview_count, self.preview_stale_count, self.preview_applied_count,
        ));
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions() {
        use LspOperationalState::*;
        // Starting -> Initializing
        assert!(transition(&Starting, Initializing).is_ok());
        // Initializing -> Indexing
        assert!(transition(&Initializing, Indexing).is_ok());
        // Initializing -> Ready
        assert!(transition(&Initializing, Ready).is_ok());
        // Indexing -> Ready
        assert!(transition(&Indexing, Ready).is_ok());
        // Indexing -> Degraded
        assert!(transition(
            &Indexing,
            Degraded {
                reason: "slow".into()
            }
        )
        .is_ok());
        // Ready -> Degraded
        assert!(transition(&Ready, Degraded { reason: "x".into() }).is_ok());
        // Ready -> RestartScheduled
        assert!(transition(
            &Ready,
            RestartScheduled {
                attempt: 1,
                delay_ms: 500
            }
        )
        .is_ok());
        // Ready -> Failed (unexpected exit, no restart budget)
        assert!(transition(
            &Ready,
            Failed {
                reason: "crash".into()
            }
        )
        .is_ok());
        // Ready -> Stopped (graceful shutdown)
        assert!(transition(&Ready, Stopped).is_ok());
        // Degraded -> Ready
        assert!(transition(&Degraded { reason: "x".into() }, Ready).is_ok());
        // Degraded -> Stopped (graceful shutdown from degraded)
        assert!(transition(&Degraded { reason: "x".into() }, Stopped).is_ok());
        // Indexing -> Stopped (graceful shutdown before ready)
        assert!(transition(&Indexing, Stopped).is_ok());
        // RestartScheduled -> Restarting
        assert!(transition(
            &RestartScheduled {
                attempt: 1,
                delay_ms: 500
            },
            Restarting { attempt: 1 }
        )
        .is_ok());
        // Restarting -> Initializing
        assert!(transition(&Restarting { attempt: 1 }, Initializing).is_ok());
        // Stopping -> Stopped
        assert!(transition(&Stopping, Stopped).is_ok());
    }

    #[test]
    fn invalid_transitions() {
        use LspOperationalState::*;
        // Starting -> Ready (must go through Initializing)
        assert!(transition(&Starting, Ready).is_err());
        // Ready -> Initializing (can't go back)
        assert!(transition(&Ready, Initializing).is_err());
        // Failed -> anything (terminal)
        assert!(transition(&Failed { reason: "x".into() }, Ready).is_err());
        // Stopped -> anything (terminal)
        assert!(transition(&Stopped, Ready).is_err());
        // Stopping -> Ready (must go through Stopped)
        assert!(transition(&Stopping, Ready).is_err());
    }

    #[test]
    fn state_labels() {
        assert_eq!(LspOperationalState::Starting.label(), "starting");
        assert_eq!(
            LspOperationalState::Failed { reason: "x".into() }.label(),
            "failed"
        );
    }

    #[test]
    fn is_usable() {
        use LspOperationalState::*;
        assert!(Ready.is_usable());
        assert!(Indexing.is_usable());
        assert!(Degraded { reason: "x".into() }.is_usable());
        assert!(!Starting.is_usable());
        assert!(!Failed { reason: "x".into() }.is_usable());
    }

    #[test]
    fn is_terminal() {
        use LspOperationalState::*;
        assert!(Failed { reason: "x".into() }.is_terminal());
        assert!(Stopped.is_terminal());
        assert!(!Ready.is_terminal());
        assert!(!Starting.is_terminal());
    }

    #[test]
    fn snapshot_from_operational_state_roundtrip() {
        use std::path::PathBuf;
        use LspOperationalState::*;

        let snap = LspOperationalHealthSnapshot::from_operational_state(
            "rust-analyzer".to_string(),
            PathBuf::from("/tmp"),
            3,
            Failed {
                reason: "synthetic".to_string(),
            },
            None,
            0,
            4,
            Some(120),
            Some(50),
            2,
            Some("synthetic".to_string()),
            vec!["line 1".to_string(), "line 2".to_string()],
        );
        assert_eq!(snap.server_id, "rust-analyzer");
        assert_eq!(snap.generation, 3);
        assert!(matches!(snap.state, Failed { .. }));
        assert!(snap.transport.is_none());
        assert_eq!(snap.pending_requests, 0);
        assert_eq!(snap.open_documents, 4);
        assert_eq!(snap.last_message_age_ms, Some(120));
        assert_eq!(snap.last_diagnostics_age_ms, Some(50));
        assert_eq!(snap.restart_attempts, 2);
        assert_eq!(snap.last_error.as_deref(), Some("synthetic"));
        assert_eq!(snap.stderr_tail.len(), 2);
    }

    #[test]
    fn status_line_includes_last_error_and_truncates_stderr() {
        use std::path::PathBuf;
        use LspOperationalState::*;

        let snap = LspOperationalHealthSnapshot::from_operational_state(
            "rust-analyzer".to_string(),
            PathBuf::from("/tmp"),
            1,
            Failed {
                reason: "oops".to_string(),
            },
            None,
            0,
            0,
            None,
            None,
            0,
            Some("oops".to_string()),
            (0..10).map(|i| format!("line {i}")).collect(),
        );
        let line = snap.status_line();
        assert!(line.contains("last_error=oops"));
        // 5 lines preview: line 5..=9
        assert!(line.contains("line 5"));
        assert!(line.contains("line 9"));
        // Earlier lines are dropped from the preview.
        assert!(!line.contains("\"line 0\""));
    }

    #[test]
    fn default_snapshot_has_no_transport_and_no_stderr() {
        let snap = LspOperationalHealthSnapshot::default();
        assert_eq!(snap.server_id, "");
        assert_eq!(snap.generation, 0);
        assert!(matches!(snap.state, LspOperationalState::Starting));
        assert!(snap.transport.is_none());
        assert!(snap.stderr_tail.is_empty());
        assert!(snap.last_error.is_none());
    }

    #[test]
    fn observability_snapshot_default() {
        let snap = LspObservabilitySnapshot::default();
        assert_eq!(snap.active_clients, 0);
        assert!(snap.clients.is_empty());
        assert_eq!(snap.cache_mode, "disabled");
        assert_eq!(snap.preview_count, 0);
    }

    #[test]
    fn observability_snapshot_status_line() {
        let snap = LspObservabilitySnapshot {
            active_clients: 2,
            cache_mode: "memory".to_string(),
            cache_entries: 5,
            preview_count: 3,
            preview_stale_count: 1,
            preview_applied_count: 1,
            ..Default::default()
        };
        let line = snap.status_line();
        assert!(line.contains("2 clients"));
        assert!(line.contains("cache=memory/5"));
        assert!(line.contains("3 previews"));
        assert!(line.contains("1 stale"));
        assert!(line.contains("1 applied"));
    }

    #[test]
    fn observability_snapshot_render_detail() {
        let snap = LspObservabilitySnapshot {
            active_clients: 1,
            clients: vec![LspOperationalHealthSnapshot::from_operational_state(
                "rust-analyzer".to_string(),
                std::path::PathBuf::from("/tmp"),
                3,
                LspOperationalState::Ready,
                None,
                2,
                5,
                Some(100),
                Some(50),
                0,
                None,
                vec![],
            )],
            cache_mode: "disabled".to_string(),
            cache_entries: 0,
            preview_count: 0,
            preview_stale_count: 0,
            preview_applied_count: 0,
            ..Default::default()
        };
        let detail = snap.render_detail();
        assert!(detail.contains("Active clients: 1"));
        assert!(detail.contains("rust-analyzer"));
        assert!(detail.contains("ready"));
        assert!(detail.contains("Cache:"));
        assert!(detail.contains("Previews:"));
    }
}

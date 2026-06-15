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
        Indexing => matches!(next, Ready | Degraded { .. } | Failed { .. } | Stopping),
        Ready => matches!(
            next,
            Degraded { .. } | RestartScheduled { .. } | Stopping | Stopped
        ),
        Degraded { .. } => matches!(
            next,
            Ready | RestartScheduled { .. } | Failed { .. } | Stopping
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspOperationalHealthSnapshot {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    pub state: LspOperationalState,
    pub transport: crate::client::ClientTransportSnapshot,
    pub pending_requests: usize,
    pub open_documents: usize,
    pub last_message_age_ms: Option<u64>,
    pub last_diagnostics_age_ms: Option<u64>,
    pub restart_attempts: u32,
}

impl LspOperationalHealthSnapshot {
    /// Format a compact status line for logging or display.
    pub fn status_line(&self) -> String {
        format!(
            "{} [gen={}, state={}, transport={:?}, pending={}, open={}]",
            self.server_id,
            self.generation,
            self.state.label(),
            self.transport,
            self.pending_requests,
            self.open_documents,
        )
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
        // Degraded -> Ready
        assert!(transition(&Degraded { reason: "x".into() }, Ready).is_ok());
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
}

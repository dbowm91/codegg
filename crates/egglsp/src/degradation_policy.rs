//! Degradation and fallback policy for LSP context collection.
//!
//! Evaluates whether a context request should be skipped, partially
//! collected, fully collected, or rejected based on the configured
//! [`LspContextMode`], server availability, and capability support.

use crate::context::LspContextMode;

// ---------------------------------------------------------------------------
// Policy config
// ---------------------------------------------------------------------------

/// Configuration for degradation behavior.
#[derive(Debug, Clone)]
pub struct LspContextDegradePolicy {
    /// The context mode controlling collection behavior.
    pub mode: LspContextMode,
    /// Prefix for degradation note messages.
    pub fallback_note_prefix: String,
    /// Whether partial results are permitted when degraded.
    pub allow_partial_results: bool,
}

impl Default for LspContextDegradePolicy {
    fn default() -> Self {
        Self {
            mode: LspContextMode::default(),
            fallback_note_prefix: "LSP degraded: ".to_string(),
            allow_partial_results: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Decision
// ---------------------------------------------------------------------------

/// The decision returned by [`evaluate_degradation`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LspContextDegradeDecision {
    /// Context collection should be skipped entirely.
    Skip { reason: String },
    /// Partial results should be collected (some capabilities unavailable).
    Partial { notes: Vec<String> },
    /// Full context collection should proceed.
    FullCollect,
    /// Context collection is required but cannot proceed.
    Fail { reason: String },
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Evaluate the degradation decision for a context request.
///
/// Rules:
/// - `Disabled` → `Skip`
/// - `Opportunistic` + server unavailable → `Partial`
/// - `Opportunistic` + capability unsupported → `Partial`
/// - `Opportunistic` + server available + capability supported → `FullCollect`
/// - `Required` + server unavailable → `Fail`
/// - `Required` + capability unsupported → `Fail`
/// - `Required` + server available + capability supported → `FullCollect`
pub fn evaluate_degradation(
    mode: &LspContextMode,
    server_available: bool,
    capability_supported: bool,
) -> LspContextDegradeDecision {
    match mode {
        LspContextMode::Disabled => LspContextDegradeDecision::Skip {
            reason: "LSP context collection is disabled".to_string(),
        },
        LspContextMode::Opportunistic => {
            if !server_available {
                LspContextDegradeDecision::Partial {
                    notes: vec!["LSP server unavailable, partial results only".to_string()],
                }
            } else if !capability_supported {
                LspContextDegradeDecision::Partial {
                    notes: vec!["LSP capability not supported, partial results only".to_string()],
                }
            } else {
                LspContextDegradeDecision::FullCollect
            }
        }
        LspContextMode::Required => {
            if !server_available {
                LspContextDegradeDecision::Fail {
                    reason: "LSP server unavailable in Required mode".to_string(),
                }
            } else if !capability_supported {
                LspContextDegradeDecision::Fail {
                    reason: "LSP capability not supported in Required mode".to_string(),
                }
            } else {
                LspContextDegradeDecision::FullCollect
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_skips() {
        let decision = evaluate_degradation(&LspContextMode::Disabled, true, true);
        assert_eq!(
            decision,
            LspContextDegradeDecision::Skip {
                reason: "LSP context collection is disabled".to_string()
            }
        );
    }

    #[test]
    fn test_opportunistic_partial_when_unavailable() {
        let decision = evaluate_degradation(&LspContextMode::Opportunistic, false, true);
        match decision {
            LspContextDegradeDecision::Partial { notes } => {
                assert!(!notes.is_empty());
                assert!(notes[0].contains("unavailable"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn test_opportunistic_full_when_available() {
        let decision = evaluate_degradation(&LspContextMode::Opportunistic, true, true);
        assert_eq!(decision, LspContextDegradeDecision::FullCollect);
    }

    #[test]
    fn test_required_fails_when_unavailable() {
        let decision = evaluate_degradation(&LspContextMode::Required, false, true);
        match decision {
            LspContextDegradeDecision::Fail { reason } => {
                assert!(reason.contains("unavailable"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn test_required_fails_when_unsupported() {
        let decision = evaluate_degradation(&LspContextMode::Required, true, false);
        match decision {
            LspContextDegradeDecision::Fail { reason } => {
                assert!(reason.contains("not supported"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn test_opportunistic_partial_when_unsupported() {
        let decision = evaluate_degradation(&LspContextMode::Opportunistic, true, false);
        match decision {
            LspContextDegradeDecision::Partial { notes } => {
                assert!(!notes.is_empty());
                assert!(notes[0].contains("not supported"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn test_required_full_when_available_and_supported() {
        let decision = evaluate_degradation(&LspContextMode::Required, true, true);
        assert_eq!(decision, LspContextDegradeDecision::FullCollect);
    }

    #[test]
    fn test_default_policy() {
        let policy = LspContextDegradePolicy::default();
        assert_eq!(policy.mode, LspContextMode::Opportunistic);
        assert!(policy.allow_partial_results);
        assert!(policy.fallback_note_prefix.contains("degraded"));
    }
}

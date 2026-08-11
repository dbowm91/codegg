//! Bounded, observable progress detection and graduated tool recovery.
//!
//! This module deliberately stores fingerprints and small classifications, never
//! tool arguments, output, or model reasoning.  It is usable by the loop and by
//! model adapters without coupling either side to provider wire formats.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

pub const DEFAULT_HISTORY_LIMIT: usize = 32;
pub const DEFAULT_MAX_RECOVERIES: u8 = 4;
const MAX_SUMMARY_BYTES: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    StructuredCall,
    MalformedCall,
    NarrationOnly,
    FinalAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSizeClass {
    Empty,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressSignal {
    None,
    NewEvidence,
    StateChanged,
    ChildAdvanced,
    DifferentResult,
    DifferentTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentKind {
    ExactRepeat,
    EquivalentResult,
    EquivalentError,
    ShortCycle,
    MalformedCall,
    NarrationWithoutAction,
    UnavailableTool,
    NoProgress,
    DelegationRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Nudge,
    Correct,
    RestoreBasePalette,
    Replan,
    Stall,
}

/// The normalized status of a tool execution as seen by recovery policy.
/// Rendered tool text remains the model-facing contract; this compact status
/// prevents recovery from having to infer authority and cancellation from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionStatus {
    Success,
    Denied,
    Timeout,
    Cancelled,
    ToolError,
    ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionOutcome {
    pub status: ToolExecutionStatus,
    pub model_text: String,
}

impl ToolExecutionOutcome {
    pub fn success(model_text: impl Into<String>) -> Self {
        Self {
            status: ToolExecutionStatus::Success,
            model_text: model_text.into(),
        }
    }

    pub fn from_tool_error(error: crate::error::ToolError) -> Self {
        let status = match error {
            crate::error::ToolError::Timeout(_) => ToolExecutionStatus::Timeout,
            crate::error::ToolError::Permission(_) => ToolExecutionStatus::Denied,
            crate::error::ToolError::Format(_) => ToolExecutionStatus::ProtocolError,
            crate::error::ToolError::NotFound(_)
            | crate::error::ToolError::Execution(_)
            | crate::error::ToolError::Disabled(_)
            | crate::error::ToolError::Io(_)
            | crate::error::ToolError::Network(_) => ToolExecutionStatus::ToolError,
        };
        Self {
            status,
            model_text: format!("Error: {error}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyPhase {
    Normal,
    AdapterRepair,
    ContinueOrReplan,
    Stall,
}

/// Turn-local owner of autonomous recovery transitions.  Provider transport
/// retries intentionally remain outside this type.
#[derive(Debug, Clone)]
pub struct AutonomyState {
    recovery: RecoveryController,
    phase: AutonomyPhase,
    transitions: u8,
    adapter_repairs: u8,
    post_tool_continuations: u8,
}

impl Default for AutonomyState {
    fn default() -> Self {
        Self {
            recovery: RecoveryController::default(),
            phase: AutonomyPhase::Normal,
            transitions: 0,
            adapter_repairs: 0,
            post_tool_continuations: 0,
        }
    }
}

impl AutonomyState {
    pub const MAX_TRANSITIONS: u8 = 4;
    pub const MAX_POST_TOOL_CONTINUATIONS: u8 = 1;

    pub fn phase(&self) -> AutonomyPhase {
        self.phase
    }
    pub fn reset_after_progress(&mut self) {
        self.phase = AutonomyPhase::Normal;
        self.transitions = 0;
        self.post_tool_continuations = 0;
    }
    pub fn adapter_repair_allowed(&mut self) -> bool {
        if self.adapter_repairs >= 1 {
            return false;
        }
        self.adapter_repairs += 1;
        self.phase = AutonomyPhase::AdapterRepair;
        true
    }
    pub fn continuation_allowed(&mut self) -> bool {
        if self.post_tool_continuations >= Self::MAX_POST_TOOL_CONTINUATIONS
            || self.transitions >= Self::MAX_TRANSITIONS
        {
            return false;
        }
        self.post_tool_continuations += 1;
        self.transitions += 1;
        self.phase = AutonomyPhase::ContinueOrReplan;
        true
    }
    pub fn observe_tool(&mut self, observation: ProgressObservation) -> RecoveryDecision {
        let decision = self.recovery.observe(observation);
        match decision {
            RecoveryDecision::Progress => self.reset_after_progress(),
            RecoveryDecision::Recover { .. } => {
                self.transitions = self.transitions.saturating_add(1);
                self.phase = AutonomyPhase::ContinueOrReplan;
                if self.transitions > Self::MAX_TRANSITIONS {
                    self.phase = AutonomyPhase::Stall;
                    return RecoveryDecision::Stalled(StalledReport {
                        last_progress: ProgressSignal::None,
                        incident: IncidentKind::NoProgress,
                        attempted_recoveries: self.transitions,
                        evidence: "autonomy transition bound exceeded".into(),
                        suggested_user_action: "Inspect the last observable tool result and provide a narrower next step.".into(),
                    });
                }
            }
            RecoveryDecision::Stalled(_) => self.phase = AutonomyPhase::Stall,
            RecoveryDecision::Continue => {}
        }
        decision
    }

    pub fn observe_tool_result(
        &mut self,
        outcome: &ToolExecutionOutcome,
        mut observation: ProgressObservation,
    ) -> RecoveryDecision {
        if matches!(outcome.status, ToolExecutionStatus::Denied) {
            observation.error_class = Some("denied".into());
        }
        self.observe_tool(observation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressObservation {
    pub action: ActionClass,
    pub canonical_tool: Option<String>,
    pub wire_tool: Option<String>,
    pub argument_fingerprint: Option<String>,
    pub result_fingerprint: Option<String>,
    pub result_size: ResultSizeClass,
    pub error_class: Option<String>,
    pub new_evidence: bool,
    pub state_changed: bool,
    pub child_advanced: bool,
    pub selected_surface_fingerprint: Option<String>,
    pub batch_id: u64,
}

impl ProgressObservation {
    pub fn tool(
        canonical_tool: impl Into<String>,
        wire_tool: impl Into<String>,
        arguments: &serde_json::Value,
        result: &str,
        batch_id: u64,
    ) -> Self {
        let canonical_tool = canonical_tool.into();
        // Keep argument and result fingerprints separate; the tuple above only
        // prevents accidental dependence on unstable JSON map ordering.
        Self {
            action: ActionClass::StructuredCall,
            canonical_tool: Some(canonical_tool),
            wire_tool: Some(wire_tool.into()),
            argument_fingerprint: Some(fingerprint(&normalize_json(arguments)).1),
            result_fingerprint: Some(fingerprint(&result).1),
            result_size: result_size_class(result),
            error_class: classify_error(result),
            new_evidence: false,
            state_changed: false,
            child_advanced: false,
            selected_surface_fingerprint: None,
            batch_id,
        }
    }

    pub fn action_key(&self) -> String {
        format!(
            "{:?}:{}",
            self.canonical_tool,
            self.argument_fingerprint.as_deref().unwrap_or("")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryIncident {
    pub kind: IncidentKind,
    pub occurrences: u8,
    pub last_action: RecoveryAction,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StalledReport {
    pub last_progress: ProgressSignal,
    pub incident: IncidentKind,
    pub attempted_recoveries: u8,
    pub evidence: String,
    pub suggested_user_action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDecision {
    Progress,
    Continue,
    Recover {
        action: RecoveryAction,
        incident: RecoveryIncident,
    },
    Stalled(StalledReport),
}

#[derive(Debug, Clone)]
pub struct RecoveryController {
    history: VecDeque<ProgressObservation>,
    incident: Option<RecoveryIncident>,
    recoveries: u8,
    next_batch: u64,
    history_limit: usize,
    max_recoveries: u8,
}

impl Default for RecoveryController {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY_LIMIT, DEFAULT_MAX_RECOVERIES)
    }
}

impl RecoveryController {
    pub fn new(history_limit: usize, max_recoveries: u8) -> Self {
        Self {
            history: VecDeque::with_capacity(history_limit.clamp(1, 128)),
            incident: None,
            recoveries: 0,
            next_batch: 0,
            history_limit: history_limit.clamp(1, 128),
            max_recoveries: max_recoveries.clamp(1, 8),
        }
    }

    pub fn next_batch(&mut self) -> u64 {
        let id = self.next_batch;
        self.next_batch += 1;
        id
    }
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
    pub fn recovery_count(&self) -> u8 {
        self.recoveries
    }
    pub fn incident(&self) -> Option<&RecoveryIncident> {
        self.incident.as_ref()
    }

    pub fn observe(&mut self, observation: ProgressObservation) -> RecoveryDecision {
        let progress = if observation.new_evidence
            || observation.state_changed
            || observation.child_advanced
        {
            ProgressSignal::StateChanged
        } else if observation.error_class.is_none()
            && self.history.iter().rev().any(|o| {
                o.result_fingerprint != observation.result_fingerprint
                    && o.action_key() == observation.action_key()
            })
        {
            ProgressSignal::DifferentResult
        } else {
            ProgressSignal::None
        };

        let incident = self.detect(&observation);
        self.push(observation);
        if progress != ProgressSignal::None {
            self.incident = None;
            self.recoveries = 0;
            return RecoveryDecision::Progress;
        }
        let Some(kind) = incident else {
            return RecoveryDecision::Continue;
        };
        let occurrences = self
            .incident
            .as_ref()
            .map_or(1, |i| i.occurrences.saturating_add(1));
        let action = match self.recoveries {
            0 => RecoveryAction::Nudge,
            1 => RecoveryAction::Correct,
            2 => RecoveryAction::RestoreBasePalette,
            3 => RecoveryAction::Replan,
            _ => RecoveryAction::Stall,
        };
        let evidence = bounded_evidence(kind, &self.history);
        let current = RecoveryIncident {
            kind,
            occurrences,
            last_action: action,
            evidence: evidence.clone(),
        };
        self.incident = Some(current.clone());
        if action == RecoveryAction::Stall || self.recoveries >= self.max_recoveries {
            return RecoveryDecision::Stalled(StalledReport {
                last_progress: ProgressSignal::None,
                incident: kind,
                attempted_recoveries: self.recoveries,
                evidence,
                suggested_user_action:
                    "Inspect the last observable tool error or provide a narrower next step.".into(),
            });
        }
        self.recoveries = self.recoveries.saturating_add(1);
        RecoveryDecision::Recover {
            action,
            incident: current,
        }
    }

    fn detect(&self, current: &ProgressObservation) -> Option<IncidentKind> {
        let same_action = self
            .history
            .iter()
            .filter(|o| o.action_key() == current.action_key())
            .count();
        if current.action == ActionClass::MalformedCall {
            return (same_action >= 1).then_some(IncidentKind::MalformedCall);
        }
        if current.action == ActionClass::NarrationOnly {
            return (same_action >= 1).then_some(IncidentKind::NarrationWithoutAction);
        }
        if matches!(current.error_class.as_deref(), Some("unavailable_tool")) {
            return Some(IncidentKind::UnavailableTool);
        }
        if matches!(current.error_class.as_deref(), Some("delegation_rejected")) {
            return Some(IncidentKind::DelegationRejected);
        }
        if current.error_class.is_some()
            && self.history.iter().any(|o| {
                o.canonical_tool == current.canonical_tool && o.error_class == current.error_class
            })
        {
            return Some(IncidentKind::EquivalentError);
        }
        if same_action >= 1 {
            if self
                .history
                .iter()
                .any(|o| o.result_fingerprint == current.result_fingerprint)
            {
                return Some(IncidentKind::EquivalentResult);
            }
            return Some(IncidentKind::ExactRepeat);
        }
        let keys: Vec<&str> = self
            .history
            .iter()
            .rev()
            .take(6)
            .map(|o| o.canonical_tool.as_deref().unwrap_or(""))
            .collect();
        let current_tool = current.canonical_tool.as_deref().unwrap_or("");
        if keys.len() >= 2 && keys[1] == current_tool && keys[0] != keys[1] {
            return Some(IncidentKind::ShortCycle);
        }
        if keys.len() >= 3 && keys[2] == current_tool && keys[0] != keys[1] {
            return Some(IncidentKind::ShortCycle);
        }
        None
    }

    fn push(&mut self, observation: ProgressObservation) {
        if self.history.len() == self.history_limit {
            self.history.pop_front();
        }
        self.history.push_back(observation);
    }
}

pub fn fingerprint<T: Serialize>(value: &T) -> (String, String) {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let full = format!("sha256:{:x}", hasher.finalize());
    (full.clone(), full)
}

pub fn normalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| (k.clone(), normalize_json(v)))
            .collect(),
        serde_json::Value::Array(items) => items.iter().map(normalize_json).collect(),
        serde_json::Value::String(s) => {
            serde_json::Value::String(s.split_whitespace().collect::<Vec<_>>().join(" "))
        }
        other => other.clone(),
    }
}

pub fn result_size_class(value: &str) -> ResultSizeClass {
    match value.len() {
        0 => ResultSizeClass::Empty,
        1..=512 => ResultSizeClass::Small,
        513..=8192 => ResultSizeClass::Medium,
        _ => ResultSizeClass::Large,
    }
}
pub fn classify_error(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if !trimmed.starts_with("Error:") {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("not found")
        || lower.contains("unknown tool")
        || lower.contains("unavailable")
    {
        return Some("unavailable_tool".into());
    }
    if lower.contains("denied") || lower.contains("permission") {
        return Some("denied".into());
    }
    if lower.contains("delegation") && lower.contains("reject") {
        return Some("delegation_rejected".into());
    }
    let class = trimmed
        .split_whitespace()
        .nth(1)
        .unwrap_or("unknown")
        .chars()
        .take(48)
        .collect();
    Some(class)
}
fn bounded_evidence(kind: IncidentKind, history: &VecDeque<ProgressObservation>) -> String {
    let tool = history
        .back()
        .and_then(|o| o.canonical_tool.as_deref())
        .unwrap_or("unknown");
    format!("{:?} observed repeatedly for tool {}", kind, tool)
        .chars()
        .take(MAX_SUMMARY_BYTES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obs(tool: &str, args: &str, result: &str) -> ProgressObservation {
        ProgressObservation {
            action: ActionClass::StructuredCall,
            canonical_tool: Some(tool.into()),
            wire_tool: Some(tool.into()),
            argument_fingerprint: Some(args.into()),
            result_fingerprint: Some(result.into()),
            result_size: result_size_class(result),
            error_class: classify_error(result),
            new_evidence: false,
            state_changed: false,
            child_advanced: false,
            selected_surface_fingerprint: None,
            batch_id: 0,
        }
    }
    #[test]
    fn first_repeat_nudges_and_eventually_stalls() {
        let mut c = RecoveryController::new(8, 4);
        assert_eq!(
            c.observe(obs("read", "a", "same")),
            RecoveryDecision::Continue
        );
        assert!(matches!(
            c.observe(obs("read", "a", "same")),
            RecoveryDecision::Recover {
                action: RecoveryAction::Nudge,
                ..
            }
        ));
        for _ in 0..3 {
            let _ = c.observe(obs("read", "a", "same"));
        }
        assert!(matches!(
            c.observe(obs("read", "a", "same")),
            RecoveryDecision::Stalled(_)
        ));
    }
    #[test]
    fn changed_result_is_progress() {
        let mut c = RecoveryController::default();
        c.observe(obs("status", "a", "old"));
        assert_eq!(
            c.observe(obs("status", "a", "new")),
            RecoveryDecision::Progress
        );
    }
    #[test]
    fn cosmetic_argument_error_retry_is_detected() {
        let mut c = RecoveryController::default();
        c.observe(obs("bash", "command-a", "Error: tool failed for path a"));
        let decision = c.observe(obs("bash", "command-b", "Error: tool failed for path b"));
        assert!(matches!(
            decision,
            RecoveryDecision::Recover {
                incident: RecoveryIncident {
                    kind: IncidentKind::EquivalentError,
                    ..
                },
                ..
            }
        ));
    }
    #[test]
    fn short_cycle_is_detected() {
        let mut c = RecoveryController::default();
        c.observe(obs("read", "a", "one"));
        c.observe(obs("grep", "b", "two"));
        c.observe(obs("list", "c", "three"));
        let decision = c.observe(obs("read", "d", "four"));
        assert!(matches!(
            decision,
            RecoveryDecision::Recover {
                incident: RecoveryIncident {
                    kind: IncidentKind::ShortCycle,
                    ..
                },
                ..
            }
        ));
    }
    #[test]
    fn malformed_and_narration_are_recoverable() {
        let mut c = RecoveryController::default();
        let m = obs("", "bad", "");
        let mut m = m;
        m.action = ActionClass::MalformedCall;
        assert!(matches!(c.observe(m.clone()), RecoveryDecision::Continue));
        assert!(matches!(c.observe(m), RecoveryDecision::Recover { .. }));
    }
    #[test]
    fn ring_is_bounded() {
        let mut c = RecoveryController::new(2, 4);
        for n in 0..10 {
            c.observe(obs("read", &n.to_string(), "x"));
        }
        assert_eq!(c.history_len(), 2);
    }

    #[test]
    fn autonomy_allows_one_post_tool_continuation() {
        let mut state = AutonomyState::default();
        assert!(state.continuation_allowed());
        assert!(!state.continuation_allowed());
    }

    #[test]
    fn typed_status_is_authoritative_over_display_text() {
        let denied = ToolExecutionOutcome::from_tool_error(crate::error::ToolError::Permission(
            "permission denied".into(),
        ));
        assert_eq!(denied.status, ToolExecutionStatus::Denied);
        let timeout = ToolExecutionOutcome::from_tool_error(crate::error::ToolError::Timeout(
            "command timed out".into(),
        ));
        assert_eq!(timeout.status, ToolExecutionStatus::Timeout);
        let misleading = ToolExecutionOutcome::success("permission denied; timeout cancelled");
        assert_eq!(misleading.status, ToolExecutionStatus::Success);
    }

    #[test]
    fn typed_tool_errors_map_to_non_success_recovery_statuses() {
        use crate::error::ToolError;

        for error in [
            ToolError::NotFound("missing".into()),
            ToolError::Execution("failed".into()),
            ToolError::Disabled("disabled".into()),
            ToolError::Io("io".into()),
            ToolError::Network("network".into()),
        ] {
            assert_eq!(
                ToolExecutionOutcome::from_tool_error(error).status,
                ToolExecutionStatus::ToolError
            );
        }
        assert_eq!(
            ToolExecutionOutcome::from_tool_error(ToolError::Format("bad result".into())).status,
            ToolExecutionStatus::ProtocolError
        );
    }
}

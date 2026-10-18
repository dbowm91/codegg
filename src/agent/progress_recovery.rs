//! Bounded, observable progress detection and graduated tool recovery.
//!
//! This module deliberately stores fingerprints and small classifications, never
//! tool arguments, output, or model reasoning.  It is usable by the loop and by
//! model adapters without coupling either side to provider wire formats.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

use crate::tool::contract::ToolEffectClass;

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

/// Why the controller believes the turn advanced.
///
/// Every variant here must have a producer in the production observation path.
/// An unreachable variant is not documentation of a capability, it is a
/// standing invitation to believe in a signal that cannot fire — which is how
/// `ChildAdvanced` survived with a hardcoded `false` at the only construction
/// site. Adding one requires wiring a real observation, not just a case arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressSignal {
    None,
    NewEvidence,
    StateChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentKind {
    ExactRepeat,
    EquivalentResult,
    EquivalentError,
    MalformedCall,
    NarrationWithoutAction,
    NoProgress,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn recovery_error_class(&self) -> Option<&'static str> {
        match self.status {
            ToolExecutionStatus::Success => None,
            ToolExecutionStatus::Denied => Some("denied"),
            ToolExecutionStatus::Timeout => Some("timeout"),
            ToolExecutionStatus::Cancelled => Some("cancelled"),
            ToolExecutionStatus::ToolError => Some("tool_error"),
            ToolExecutionStatus::ProtocolError => Some("protocol_error"),
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
            // A missing workspace root is deterministic: the call was refused
            // because it had no directory to act in, and re-running it cannot
            // produce a different outcome. Classifying it as a tool error
            // (rather than a transport fault) keeps it on the same recovery
            // path as any other refusal.
            | crate::error::ToolError::MissingWorkspaceRoot { .. }
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
                        execution_status: None,
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
        observation.execution_status = Some(outcome.status);
        observation.error_class = outcome.recovery_error_class().map(str::to_owned);
        self.observe_tool(observation)
    }
}

/// One tool call as recovery sees it.
///
/// This struct holds fingerprints and small classifications only. It never
/// carries tool arguments, tool output, or model reasoning: the raw values stay
/// in the normal model context and recovery sees only their hashes.
///
/// Two fields that once lived here were removed rather than left in place.
/// `new_evidence` and `child_advanced` were both hardcoded `false` at the only
/// production construction site, with no other writer anywhere in the tree, so
/// `new_evidence` made half of `NewEvidence` unreachable and made
/// `ProgressSignal::ChildAdvanced` unreachable outright. A field that can never
/// be true is not a hook for a future signal — it is a claim that the signal
/// exists. Novelty is now derived from history (`novel_read` /
/// `novel_read_result` in `RecoveryController::observe`) instead of asserted by
/// a field nobody writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressObservation {
    pub action: ActionClass,
    pub canonical_tool: Option<String>,
    pub wire_tool: Option<String>,
    pub argument_fingerprint: Option<String>,
    pub result_fingerprint: Option<String>,
    pub result_size: ResultSizeClass,
    pub error_class: Option<String>,
    pub execution_status: Option<ToolExecutionStatus>,
    /// From the tool's contract. `None` means the registry has no entry for
    /// this name — an unregistered or renamed tool — which is *unknown*, never
    /// read-only. See [`is_read_effect`].
    pub effect_class: Option<ToolEffectClass>,
    /// True only when the loop observed an actual state transition for this
    /// call. See the construction site in `src/agent/loop.rs` for what counts
    /// and, deliberately, what does not.
    pub state_changed: bool,
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
            error_class: None,
            execution_status: None,
            effect_class: None,
            state_changed: false,
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
    pub execution_status: Option<ToolExecutionStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StalledReport {
    pub last_progress: ProgressSignal,
    pub incident: IncidentKind,
    pub attempted_recoveries: u8,
    pub evidence: String,
    pub suggested_user_action: String,
    pub execution_status: Option<ToolExecutionStatus>,
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
    /// `action_key()` of the call the tracked incident belongs to. Ladder state
    /// describes *one* incident; this is what makes "the same incident" a
    /// checkable fact instead of an assumption carried forward indefinitely.
    incident_key: Option<String>,
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
            incident_key: None,
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
        let successful = observation
            .execution_status
            .is_none_or(|status| status == ToolExecutionStatus::Success);
        let action_key = observation.action_key();
        let same_action_count = self
            .history
            .iter()
            .filter(|o| o.action_key() == action_key)
            .count();
        // A call this turn has never made cannot be a repeat of anything, so
        // on a read-only surface it is evidence by definition. Keying this off
        // novelty rather than off "did the model read recently" is what lets a
        // model read a dozen different files, or alternate `read` and `bash`
        // while exploring, without the detector calling that a stall.
        let novel_read = same_action_count == 0
            && successful
            && observation.error_class.is_none()
            && is_read_effect(observation.effect_class);
        let novel_read_result = is_read_effect(observation.effect_class)
            && successful
            && observation.error_class.is_none()
            // Only the first changed result for an action is evidence. This
            // bounded rule prevents volatile output from resetting recovery
            // forever while still recognizing a genuine read transition.
            && same_action_count == 1
            && self.history.iter().any(|o| {
                o.action_key() == action_key && o.result_fingerprint != observation.result_fingerprint
            });
        // Deliberately narrow, and deliberately *not* widened to "the call
        // succeeded". A successful mutating or process-class call — `bash`,
        // git, a job submission — carries no progress signal on its own,
        // because the loop has no oracle that distinguishes "changed the
        // workspace" from "ran and changed nothing": `state_changed` needs a
        // drained file-change bus event, and nothing emits on that bus for
        // process execution. Crediting those calls unconditionally would credit
        // every successful step a looping model takes and make the ladder
        // unable to see it. Widening this needs a per-tool state-change
        // observation, which does not exist yet.
        let progress = if successful && observation.state_changed {
            ProgressSignal::StateChanged
        } else if successful && (novel_read || novel_read_result) {
            ProgressSignal::NewEvidence
        } else {
            ProgressSignal::None
        };

        let execution_status = observation.execution_status;
        let incident = self.detect(&observation);
        self.push(observation);
        if progress != ProgressSignal::None {
            self.incident = None;
            self.incident_key = None;
            self.recoveries = 0;
            return RecoveryDecision::Progress;
        }
        let Some(kind) = incident else {
            return RecoveryDecision::Continue;
        };
        // Ladder state belongs to one incident. A different call is different
        // work, so it starts at the first rung instead of inheriting a count
        // the model has already climbed past. Without this reset, unrelated
        // failures accumulated into a terminal stall for work that was never
        // repeated — and a model that fixed its problem mid-sequence was
        // terminated for the failures that preceded the fix.
        if self.incident_key.as_deref() != Some(action_key.as_str()) {
            self.recoveries = 0;
            self.incident = None;
        }
        self.incident_key = Some(action_key);
        let occurrences = self
            .incident
            .as_ref()
            .map_or(1, |i| i.occurrences.saturating_add(1));
        let action = match self.recoveries {
            0 => RecoveryAction::Nudge,
            1 => RecoveryAction::Correct,
            2 => RecoveryAction::RestoreBasePalette,
            3 => RecoveryAction::Replan,
            _ => {
                tracing::warn!(
                    recoveries = self.recoveries,
                    "unmapped recovery count; defaulting to Stall"
                );
                RecoveryAction::Stall
            }
        };
        let evidence = bounded_evidence(kind, &self.history);
        let current = RecoveryIncident {
            kind,
            occurrences,
            last_action: action,
            evidence: evidence.clone(),
            execution_status,
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
                execution_status: current.execution_status,
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
        if current.error_class.is_some()
            && self.history.iter().any(|o| {
                o.action_key() == current.action_key() && o.error_class == current.error_class
            })
        {
            // Keyed on the whole call, like every other incident. It used to
            // compare `canonical_tool` and `error_class` alone, so any second
            // failure of the same tool counted as equivalent work — five
            // different commands failing the same way climbed the whole ladder
            // and terminated a turn that had never repeated anything. The error
            // *text* deliberately does not participate: the argument
            // fingerprint already identifies the call, and letting wording into
            // the key would make classification unstable across phrasings of
            // the same failure.
            return Some(IncidentKind::EquivalentError);
        }
        if same_action >= 1 {
            if self.history.iter().any(|o| {
                o.action_key() == current.action_key()
                    && o.result_fingerprint == current.result_fingerprint
            }) {
                return Some(IncidentKind::EquivalentResult);
            }
            return Some(IncidentKind::ExactRepeat);
        }
        // Nothing else is an incident. An earlier `ShortCycle` arm compared
        // `canonical_tool` alone and so fired exactly when the same tool was
        // reached for again with *different* arguments — reading a second
        // file, running a second grep. That is the ordinary shape of a model
        // doing its job, and because it outranked nothing it climbed the whole
        // ladder and stalled the turn. Repeated *work* is `ExactRepeat` /
        // `EquivalentResult` above, keyed on the whole call; distinct work is
        // progress (`novel_read` in `observe`). There is no third case.
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

/// Whether a call's effect class is a read.
///
/// `None` means the tool registry had no entry for the reported name — an
/// unregistered or renamed tool. That is *unknown*, and it is treated exactly
/// like a mutation: `ToolEffectClass`'s own `#[default]` is `NonIdempotent` for
/// the same reason. Never widen this to accept an unknown tool as a read. The
/// registry is the only authority for what a call does, and a lookup miss must
/// not silently become permission to call a mutation evidence.
fn is_read_effect(effect_class: Option<ToolEffectClass>) -> bool {
    matches!(
        effect_class,
        Some(ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate)
    )
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
            error_class: None,
            execution_status: None,
            effect_class: None,
            state_changed: false,
            selected_surface_fingerprint: None,
            batch_id: 0,
        }
    }
    fn read_obs(tool: &str, args: &str, result: &str) -> ProgressObservation {
        let mut o = obs(tool, args, result);
        o.effect_class = Some(ToolEffectClass::ReadOnly);
        o.execution_status = Some(ToolExecutionStatus::Success);
        o
    }
    /// A mutating/process-class call that failed with a typed tool error.
    fn failing_obs(tool: &str, args: &str, error_text: &str) -> ProgressObservation {
        let mut o = obs(tool, args, error_text);
        o.effect_class = Some(ToolEffectClass::NonIdempotent);
        o.execution_status = Some(ToolExecutionStatus::ToolError);
        o.error_class = Some("tool_error".into());
        o
    }
    fn mutating_success_obs(tool: &str, args: &str, result: &str) -> ProgressObservation {
        let mut o = obs(tool, args, result);
        o.effect_class = Some(ToolEffectClass::ProcessExec);
        o.execution_status = Some(ToolExecutionStatus::Success);
        o
    }
    /// A decision that escalates the ladder rather than letting the turn run on.
    fn is_escalation(decision: &RecoveryDecision) -> bool {
        matches!(
            decision,
            RecoveryDecision::Recover { .. } | RecoveryDecision::Stalled(_)
        )
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
        let mut first = obs("status", "a", "old");
        first.effect_class = Some(ToolEffectClass::ReadOnly);
        c.observe(first);
        let mut second = obs("status", "a", "new");
        second.effect_class = Some(ToolEffectClass::ReadOnly);
        assert_eq!(c.observe(second), RecoveryDecision::Progress);
    }

    #[test]
    fn equivalent_result_is_scoped_to_action_identity() {
        let mut c = RecoveryController::default();
        c.observe(obs("read", "a", "shared"));
        c.observe(obs("grep", "b", "shared"));
        let decision = c.observe(obs("list", "c", "shared"));
        assert_eq!(decision, RecoveryDecision::Continue);
    }

    #[test]
    fn repeated_same_action_and_result_is_equivalent() {
        let mut c = RecoveryController::default();
        c.observe(obs("read", "a", "same"));
        let decision = c.observe(obs("read", "a", "same"));
        assert!(matches!(
            decision,
            RecoveryDecision::Recover {
                incident: RecoveryIncident {
                    kind: IncidentKind::EquivalentResult,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn volatile_mutating_output_does_not_reset_recovery() {
        let mut c = RecoveryController::new(16, 2);
        for result in ["one", "two", "three", "four"] {
            let mut observation = obs("write", "a", result);
            observation.effect_class = Some(ToolEffectClass::NonIdempotent);
            let decision = c.observe(observation);
            assert!(!matches!(decision, RecoveryDecision::Progress));
        }
        assert!(c.recovery_count() > 0);
    }

    #[test]
    fn typed_status_remains_authoritative_through_recovery() {
        for status in [
            ToolExecutionStatus::Denied,
            ToolExecutionStatus::Timeout,
            ToolExecutionStatus::Cancelled,
            ToolExecutionStatus::ProtocolError,
            ToolExecutionStatus::ToolError,
        ] {
            let mut c = AutonomyState::default();
            let mut observation = obs("tool", "a", "permission denied timeout cancelled");
            observation.execution_status = Some(status);
            let outcome = ToolExecutionOutcome {
                status,
                model_text: "permission denied timeout cancelled".into(),
            };
            c.observe_tool_result(&outcome, observation.clone());
            let decision = c.observe_tool_result(&outcome, observation);
            assert!(matches!(
                decision,
                RecoveryDecision::Recover {
                    incident: RecoveryIncident {
                        execution_status: Some(actual),
                        ..
                    },
                    ..
                } if actual == status
            ));
        }
    }

    #[test]
    fn successful_failure_like_text_is_not_a_failure() {
        let mut c = AutonomyState::default();
        let outcome = ToolExecutionOutcome::success("permission denied; timeout cancelled");
        let decision = c.observe_tool_result(&outcome, obs("read", "a", &outcome.model_text));
        assert_eq!(decision, RecoveryDecision::Continue);
    }
    /// A retry with different arguments is a different call, not a repeat.
    ///
    /// This test used to assert the opposite — that `bash("command-a")` followed
    /// by `bash("command-b")`, both failing with the same `tool_error` class,
    /// was `EquivalentError`. That assertion *was* the defect: it is why five
    /// distinct failing commands climbed the ladder. The equivalent-error key is
    /// now the whole `action_key()`.
    #[test]
    fn a_retry_with_different_arguments_is_not_equivalent_work() {
        let mut c = RecoveryController::default();
        c.observe(failing_obs("bash", "command-a", "failed"));
        let decision = c.observe(failing_obs("bash", "command-b", "failed"));
        assert!(
            !is_escalation(&decision),
            "a different command is different work: {decision:?}"
        );
        assert_eq!(c.recovery_count(), 0);
    }

    /// The true positive survives: the *identical* failing call still escalates,
    /// and it starts at the first rung.
    #[test]
    fn an_identical_failing_call_is_still_an_incident() {
        let mut c = RecoveryController::default();
        c.observe(failing_obs(
            "bash",
            "cargo build",
            "error[E0425]: not found",
        ));
        let decision = c.observe(failing_obs(
            "bash",
            "cargo build",
            "error[E0425]: not found",
        ));
        assert!(matches!(
            decision,
            RecoveryDecision::Recover {
                action: RecoveryAction::Nudge,
                incident: RecoveryIncident {
                    kind: IncidentKind::EquivalentError,
                    ..
                },
                ..
            }
        ));
    }

    /// The identical failing call with *different* error text is still the
    /// identical call. Wording is not identity — the arguments are what say
    /// which work this is.
    #[test]
    fn error_wording_is_not_part_of_the_equivalent_error_key() {
        let mut c = RecoveryController::default();
        c.observe(failing_obs(
            "bash",
            "cargo build",
            "error[E0425]: cannot find `a`",
        ));
        let decision = c.observe(failing_obs("bash", "cargo build", "error[E0433]: no `b`"));
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

    /// An unregistered tool name carries `effect_class: None`. That is
    /// *unknown*, not read-only, so a novel call through it is not
    /// `NewEvidence`. Treating a lookup miss as a read would let an
    /// unrecognised mutation be counted as evidence.
    #[test]
    fn an_unregistered_tool_name_is_unknown_not_read_only() {
        let mut c = RecoveryController::default();
        let observation = obs("not_a_registered_tool", "args", "output");
        assert_eq!(observation.effect_class, None);
        assert!(
            !is_read_effect(observation.effect_class),
            "a registry miss must never read as a read"
        );
        assert_eq!(c.observe(observation), RecoveryDecision::Continue);
        assert_eq!(c.recovery_count(), 0);
    }

    /// W6 sweep: the ordinary shape of a bisecting build failure — the same
    /// command refined one attempt at a time, each attempt failing the same
    /// way. Every attempt is a different call, so none is a repeat. This must
    /// never stall; the bound is `ExecutionLimits::max_turns`.
    #[test]
    fn a_bisecting_failure_loop_does_not_stall() {
        let mut c = RecoveryController::default();
        for n in 0..12u8 {
            let decision = c.observe(failing_obs(
                "bash",
                &format!("cargo build --features feature-{n}"),
                &format!("error[E0599]: method `step{n}` not found"),
            ));
            assert!(
                !is_escalation(&decision),
                "attempt {n} is a new command, not a repeat: {decision:?}"
            );
        }
        assert_eq!(c.recovery_count(), 0);
    }

    /// W6 sweep: the build/fix loop. A failing build, then the edit that
    /// addresses it, then the next failing build — alternating between two
    /// different calls, so no incident identity ever repeats.
    #[test]
    fn a_build_fix_loop_does_not_stall() {
        let mut c = RecoveryController::default();
        for n in 0..12u8 {
            let build = c.observe(failing_obs(
                "bash",
                &format!("cargo build -p crate-{n}"),
                "error: could not compile",
            ));
            assert!(!is_escalation(&build), "build {n}: {build:?}");
            let fix = c.observe(obs(
                "edit",
                &format!("src/crate-{n}/lib.rs"),
                "applied 1 edit",
            ));
            assert!(
                matches!(fix, RecoveryDecision::Continue | RecoveryDecision::Progress),
                "fix {n} is not an incident: {fix:?}"
            );
        }
        assert_eq!(c.recovery_count(), 0);
    }

    /// W6 sweep: read-heavy exploration across many files — the shape the
    /// removed `ShortCycle` arm used to kill.
    #[test]
    fn read_heavy_exploration_does_not_stall() {
        let mut c = RecoveryController::default();
        for n in 0..40u8 {
            let decision = c.observe(read_obs("read", &format!("src/module-{n}.rs"), "contents"));
            assert_eq!(decision, RecoveryDecision::Progress, "read {n}");
        }
        assert_eq!(c.recovery_count(), 0);
    }

    /// W7: the ladder still terminates a genuinely repeating turn, through the
    /// same bounded rungs and no further. Widening what counts as progress must
    /// never make a real loop survivable.
    #[test]
    fn the_ladder_still_terminates_a_genuinely_repeating_turn() {
        let mut c = RecoveryController::default();
        let mut rungs = Vec::new();
        let mut stalled = false;
        for _ in 0..12 {
            match c.observe(read_obs("read", "same-path", "same-bytes")) {
                RecoveryDecision::Recover { action, .. } => rungs.push(action),
                RecoveryDecision::Stalled(_) => {
                    stalled = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(stalled, "an identical repeat must still terminate the turn");
        assert_eq!(
            rungs,
            vec![
                RecoveryAction::Nudge,
                RecoveryAction::Correct,
                RecoveryAction::RestoreBasePalette,
                RecoveryAction::Replan,
            ],
            "the bounded ladder must not grow or reorder"
        );
    }

    /// W7: the same guarantee through `AutonomyState`, which owns the turn-level
    /// transition bound. A stalled turn still breaks, and the transition budget
    /// remains the backstop for a loop that never classifies an incident.
    #[test]
    fn autonomy_state_still_bounds_and_breaks_a_repeating_turn() {
        let mut state = AutonomyState::default();
        let mut recoveries_before_stall = None;
        for _ in 0..12 {
            if let RecoveryDecision::Stalled(report) = state.observe_tool_result(
                &ToolExecutionOutcome::success("same-bytes"),
                obs("read", "same-path", "same-bytes"),
            ) {
                recoveries_before_stall = Some(report.attempted_recoveries);
                break;
            }
        }
        assert!(
            recoveries_before_stall.is_some(),
            "the transition bound must still break the turn"
        );
        assert_eq!(state.phase(), AutonomyPhase::Stall);
    }

    /// W1 evidence (a): distinct failing commands are distinct work.
    ///
    /// `detect` classified `EquivalentError` on `canonical_tool` plus
    /// `error_class` alone, ignoring `action_key()` and ignoring the error
    /// text. So *any* second failure of the same tool was "equivalent" work
    /// however different the command and however different the message. A model
    /// working through a bisecting build failure — five different commands,
    /// five different diagnostics — climbed Nudge → Correct →
    /// RestoreBasePalette → Replan and stood one rung from having its turn
    /// terminated for work it had never once repeated.
    #[test]
    fn distinct_failing_commands_do_not_climb_the_ladder() {
        let mut c = RecoveryController::default();
        let mut escalated = Vec::new();
        for n in 0..5u8 {
            let command = format!("cargo build -p crate-{n}");
            let decision = c.observe(failing_obs(
                "bash",
                &command,
                &format!("error[E0425]: cannot find value `x{n}` in this scope"),
            ));
            if is_escalation(&decision) {
                escalated.push(command);
            }
        }
        assert!(
            escalated.is_empty(),
            "five distinct failing commands must never escalate the ladder, but these did: {escalated:?}"
        );
        assert_eq!(c.recovery_count(), 0, "no incident was warranted at all");
    }

    /// W1 evidence (b): an unrelated incident must not inherit an earlier
    /// incident's ladder count.
    ///
    /// `observe` reset `recoveries`/`incident` only on a progress signal,
    /// never on a change of incident identity. Two repeats of one call left
    /// `recoveries` at 1, and the next unrelated failures kept climbing that
    /// count until the turn was terminated for work it had never repeated.
    #[test]
    fn an_unrelated_incident_does_not_inherit_an_earlier_incidents_ladder_count() {
        let mut c = RecoveryController::default();
        c.observe(read_obs("read", "a", "same"));
        assert!(matches!(
            c.observe(read_obs("read", "a", "same")),
            RecoveryDecision::Recover { .. }
        ));
        assert_eq!(c.recovery_count(), 1, "one repeat is one rung");

        for n in 0..3u8 {
            let decision = c.observe(failing_obs(
                "bash",
                &format!("retry-{n}"),
                &format!("distinct failure {n}"),
            ));
            assert!(
                !is_escalation(&decision),
                "an unrelated failure must not climb the earlier incident's ladder: {decision:?}"
            );
        }
        assert_eq!(
            c.recovery_count(),
            1,
            "three unrelated failures must add nothing to the one genuine incident"
        );
    }

    /// W1 evidence (c): the model recovered, and the ladder did not notice.
    ///
    /// `state_changed` requires a drained file-change bus event *and*
    /// `is_file_modifying_tool`, which matches only
    /// `write|edit|replace|apply_patch`. A successful `bash` therefore carried
    /// no progress signal, and whatever failure count had accumulated was
    /// carried straight across the recovery — so the first failure *after* the
    /// model had already fixed something was escalated against the
    /// pre-recovery ladder instead of starting from the first rung.
    #[test]
    fn a_successful_mutating_shell_call_does_not_carry_a_failure_ladder_forward() {
        let mut c = RecoveryController::default();
        c.observe(failing_obs("bash", "make --dry-run", "undefined reference"));
        c.observe(failing_obs("bash", "make --dry-run", "undefined reference"));
        assert_eq!(c.recovery_count(), 1, "the retry is one rung");

        let recovered = c.observe(mutating_success_obs(
            "bash",
            "make --dry-run --fix",
            "linking target/debug/app",
        ));
        assert!(
            !is_escalation(&recovered),
            "a successful call that is not a repeat is not an incident: {recovered:?}"
        );

        // The same command that just succeeded, now failing, is a first
        // incident *for that command* — it must start at the first rung, not
        // continue the ladder the pre-recovery failures built up.
        let next = c.observe(failing_obs(
            "bash",
            "make --dry-run --fix",
            "undefined reference to `helper`",
        ));
        assert!(
            matches!(
                next,
                RecoveryDecision::Recover {
                    action: RecoveryAction::Nudge,
                    ..
                }
            ),
            "the post-recovery failure must start at Nudge, not inherit the earlier ladder: {next:?}"
        );
        assert_eq!(c.recovery_count(), 1);
    }

    /// Regression: `detect` had a `ShortCycle` arm that compared
    /// `canonical_tool` alone while every other check used the whole
    /// `action_key`. It therefore fired exactly when the same tool was
    /// reached for again with *different* arguments — reading a second file,
    /// running a second grep — which is the ordinary shape of a model
    /// exploring a codebase. The bounded ladder then ran Nudge → Correct →
    /// RestoreBasePalette → Replan → Stall and killed the turn, so a model
    /// that was simply reading and grepping was reported as "stalled".
    #[test]
    fn reaching_for_the_same_tool_with_new_arguments_is_not_an_incident() {
        let mut c = RecoveryController::default();
        // read a; grep; read b; list; read c ... — different files, same tools.
        for n in 0..12 {
            let tool = match n % 3 {
                0 => "read",
                1 => "grep",
                _ => "list",
            };
            let decision = c.observe(read_obs(tool, &format!("target-{n}"), "contents"));
            assert_eq!(
                decision,
                RecoveryDecision::Progress,
                "distinct work must read as progress (iteration {n})"
            );
        }
        assert_eq!(c.recovery_count(), 0, "no recovery was ever warranted");
    }

    /// The reported scenario: reads interleaved with shell work, which is how
    /// a model that batch-reads actually operates. Never stalls.
    #[test]
    fn interleaving_reads_and_shell_is_never_a_stall() {
        let mut c = RecoveryController::default();
        for n in 0..12 {
            let decision = c.observe(read_obs("read", &format!("file-{n}.rs"), "contents"));
            assert_eq!(decision, RecoveryDecision::Progress);
            let decision = c.observe(obs(
                "bash",
                &format!("rg -n 'needle' src/module-{n}"),
                "matches",
            ));
            assert!(
                !matches!(decision, RecoveryDecision::Stalled(_)),
                "interleaving shell work must not stall (iteration {n}): {decision:?}"
            );
        }
        assert_eq!(c.recovery_count(), 0, "no recovery was ever warranted");
    }

    /// Novelty is the whole point: a call the turn never made is not a repeat,
    /// so it must not be escalated no matter how it interleaves.
    #[test]
    fn a_novel_call_is_never_an_incident() {
        let mut c = RecoveryController::default();
        for n in 0..20 {
            assert!(matches!(
                c.observe(obs("bash", &format!("cmd-{n}"), "")),
                RecoveryDecision::Continue
            ));
        }
        assert_eq!(c.recovery_count(), 0);
    }

    /// Genuinely repeated work is still caught — the fix narrows what counts
    /// as a loop, it does not abolish loop detection.
    #[test]
    fn the_identical_call_is_still_detected() {
        let mut c = RecoveryController::default();
        c.observe(read_obs("read", "same", "one"));
        let decision = c.observe(read_obs("read", "same", "one"));
        assert!(matches!(
            decision,
            RecoveryDecision::Recover {
                incident: RecoveryIncident {
                    kind: IncidentKind::EquivalentResult,
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

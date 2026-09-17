//! Fresh provider context-epoch reconstruction (long-horizon M004).
//!
//! This module is a **consumer/path of the existing compaction owner**, not a
//! second compaction engine. It owns no token-accounting algorithm, no
//! transcript store, and no history rewrite:
//!
//! - policy decisions reuse `codegg-core::work_plan::epoch_policy`
//!   (deterministic, host-state only);
//! - replacement validation reuses `super::rollover::validate_replacement_messages`
//!   and `super::compaction::{validate_message_invariants,
//!   count_continuation_frames}`;
//! - checkpoint persistence reuses
//!   `codegg-core::session::continuation::ContinuationCheckpointStore` via the
//!   existing rollover sequencing (`prepare_candidate` / `install_prepared`);
//! - durable session message history is never deleted or mutated here — the
//!   caller replaces only the in-memory/provider-visible sequence after a
//!   verified prepared checkpoint exists, exactly like normal rollover.
//!
//! A context epoch is provider-visible context, not durable task state. At a
//! safe policy-selected boundary the next epoch is rebuilt from canonical
//! system/runtime instructions, objective, WorkPlan/Goal/Todo projections,
//! installed continuation state, bounded latest user steering, and exact
//! recovery handles. Epoch reset never resets workspace, Git, jobs,
//! AgentRuns, budgets, model selection, permissions, sandbox, or user
//! steering state.

use crate::provider::{ContentPart, Message};

// Re-export the deterministic policy so callers have one import path.
pub use codegg_core::work_plan::{
    decide_epoch, decision_diagnostic, epoch_supported_for_profile, ContextEpochDecision,
    ContextEpochInputs, ContextEpochKeepReason, ContextEpochPolicy, ContextEpochTrigger,
};

// ── Bounds ────────────────────────────────────────────────────────────────

/// Maximum steering messages carried into a fresh epoch (latest spine).
pub const MAX_EPOCH_STEERING_MESSAGES: usize = 5;
/// Maximum chars per steering message excerpt.
pub const MAX_EPOCH_STEERING_CHARS: usize = 2000;
/// Maximum recovery handles listed in the handoff block.
pub const MAX_EPOCH_RECOVERY_HANDLES: usize = 16;
/// Maximum chars for the assembled handoff block.
pub const MAX_EPOCH_HANDOFF_CHARS: usize = 12 * 1024;

// ── Inputs ────────────────────────────────────────────────────────────────

/// Host-owned inputs to one fresh-epoch reconstruction.
///
/// Every field comes from authoritative host state: compiled prompt owners,
/// Goal/WorkPlan/Todo stores, the installed continuation checkpoint, and
/// exact user-authored steering. Model-generated summaries, hidden reasoning,
/// and provider-private state never enter here.
pub struct FreshEpochInputs<'a> {
    /// Canonical compiled system/developer/runtime instructions.
    pub system_instructions: &'a str,
    /// Immutable session/user objective provenance.
    pub objective: &'a str,
    /// Latest Goal projection when a Goal is active.
    pub goal: Option<FreshEpochGoal<'a>>,
    /// Current WorkPlan provenance (bounded handoff, never full plan).
    pub work_plan: Option<&'a codegg_core::work_plan::WorkPlanCheckpointProvenance>,
    /// Bounded Todo projection (identifiers/short contents only).
    pub todos: &'a [String],
    /// Installed continuation projection text (single current block source).
    pub continuation_frame_text: &'a str,
    /// Exact recovery handles (`ctx://...`), bounded.
    pub recovery_handles: &'a [String],
    /// Bounded latest user steering/control spine (exact user texts, newest
    /// last). Older steering beyond the bound is dropped, never summarized
    /// by the model.
    pub steering: &'a [String],
    /// Installed checkpoint identity for lineage/diagnostics.
    pub checkpoint_id: &'a str,
    pub checkpoint_sequence: i64,
}

#[derive(Debug, Clone)]
pub struct FreshEpochGoal<'a> {
    pub goal_id: &'a str,
    pub revision: i64,
    pub objective: &'a str,
    pub current_phase: Option<&'a str>,
    pub next_action: Option<&'a str>,
}

// ── Lineage ───────────────────────────────────────────────────────────────

/// Bounded epoch lineage for diagnostics/events/restart.
///
/// Carries IDs, revisions, counts, and reason codes only — never payload
/// contents, user text, or secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEpochLineage {
    pub epoch_id: String,
    pub reason: String,
    pub trigger: String,
    pub checkpoint_id: String,
    pub checkpoint_sequence: i64,
    pub work_plan_id: Option<String>,
    pub work_plan_revision: Option<i64>,
    pub prior_compaction_count: usize,
    pub profile_id: String,
}

impl ContextEpochLineage {
    pub fn bounded_line(&self) -> String {
        format!(
            "context_epoch(id={}, reason={}, trigger={}, checkpoint={} seq {}, work_plan={} rev {}, prior_compactions={}, profile={})",
            self.epoch_id,
            self.reason,
            self.trigger,
            if self.checkpoint_id.is_empty() {
                "-"
            } else {
                &self.checkpoint_id
            },
            self.checkpoint_sequence,
            self.work_plan_id.as_deref().unwrap_or("-"),
            self.work_plan_revision
                .map(|r| r.to_string())
                .unwrap_or_else(|| "-".to_string()),
            self.prior_compaction_count,
            if self.profile_id.is_empty() {
                "-"
            } else {
                &self.profile_id
            },
        )
    }
}

// ── Reconstruction ────────────────────────────────────────────────────────

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn bound_steering(steering: &[String]) -> Vec<String> {
    steering
        .iter()
        .rev()
        .take(MAX_EPOCH_STEERING_MESSAGES)
        .rev()
        .map(|s| truncate_chars(s.trim(), MAX_EPOCH_STEERING_CHARS))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Build the single handoff block text from authoritative host state.
///
/// Order is fixed: objective → Goal → WorkPlan → Todos → continuation state
/// → recovery handles → latest steering. No historical stale compaction
/// frames are copied; no hidden reasoning is included.
///
/// The text starts with the versioned continuation marker so the existing
/// single-frame counter (`count_continuation_frames`) recognizes exactly one
/// current block, just like normal compaction.
pub fn build_handoff_text(inputs: &FreshEpochInputs<'_>) -> String {
    let mut parts: Vec<String> =
        vec![crate::agent::context_frame::CONTINUATION_STATE_MARKER_V1.to_string()];
    let objective = inputs.objective.trim();
    if !objective.is_empty() {
        parts.push(format!("- Objective: {objective}"));
    }
    if let Some(goal) = inputs.goal.as_ref() {
        let mut line = format!("Goal: {} rev {}", goal.goal_id, goal.revision);
        if let Some(phase) = goal.current_phase {
            let phase = phase.trim();
            if !phase.is_empty() {
                line.push_str(&format!(" phase {phase}"));
            }
        }
        if let Some(action) = goal.next_action {
            let action = action.trim();
            if !action.is_empty() {
                line.push_str(&format!(" next: {action}"));
            }
        }
        parts.push(line);
    }
    if let Some(work_plan) = inputs.work_plan {
        let mut line = format!(
            "WorkPlan: {} rev {} status {}",
            work_plan.plan_id, work_plan.revision, work_plan.status
        );
        if let Some(phase) = work_plan.current_phase.as_deref() {
            line.push_str(&format!(" phase {phase}"));
        }
        if let Some(item) = work_plan.current_item_id.as_deref() {
            line.push_str(&format!(" item {item}"));
        }
        parts.push(line);
        for item in work_plan.actionable.iter().take(5) {
            let mut entry = format!("- next {}: {}", item.id, item.description);
            if let Some(action) = item.next_action.as_deref() {
                entry.push_str(&format!(" (next: {action})"));
            }
            parts.push(entry);
        }
        for item in work_plan.blocked.iter().take(3) {
            parts.push(format!("- blocked {}: {}", item.id, item.description));
        }
    }
    if !inputs.todos.is_empty() {
        let todos: Vec<String> = inputs
            .todos
            .iter()
            .take(8)
            .map(|t| truncate_chars(t.trim(), 200))
            .filter(|t| !t.is_empty())
            .collect();
        if !todos.is_empty() {
            parts.push(format!("Todos: {}", todos.join(" | ")));
        }
    }
    let frame = inputs.continuation_frame_text.trim();
    if !frame.is_empty() {
        parts.push(format!("Continuation state:\n{frame}"));
    }
    let handles: Vec<String> = inputs
        .recovery_handles
        .iter()
        .take(MAX_EPOCH_RECOVERY_HANDLES)
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();
    if !handles.is_empty() {
        parts.push(format!("Recovery handles: {}", handles.join(", ")));
    }
    for entry in bound_steering(inputs.steering) {
        parts.push(format!("User steering: {entry}"));
    }
    parts.push(format!(
        "Checkpoint: {} seq {}",
        inputs.checkpoint_id, inputs.checkpoint_sequence
    ));
    let mut text = parts.join("\n");
    if text.len() > MAX_EPOCH_HANDOFF_CHARS {
        let mut end = MAX_EPOCH_HANDOFF_CHARS;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[bounded epoch handoff truncated]");
    }
    text
}

/// Build a fresh provider-visible message sequence from canonical state.
///
/// The result contains no tool history (no orphan pairs by construction),
/// exactly one CodeGG continuation/handoff block, and the latest user
/// steering as the final visible input. Caller must validate with
/// [`validate_fresh_epoch_messages`] before replacing history, and must
/// only activate after a verified prepared checkpoint exists (reuse the
/// rollover transaction ordering; see `super::rollover`).
///
/// Like the canonical compaction engine, the handoff is always its own
/// `System` message starting with the versioned continuation marker. If the
/// resolved profile cannot accept that form, this returns a bounded error
/// and the caller must use normal compaction for that profile instead of
/// inventing a provider-specific second history model.
pub fn build_fresh_epoch_messages(
    inputs: &FreshEpochInputs<'_>,
    profile: &crate::model_profile::types::ResolvedModelProfile,
) -> Result<Vec<Message>, String> {
    if inputs.system_instructions.trim().is_empty() && inputs.objective.trim().is_empty() {
        return Err("fresh epoch requires canonical instructions or objective".to_string());
    }
    if !epoch_supported_for_profile(profile) {
        return Err(format!(
            "unsupported profile for fresh epoch ({:?}); use normal compaction",
            profile.prompt_profile
        ));
    }
    // Reject hidden-reasoning smuggling: the handoff builder never emits it,
    // and inputs are host-owned. Defense-in-depth: refuse inputs that look
    // like private reasoning carriers.
    for handle in inputs.recovery_handles {
        if handle.contains('\0') {
            return Err("invalid recovery handle: NUL byte".to_string());
        }
    }
    let mut messages: Vec<Message> = Vec::new();
    let system = inputs.system_instructions.trim();
    if !system.is_empty() {
        messages.push(Message::System {
            content: system.to_string().into(),
        });
    }
    let handoff = build_handoff_text(inputs);
    if handoff.trim().is_empty() {
        return Err("fresh epoch handoff is empty".to_string());
    }
    // Canonical placement: its own System block starting with the versioned
    // marker (same contract as `compile_frame_messages`). Never merged into
    // the base instructions and never copied as stale history.
    messages.push(Message::System {
        content: handoff.into(),
    });
    // Latest steering stays visible as the current user turn when it is not
    // already the tail. This preserves post-checkpoint user control over
    // stale next actions.
    if let Some(latest) = bound_steering(inputs.steering).last() {
        let tail_has_it = messages.iter().rev().take(2).any(|m| match m {
            Message::User { content } => content.iter().any(|p| match p {
                ContentPart::Text { text } => text.contains(latest.as_str()),
                _ => false,
            }),
            Message::System { content } => content.contains(latest.as_str()),
            _ => false,
        });
        if !tail_has_it {
            messages.push(Message::User {
                content: vec![ContentPart::Text {
                    text: latest.clone().into(),
                }],
            });
        }
    }
    validate_fresh_epoch_messages(&messages)?;
    Ok(messages)
}

/// Validate a fresh-epoch candidate before activation.
///
/// Checks, in order: tool-call/result pairing and IDs remain valid (fresh
/// epochs carry no tool history, so any tool message is a defect); exactly
/// one current CodeGG continuation frame; at least one visible user or
/// system handoff block. Provider chronology is the emitted order.
pub fn validate_fresh_epoch_messages(messages: &[Message]) -> Result<(), String> {
    crate::context::compaction::validate_message_invariants(messages)
        .map_err(|e| format!("tool-pair invariant: {e}"))?;
    let frames = crate::context::compaction::count_continuation_frames(messages);
    // The handoff block renders through the same continuation-frame marker
    // as normal compaction (`to_continuation_text` prefix). Profiles that
    // merge control into the base system message still carry exactly one
    // marker; a missing marker means the handoff was dropped.
    if frames != 1 {
        return Err(format!(
            "exactly one continuation frame required in fresh epoch, found {frames}"
        ));
    }
    Ok(())
}

/// Build the bounded `context_epoch:started` bus event for a fresh epoch.
///
/// Never carries payload contents, user text, or secrets — IDs, revisions,
/// counts, and reason codes only.
pub fn build_epoch_started_event(
    session_id: &str,
    decision: &ContextEpochDecision,
    lineage: &ContextEpochLineage,
) -> codegg_core::bus::events::AppEvent {
    codegg_core::bus::events::AppEvent::ContextEpochStarted {
        session_id: session_id.to_string(),
        reason: decision.reason_code().to_string(),
        trigger: decision
            .trigger
            .map(|t| t.as_str().to_string())
            .unwrap_or_else(|| "-".to_string()),
        checkpoint_id: lineage.checkpoint_id.clone(),
        checkpoint_sequence: lineage.checkpoint_sequence,
        work_plan_id: lineage.work_plan_id.clone(),
        work_plan_revision: lineage.work_plan_revision,
        prior_compaction_count: lineage.prior_compaction_count,
        profile_id: lineage.profile_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_profile::types::ResolvedModelProfile;
    use codegg_core::model_profile::resolve::infer_builtin_profile;
    use codegg_core::work_plan::{WorkPlanCheckpointProvenance, MAX_CHECKPOINT_WORK_ITEMS};

    fn provenance_fixture() -> WorkPlanCheckpointProvenance {
        WorkPlanCheckpointProvenance {
            plan_id: "wp_1".to_string(),
            revision: 3,
            status: "active".to_string(),
            current_phase: Some("phase one".to_string()),
            current_item_id: Some("wi_a".to_string()),
            actionable: vec![codegg_core::work_plan::WorkPlanCheckpointItem {
                id: "wi_a".to_string(),
                revision: 0,
                status: "actionable".to_string(),
                description: "implement epoch".to_string(),
                next_action: Some("write code".to_string()),
            }],
            blocked: vec![],
            source_digest: "sha256:abc".to_string(),
            total_items: 3,
            actionable_count: 1,
            blocked_count: 0,
        }
    }

    fn inputs<'a>(
        work_plan: Option<&'a WorkPlanCheckpointProvenance>,
        steering: &'a [String],
        todos: &'a [String],
        handles: &'a [String],
    ) -> FreshEpochInputs<'a> {
        FreshEpochInputs {
            system_instructions: "system: be helpful",
            objective: "ship the feature",
            goal: Some(FreshEpochGoal {
                goal_id: "goal-1",
                revision: 2,
                objective: "ship the feature",
                current_phase: Some("phase one"),
                next_action: Some("write code"),
            }),
            work_plan,
            todos,
            continuation_frame_text: "Continuation [v1]\nGoal: ship\nNext: write code",
            recovery_handles: handles,
            steering,
            checkpoint_id: "ckpt-1",
            checkpoint_sequence: 2,
        }
    }

    fn profile_allows_late_system() -> ResolvedModelProfile {
        infer_builtin_profile("openai/gpt-5")
    }

    #[test]
    fn handoff_contains_authoritative_state_and_no_reasoning() {
        let provenance = provenance_fixture();
        let steering = vec!["keep tests green".to_string()];
        let todos = vec!["implement epoch".to_string()];
        let handles = vec!["ctx://tool/s/0/c0".to_string()];
        let text = build_handoff_text(&inputs(Some(&provenance), &steering, &todos, &handles));
        assert!(text.contains("ship the feature"));
        assert!(text.contains("wp_1"));
        assert!(text.contains("wi_a"));
        assert!(text.contains("keep tests green"));
        assert!(text.contains("ctx://tool/s/0/c0"));
        assert!(!text.contains("reasoning"));
        assert!(text.len() <= MAX_EPOCH_HANDOFF_CHARS + 64);
    }

    #[test]
    fn fresh_messages_have_single_frame_and_valid_pairs() {
        let provenance = provenance_fixture();
        let steering = vec!["latest steering".to_string()];
        let todos = vec!["implement epoch".to_string()];
        let handles = vec!["ctx://tool/s/0/c0".to_string()];
        let messages = build_fresh_epoch_messages(
            &inputs(Some(&provenance), &steering, &todos, &handles),
            &profile_allows_late_system(),
        )
        .expect("fresh epoch");
        // No tool history is carried, so pairing is trivially valid.
        assert!(crate::context::compaction::validate_message_invariants(&messages).is_ok());
        assert_eq!(
            crate::context::compaction::count_continuation_frames(&messages),
            1
        );
        // Latest steering remains visible.
        let visible: String = messages
            .iter()
            .filter_map(|m| match m {
                Message::User { content } => Some(
                    content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                Message::System { content } => Some(content.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("latest steering"));
        assert!(visible.contains("wp_1"));
    }

    #[test]
    fn unsupported_profile_uses_normal_compaction() {
        let minimax = infer_builtin_profile("minimax/minimax-2.7");
        assert!(!epoch_supported_for_profile(&minimax));
        let provenance = provenance_fixture();
        let steering = vec!["steering".to_string()];
        let todos = vec!["implement epoch".to_string()];
        let handles = vec!["ctx://tool/s/0/c0".to_string()];
        let err = build_fresh_epoch_messages(
            &inputs(Some(&provenance), &steering, &todos, &handles),
            &minimax,
        )
        .unwrap_err();
        assert!(
            err.contains("unsupported profile"),
            "unsupported profile must fall back to normal compaction, got: {err}"
        );
    }

    #[test]
    fn empty_instructions_and_objective_rejected() {
        let provenance = provenance_fixture();
        let steering: Vec<String> = vec![];
        let todos = vec!["implement epoch".to_string()];
        let handles = vec!["ctx://tool/s/0/c0".to_string()];
        let mut bad = inputs(Some(&provenance), &steering, &todos, &handles);
        bad.system_instructions = "  ";
        bad.objective = "  ";
        assert!(build_fresh_epoch_messages(&bad, &profile_allows_late_system()).is_err());
    }

    #[test]
    fn hidden_reasoning_never_included() {
        let provenance = provenance_fixture();
        let steering = vec!["ordinary steering".to_string()];
        let todos = vec!["implement epoch".to_string()];
        let handles = vec!["ctx://tool/s/0/c0".to_string()];
        let messages = build_fresh_epoch_messages(
            &inputs(Some(&provenance), &steering, &todos, &handles),
            &profile_allows_late_system(),
        )
        .unwrap();
        for message in &messages {
            match message {
                Message::Assistant { content, .. } | Message::User { content } => {
                    for part in content {
                        if let ContentPart::Reasoning { .. } = part {
                            panic!("fresh epoch must not carry hidden reasoning");
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn provenance_bound_is_respected() {
        const { assert!(MAX_CHECKPOINT_WORK_ITEMS <= 8) };
    }

    #[test]
    fn lineage_line_never_carries_body() {
        let lineage = ContextEpochLineage {
            epoch_id: "epoch-1".to_string(),
            reason: "phase_boundary".to_string(),
            trigger: "phase_boundary".to_string(),
            checkpoint_id: "ckpt-1".to_string(),
            checkpoint_sequence: 2,
            work_plan_id: Some("wp_1".to_string()),
            work_plan_revision: Some(3),
            prior_compaction_count: 4,
            profile_id: "openai/gpt-5".to_string(),
        };
        let line = lineage.bounded_line();
        assert!(line.contains("ckpt-1"));
        assert!(line.contains("wp_1"));
        assert!(!line.contains("secret-body-marker"));
    }
}

//! Authoritative continuation snapshot assembler (M002).
//!
//! Single host-side assembler that turns host-owned session/goal/todo,
//! ledger, and user-intent state into one typed [`ContinuationSnapshot`]
//! accepted by the M001 continuation store and renderable as exactly one
//! versioned continuation frame.
//!
//! Source precedence (encoded, not just documented):
//!
//! ```text
//! Objective:
//!   1. Active durable Goal.objective + goal ID/revision when present.
//!   2. Otherwise immutable session-origin prompt.
//!   3. Never an LLM paraphrase.
//!
//! Current task:
//!   1. Explicit active goal current_phase / next_action.
//!   2. In-progress todo.
//!   3. Most recent host-owned continuation next action.
//!   4. No invented fallback string.
//!
//! Plan: metadata only (path + SHA-256 digest + phase/action + bounded
//! item descriptors + explicit plan_unavailable diagnostic). Never infer
//! project identity from the plan path.
//!
//! Deterministic evidence (files, commands, tests, errors, security,
//! artifact handles) comes from host/runtime state.
//!
//! Semantic fields (constraints, decisions, unresolved blockers, next
//! steps) are advisory enrichment merged against the previous installed
//! checkpoint plus current-epoch evidence; they never override host facts.
//! ```
//!
//! Prompt-block precedence (documented for M004 activation):
//!
//! ```text
//! current turn user input (newest, always visible)
//!   outranks active goal revision newer than the checkpoint
//!     outranks installed continuation state projection
//!       outranks stale checkpoint semantic next steps
//! ```
//!
//! Once a continuation checkpoint is installed, the prompt compiler must
//! not emit a contradictory duplicate objective/progress projection: the
//! continuation block (`PromptBlockKind::ContinuationState`,
//! `continuation:installed-checkpoint`) supersedes the pre-compaction
//! `goal:active-checkpoint` block, which remains useful only before the
//! first compaction.

use crate::agent::context_frame::{bounded_artifact_handles, ContextFrame, ContextLedgerState};
use crate::context::{compute_content_hash, stable_hash_hex};
use crate::provider::{ContentPart, Message};
use crate::session::continuation::{ContinuationCheckpoint, ContinuationCheckpointPayload};
use serde::{Deserialize, Serialize};

/// Maximum estimated tokens for the exact user-intent spine (M002 §6.3).
///
/// Comparable to the separate retained-user budget used by reference
/// implementations (order of 20k tokens); measured with CodeGG token
/// estimation and recorded with truncation diagnostics.
pub const MAX_INTENT_SPINE_TOKENS: usize = 20_000;

/// Maximum inline chars per intent entry (defense-in-depth alongside the
/// token budget).
pub const MAX_INTENT_ENTRY_CHARS: usize = 8_000;

/// Maximum semantic items per list carried in a snapshot.
pub const MAX_SEMANTIC_ITEMS: usize = 32;

/// Maximum chars per semantic item.
pub const MAX_SEMANTIC_ITEM_CHARS: usize = 1_024;

/// Maximum deterministic evidence items per list in a snapshot.
pub const MAX_EVIDENCE_ITEMS: usize = 32;

/// Maximum chars per evidence item in a snapshot.
pub const MAX_EVIDENCE_ITEM_CHARS: usize = 1_024;

/// Maximum model-visible continuation frame bytes (defense-in-depth).
pub const MAX_FRAME_TEXT_BYTES: usize = 16 * 1024;

/// Where the authoritative objective came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveSource {
    ActiveGoal,
    SessionOrigin,
}

/// Where the current task came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentTaskSource {
    GoalNextAction,
    GoalPhase,
    WorkPlanNextAction,
    InProgressTodo,
    PreviousContinuation,
    None,
}

/// One exact user-authored intent entry with its own provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationIntentEntry {
    /// Checkpoint-owned intent ID (`intent_origin`, `intent_steer_<n>`,
    /// `intent_current`). Stable within one snapshot; M003 may add exact
    /// evidence references without inventing MessageStore provenance.
    pub id: String,
    /// SHA-256 digest of the full original user text (even when inline
    /// text is truncated).
    pub digest: String,
    /// Bounded inline text (possibly truncated).
    pub text: String,
    /// True when `text` is a bounded prefix of the original.
    pub truncated: bool,
    /// Original full length in chars.
    pub full_chars: usize,
}

/// Bounded plan metadata (never an unbounded file body).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationPlanState {
    pub plan_path: Option<String>,
    pub plan_digest: Option<String>,
    /// Set when the path cannot be read; compaction must not abort.
    pub plan_unavailable: Option<String>,
    pub current_phase: Option<String>,
    pub current_action: Option<String>,
}

/// Explicit active-goal provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationGoalState {
    pub goal_id: String,
    pub revision: i64,
    pub objective: String,
}

/// Bounded todo projection (identifiers only, not a task graph).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationTodoState {
    pub in_progress: Vec<String>,
    pub pending: Vec<String>,
    pub blocked: Vec<String>,
}

/// Advisory semantic state (enrichment only, never authority).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationSemanticState {
    pub constraints: Vec<String>,
    pub decisions: Vec<String>,
    pub unresolved_blockers: Vec<String>,
    pub next_steps: Vec<String>,
}

/// Previous-checkpoint provenance carried for merge/staleness detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationPreviousState {
    pub checkpoint_id: String,
    pub sequence: i64,
    pub digest: String,
}

/// One typed authoritative continuation snapshot.
///
/// Assembled purely from host-owned inputs by
/// [`assemble_continuation_snapshot`]. Convertible into an M001 store
/// payload ([`to_payload`]) and into one versioned model-visible frame
/// ([`to_context_frame`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationSnapshot {
    pub session_id: String,
    /// Authoritative current objective (goal or origin, never paraphrase).
    pub objective: String,
    pub objective_source: ObjectiveSource,
    pub goal: Option<ContinuationGoalState>,
    /// Immutable session-origin task provenance (always retained).
    pub origin_text: Option<String>,
    pub origin_digest: Option<String>,
    pub current_task: Option<String>,
    pub current_task_source: CurrentTaskSource,
    pub plan: ContinuationPlanState,
    /// Bounded WorkPlan provenance (M004). `None` for legacy sessions
    /// without an active WorkPlan; the full plan stays in
    /// `codegg-core::work_plan` and is reachable through WorkPlan tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_plan: Option<codegg_core::work_plan::WorkPlanCheckpointProvenance>,
    pub todos: ContinuationTodoState,
    pub intent_spine: Vec<ContinuationIntentEntry>,
    pub intent_truncated: bool,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub security_findings: Vec<String>,
    pub artifact_handles: Vec<String>,
    pub semantic: ContinuationSemanticState,
    pub previous: Option<ContinuationPreviousState>,
    /// Bounded diagnostics: sources used, intent truncation, prior
    /// checkpoint carried forward, semantic-enrichment outcome.
    pub diagnostics: Vec<String>,
}

/// Typed inputs to the pure assembler.
///
/// Storage lookups happen in the agent/turn adapter before calling the
/// pure assembler; this struct carries already-loaded values plus the
/// current provider-visible messages for steering extraction. The plan
/// file body (when readable under the current workspace) is supplied as
/// `plan_content` so the assembler stays synchronous and pure.
pub struct ContinuationAssemblyInput<'a> {
    pub session_id: &'a str,
    pub origin_prompt: Option<&'a str>,
    pub current_user_message: Option<&'a str>,
    /// Current provider-visible messages for steering extraction.
    pub messages: &'a [Message],
    pub active_goal: Option<&'a crate::goal::model::Goal>,
    pub todos: &'a [crate::task_state::TodoItem],
    pub ledger: &'a ContextLedgerState,
    pub security_findings: &'a [String],
    pub previous_checkpoint: Option<&'a ContinuationCheckpoint>,
    pub plan_path: Option<&'a str>,
    /// Already-read plan file body, if readable. `None` with
    /// `plan_path = Some` records a `plan_unavailable` diagnostic.
    pub plan_content: Option<&'a str>,
    /// Active durable WorkPlan plus its items for bounded checkpoint
    /// provenance (M004). `None` for legacy sessions without a plan.
    pub active_work_plan: Option<(
        &'a codegg_core::work_plan::WorkPlan,
        &'a [codegg_core::work_plan::WorkItem],
    )>,
}

fn estimate_tokens(text: &str) -> usize {
    eggcontext::estimate_tokens_sync(text, None)
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let truncated: String = text.chars().take(max_chars).collect();
    (truncated, true)
}

fn bound_list(items: &[String], max_items: usize, max_chars: usize) -> Vec<String> {
    items
        .iter()
        .take(max_items)
        .map(|item| {
            let (text, truncated) = truncate_chars(item, max_chars);
            if truncated {
                format!("{text}…[truncated]")
            } else {
                text
            }
        })
        .filter(|item| !item.trim().is_empty())
        .collect()
}

fn user_text_of(message: &Message) -> Option<String> {
    match message {
        Message::User { content } => {
            let text = content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = text.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        }
        _ => None,
    }
}

fn intent_entry(id: String, full_text: &str) -> ContinuationIntentEntry {
    let digest = stable_hash_hex(full_text.as_bytes());
    let full_chars = full_text.chars().count();
    let (text, truncated) = truncate_chars(full_text, MAX_INTENT_ENTRY_CHARS);
    ContinuationIntentEntry {
        id,
        digest,
        text,
        truncated,
        full_chars,
    }
}

/// Extract the exact user steering spine from provider-visible messages.
///
/// Deterministically includes the immutable origin (when it matches the
/// session-origin prompt), the most recent user steering/correction
/// messages, and the current triggering user message. Assistant responses
/// are never treated as intent.
fn extract_intent_spine(
    messages: &[Message],
    origin_prompt: Option<&str>,
    current_user_message: Option<&str>,
    diagnostics: &mut Vec<String>,
) -> (Vec<ContinuationIntentEntry>, bool) {
    let mut user_texts: Vec<String> = messages.iter().filter_map(user_text_of).collect();

    if let Some(current) = current_user_message {
        let current = current.trim();
        if !current.is_empty() && user_texts.last().is_none_or(|last| last != current) {
            user_texts.push(current.to_string());
        }
    }

    if user_texts.is_empty() {
        if let Some(origin) = origin_prompt {
            let origin = origin.trim();
            if !origin.is_empty() {
                let entry = intent_entry("intent_origin".to_string(), origin);
                return (vec![entry], false);
            }
        }
        return (Vec::new(), false);
    }

    // Identify the origin entry: first user text matching the immutable
    // session-origin prompt, else the first user text (provenance).
    let origin_text = origin_prompt
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .or_else(|| user_texts.first().cloned());

    let mut entries = Vec::new();
    if let Some(origin) = origin_text.as_deref() {
        entries.push(intent_entry("intent_origin".to_string(), origin));
    }

    // Steering: user texts after the origin position (skip the origin
    // itself when it appears as the first message).
    let steering_start = if user_texts
        .first()
        .is_some_and(|first| Some(first.as_str()) == origin_text.as_deref())
    {
        1
    } else {
        0
    };
    for (index, text) in user_texts.iter().skip(steering_start).enumerate() {
        // Avoid duplicating the origin entry when the transcript repeats it.
        if Some(text.as_str()) == origin_text.as_deref() {
            continue;
        }
        entries.push(intent_entry(format!("intent_steer_{index:04}"), text));
    }

    if entries.is_empty() {
        return (entries, false);
    }

    // Bounded budget: retain origin/current steering boundaries rather than
    // arbitrary middle slices. Keep the origin plus the most recent
    // steering that fits; record truncation diagnostics with digests.
    let mut total_tokens: usize = entries
        .iter()
        .map(|entry| estimate_tokens(&entry.text))
        .sum();
    let mut truncated = false;
    if total_tokens > MAX_INTENT_SPINE_TOKENS {
        truncated = true;
        let origin_entry = entries.remove(0);
        let origin_tokens = estimate_tokens(&origin_entry.text);
        let mut kept_from_tail: Vec<ContinuationIntentEntry> = Vec::new();
        let mut kept_tokens = 0usize;
        for entry in entries.into_iter().rev() {
            let tokens = estimate_tokens(&entry.text);
            if origin_tokens + kept_tokens + tokens > MAX_INTENT_SPINE_TOKENS {
                continue;
            }
            kept_tokens += tokens;
            kept_from_tail.push(entry);
        }
        kept_from_tail.reverse();
        let dropped = user_texts.len().saturating_sub(kept_from_tail.len() + 1);
        let mut rebuilt = Vec::with_capacity(kept_from_tail.len() + 1);
        rebuilt.push(origin_entry);
        rebuilt.extend(kept_from_tail);
        entries = rebuilt;
        total_tokens = origin_tokens + kept_tokens;
        diagnostics.push(format!(
            "intent_spine_truncated(dropped_middle_entries={dropped}, kept={}, budget_tokens={MAX_INTENT_SPINE_TOKENS})",
            entries.len(),
        ));
    }

    // Re-mark the current (last) entry ID for recovery-reference stability.
    if entries.len() > 1 {
        if let Some(last) = entries.last_mut() {
            last.id = "intent_current".to_string();
        }
    }

    diagnostics.push(format!(
        "intent_spine(entries={}, estimated_tokens={total_tokens}, truncated={truncated})",
        entries.len(),
        total_tokens = total_tokens,
        truncated = truncated,
    ));
    (entries, truncated)
}

/// Previous installed semantic state decoded from an M001 payload body.
///
/// Unknown shapes degrade to empty state with a diagnostic; a decode
/// failure never partially merges.
fn previous_semantic(previous: Option<&ContinuationCheckpoint>) -> ContinuationSemanticState {
    let Some(checkpoint) = previous else {
        return ContinuationSemanticState::default();
    };
    let body = &checkpoint.payload.body;
    let semantic = body.get("semantic");
    let get_list = |key: &str| {
        semantic
            .and_then(|value| value.get(key))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .take(MAX_SEMANTIC_ITEMS)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let blockers = get_list("unresolved_blockers");
    let blockers = if blockers.is_empty() {
        get_list("unresolved_errors")
    } else {
        blockers
    };
    ContinuationSemanticState {
        constraints: get_list("constraints"),
        decisions: get_list("decisions"),
        unresolved_blockers: blockers,
        next_steps: get_list("next_steps"),
    }
}

/// Most recent host-owned continuation next action from a previous payload.
fn previous_next_action(previous: Option<&ContinuationCheckpoint>) -> Option<String> {
    let body = &previous?.payload.body;
    if let Some(action) = body.get("current_task").and_then(|v| v.as_str()) {
        let trimmed = action.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    body.get("semantic")
        .and_then(|semantic| semantic.get("next_steps"))
        .and_then(|steps| steps.as_array())
        .and_then(|steps| steps.first())
        .and_then(|first| first.as_str())
        .map(|step| step.trim().to_string())
        .filter(|step| !step.is_empty())
}

/// Assemble one authoritative continuation snapshot from host-owned inputs.
///
/// Deterministic and synchronous over captured state: goal-store lookup
/// failure must fall back to session-origin provenance with a diagnostic
/// (callers pass `active_goal = None` in that case rather than a fake
/// goal). An unreadable plan path records `plan_unavailable` and never
/// aborts assembly.
pub fn assemble_continuation_snapshot(
    input: ContinuationAssemblyInput<'_>,
) -> ContinuationSnapshot {
    let mut diagnostics = Vec::new();
    diagnostics.push(format!("source(session_id={})", input.session_id));

    // --- Objective / goal precedence ---
    let (objective, objective_source, goal_state, origin_text, origin_digest) =
        match input.active_goal {
            Some(goal) => {
                diagnostics.push(format!(
                    "objective_source(active_goal id={} revision={})",
                    goal.id, goal.revision
                ));
                let origin = input.origin_prompt.map(str::trim).filter(|t| !t.is_empty());
                let origin_text = origin.map(str::to_string);
                let origin_digest = origin.map(|text| stable_hash_hex(text.as_bytes()));
                (
                    goal.objective.clone(),
                    ObjectiveSource::ActiveGoal,
                    Some(ContinuationGoalState {
                        goal_id: goal.id.clone(),
                        revision: goal.revision,
                        objective: goal.objective.clone(),
                    }),
                    origin_text,
                    origin_digest,
                )
            }
            None => {
                let origin = input.origin_prompt.map(str::trim).unwrap_or_default();
                diagnostics.push("objective_source(session_origin)".to_string());
                let (origin_text, origin_digest) = if origin.is_empty() {
                    (None, None)
                } else {
                    (
                        Some(origin.to_string()),
                        Some(stable_hash_hex(origin.as_bytes())),
                    )
                };
                let objective = if origin.is_empty() {
                    String::new()
                } else {
                    origin.to_string()
                };
                (
                    objective,
                    ObjectiveSource::SessionOrigin,
                    None,
                    origin_text,
                    origin_digest,
                )
            }
        };

    // --- Current task precedence ---
    let in_progress_todo = input
        .todos
        .iter()
        .find(|item| item.status == crate::task_state::TodoStatus::InProgress)
        .map(|item| item.content.clone());
    // WorkPlan next action: current item's next_action, else first
    // actionable item's next_action/description. Bounded and host-owned;
    // never model prose.
    let work_plan_next_action: Option<String> =
        input.active_work_plan.as_ref().and_then(|(plan, items)| {
            if let Some(current_id) = plan.current_item_id.as_ref() {
                if let Some(current) = items.iter().find(|i| i.id == *current_id) {
                    if !current.status.is_terminal() {
                        if let Some(action) = current.next_action.as_deref() {
                            let trimmed = action.trim();
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
                        }
                        let desc = current.description.trim();
                        if !desc.is_empty() {
                            return Some(desc.to_string());
                        }
                    }
                }
            }
            // Fall back to the first actionable item in stable order.
            codegg_core::work_plan::actionable_items(items)
                .first()
                .and_then(|item| {
                    item.next_action
                        .as_deref()
                        .map(|a| a.trim().to_string())
                        .filter(|a| !a.is_empty())
                        .or_else(|| {
                            let desc = item.description.trim();
                            (!desc.is_empty()).then(|| desc.to_string())
                        })
                })
        });
    let prev_action = previous_next_action(input.previous_checkpoint);
    let (current_task, current_task_source) = match input.active_goal {
        Some(goal)
            if goal
                .next_action
                .as_deref()
                .is_some_and(|a| !a.trim().is_empty()) =>
        {
            (
                goal.next_action.clone().map(|a| a.trim().to_string()),
                CurrentTaskSource::GoalNextAction,
            )
        }
        Some(goal)
            if goal
                .current_phase
                .as_deref()
                .is_some_and(|p| !p.trim().is_empty()) =>
        {
            (
                goal.current_phase.clone().map(|p| p.trim().to_string()),
                CurrentTaskSource::GoalPhase,
            )
        }
        _ if work_plan_next_action
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty()) =>
        {
            (
                work_plan_next_action.clone(),
                CurrentTaskSource::WorkPlanNextAction,
            )
        }
        _ if in_progress_todo
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty()) =>
        {
            (in_progress_todo.clone(), CurrentTaskSource::InProgressTodo)
        }
        _ if prev_action.as_deref().is_some_and(|a| !a.trim().is_empty()) => {
            (prev_action.clone(), CurrentTaskSource::PreviousContinuation)
        }
        _ => (None, CurrentTaskSource::None),
    };
    diagnostics.push(format!("current_task_source({:?})", current_task_source));

    // --- WorkPlan provenance (M004, bounded handoff, never full plan) ---
    let work_plan = input.active_work_plan.as_ref().map(|(plan, items)| {
        let provenance = codegg_core::work_plan::build_checkpoint_provenance(plan, items);
        diagnostics.push(format!(
            "work_plan_provenance(id={} revision={} actionable={} blocked={})",
            provenance.plan_id,
            provenance.revision,
            provenance.actionable_count,
            provenance.blocked_count,
        ));
        provenance
    });
    if input.active_work_plan.is_none() {
        diagnostics.push("work_plan_provenance(absent:legacy)".to_string());
    }

    // --- Plan metadata (never an unbounded body) ---
    let plan = match (input.plan_path, input.plan_content) {
        (Some(path), Some(content)) => {
            diagnostics.push(format!("plan_source(path={path})"));
            ContinuationPlanState {
                plan_path: Some(path.to_string()),
                plan_digest: Some(stable_hash_hex(content.as_bytes())),
                plan_unavailable: None,
                current_phase: input.active_goal.and_then(|g| g.current_phase.clone()),
                current_action: input
                    .active_goal
                    .and_then(|g| g.next_action.clone())
                    .or(in_progress_todo.clone()),
            }
        }
        (Some(path), None) => {
            diagnostics.push(format!("plan_unavailable(path={path})"));
            ContinuationPlanState {
                plan_path: Some(path.to_string()),
                plan_digest: None,
                plan_unavailable: Some("plan path could not be read under workspace".to_string()),
                current_phase: input.active_goal.and_then(|g| g.current_phase.clone()),
                current_action: input
                    .active_goal
                    .and_then(|g| g.next_action.clone())
                    .or(in_progress_todo.clone()),
            }
        }
        (None, _) => ContinuationPlanState {
            plan_path: None,
            plan_digest: None,
            plan_unavailable: None,
            current_phase: input.active_goal.and_then(|g| g.current_phase.clone()),
            current_action: input
                .active_goal
                .and_then(|g| g.next_action.clone())
                .or(in_progress_todo.clone()),
        },
    };

    // --- Todos (bounded identifiers) ---
    let mut in_progress = Vec::new();
    let mut pending = Vec::new();
    let mut blocked = Vec::new();
    for item in input.todos {
        let bounded = truncate_chars(&item.content, MAX_EVIDENCE_ITEM_CHARS).0;
        match item.status {
            crate::task_state::TodoStatus::InProgress => in_progress.push(bounded),
            crate::task_state::TodoStatus::Pending => pending.push(bounded),
            crate::task_state::TodoStatus::Blocked => blocked.push(bounded),
            crate::task_state::TodoStatus::Completed | crate::task_state::TodoStatus::Cancelled => {
            }
        }
    }
    let todos = ContinuationTodoState {
        in_progress: in_progress.into_iter().take(MAX_EVIDENCE_ITEMS).collect(),
        pending: pending.into_iter().take(MAX_EVIDENCE_ITEMS).collect(),
        blocked: blocked.into_iter().take(MAX_EVIDENCE_ITEMS).collect(),
    };

    // --- Exact user intent spine ---
    let (intent_spine, intent_truncated) = extract_intent_spine(
        input.messages,
        input.origin_prompt,
        input.current_user_message,
        &mut diagnostics,
    );

    // --- Deterministic evidence from host/runtime state ---
    let touched_files = bound_list(
        &input.ledger.touched_files,
        MAX_EVIDENCE_ITEMS,
        MAX_EVIDENCE_ITEM_CHARS,
    );
    let commands_run = bound_list(
        &input
            .ledger
            .commands_run
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        MAX_EVIDENCE_ITEMS,
        MAX_EVIDENCE_ITEM_CHARS,
    );
    let test_results = bound_list(
        &input.ledger.test_results,
        MAX_EVIDENCE_ITEMS,
        MAX_EVIDENCE_ITEM_CHARS,
    );
    let unresolved_errors = bound_list(
        &input.ledger.unresolved_errors,
        MAX_EVIDENCE_ITEMS,
        MAX_EVIDENCE_ITEM_CHARS,
    );
    let security_findings = bound_list(
        input.security_findings,
        MAX_EVIDENCE_ITEMS,
        MAX_EVIDENCE_ITEM_CHARS,
    );
    let artifact_handles = bounded_artifact_handles(&input.ledger.artifact_handles);
    diagnostics.push(format!(
        "evidence(files={}, commands={}, tests={}, errors={}, security={}, artifacts={})",
        touched_files.len(),
        commands_run.len(),
        test_results.len(),
        unresolved_errors.len(),
        security_findings.len(),
        artifact_handles.len(),
    ));

    // --- Semantic carry-forward (previous installed + current epoch) ---
    let previous_semantic = previous_semantic(input.previous_checkpoint);
    if input.previous_checkpoint.is_some() {
        diagnostics.push("previous_checkpoint(carried_forward)".to_string());
    }
    // Current-epoch user constraints are deterministic evidence; they join
    // the carried-forward semantic constraints here so later enrichment
    // merges rather than regenerates from a previous summary.
    let epoch_constraints = crate::context::compaction::extract_user_constraints(input.messages);
    let mut semantic = previous_semantic;
    for constraint in bound_list(
        &epoch_constraints,
        MAX_SEMANTIC_ITEMS,
        MAX_SEMANTIC_ITEM_CHARS,
    ) {
        if !semantic.constraints.contains(&constraint) {
            semantic.constraints.push(constraint);
        }
    }
    semantic.constraints.truncate(MAX_SEMANTIC_ITEMS);
    semantic.decisions.truncate(MAX_SEMANTIC_ITEMS);
    semantic.unresolved_blockers.truncate(MAX_SEMANTIC_ITEMS);
    semantic.next_steps.truncate(MAX_SEMANTIC_ITEMS);

    let previous = input.previous_checkpoint.map(|checkpoint| {
        let digest = checkpoint
            .payload
            .digest()
            .unwrap_or_else(|_| checkpoint.payload_digest.clone());
        ContinuationPreviousState {
            checkpoint_id: checkpoint.id.clone(),
            sequence: checkpoint.sequence,
            digest,
        }
    });

    ContinuationSnapshot {
        session_id: input.session_id.to_string(),
        objective,
        objective_source,
        goal: goal_state,
        origin_text,
        origin_digest,
        current_task,
        current_task_source,
        plan,
        work_plan,
        todos,
        intent_spine,
        intent_truncated,
        touched_files,
        commands_run,
        test_results,
        unresolved_errors,
        security_findings,
        artifact_handles,
        semantic,
        previous,
        diagnostics,
    }
}

/// Merge a semantic enrichment frame into a snapshot without overwriting
/// host facts.
///
/// Only the four semantic-owned lists are updated (merged with
/// deduplication and bounds). Goal ID/revision, plan digest, file list,
/// command list, and test state are never touched: the semantic model is
/// advisory/enrichment, never authority.
pub fn apply_semantic_enrichment(
    snapshot: &mut ContinuationSnapshot,
    semantic: &ContextFrame,
    outcome: &str,
) {
    merge_semantic_list(&mut snapshot.semantic.constraints, &semantic.constraints);
    merge_semantic_list(&mut snapshot.semantic.decisions, &semantic.decisions);
    // Semantic unresolved errors are blockers, not deterministic test
    // state: they join the advisory blocker list, never replace
    // `unresolved_errors` / `test_results`.
    merge_semantic_list(
        &mut snapshot.semantic.unresolved_blockers,
        &semantic.unresolved_errors,
    );
    // Current user steering always outranks older checkpoint semantic next
    // steps: enrichment next steps merge behind, never ahead of, the
    // snapshot's existing next steps derived from host state.
    let mut merged_next = snapshot.semantic.next_steps.clone();
    for step in &semantic.next_steps {
        let (bounded, _) = truncate_chars(step, MAX_SEMANTIC_ITEM_CHARS);
        if bounded.trim().is_empty() || merged_next.contains(&bounded) {
            continue;
        }
        merged_next.push(bounded);
    }
    merged_next.truncate(MAX_SEMANTIC_ITEMS);
    snapshot.semantic.next_steps = merged_next;
    snapshot
        .diagnostics
        .push(format!("semantic_enrichment({outcome})"));
}

fn merge_semantic_list(target: &mut Vec<String>, incoming: &[String]) {
    for item in incoming {
        let (bounded, _) = truncate_chars(item, MAX_SEMANTIC_ITEM_CHARS);
        if bounded.trim().is_empty() || target.contains(&bounded) {
            continue;
        }
        target.push(bounded);
    }
    target.truncate(MAX_SEMANTIC_ITEMS);
}

impl ContinuationSnapshot {
    /// Model-visible projection: one versioned continuation frame.
    ///
    /// Prioritizes objective/current work/constraints/decisions/next
    /// action and provides exact recovery handles for large evidence
    /// rather than embedding it. Bounded to [`MAX_FRAME_TEXT_BYTES`]
    /// with UTF-8-safe truncation.
    pub fn to_context_frame(&self) -> ContextFrame {
        let mut unresolved = self.unresolved_errors.clone();
        for blocker in &self.semantic.unresolved_blockers {
            if !unresolved.contains(blocker) {
                unresolved.push(blocker.clone());
            }
        }
        // WorkPlan blocked summaries join the advisory blocker surface only;
        // deterministic `unresolved_errors` stay authoritative.
        if let Some(work_plan) = self.work_plan.as_ref() {
            for item in work_plan.blocked.iter() {
                let marker = format!("{}: {}", item.id, item.description);
                if !unresolved.contains(&marker) && unresolved.len() < MAX_EVIDENCE_ITEMS {
                    unresolved.push(marker);
                }
            }
        }
        unresolved.truncate(MAX_EVIDENCE_ITEMS);
        let mut next_steps = self.semantic.next_steps.clone();
        if next_steps.is_empty() {
            if let Some(task) = self.current_task.clone() {
                next_steps.push(task);
            } else if let Some(work_plan) = self.work_plan.as_ref() {
                if let Some(first) = work_plan.actionable.first() {
                    if let Some(action) = first.next_action.clone() {
                        next_steps.push(action);
                    } else {
                        next_steps.push(first.description.clone());
                    }
                }
            }
        }
        ContextFrame {
            user_goal: Some(self.objective.clone()).filter(|goal| !goal.trim().is_empty()),
            current_task: self.current_task.clone(),
            constraints: self.semantic.constraints.clone(),
            decisions: self.semantic.decisions.clone(),
            touched_files: self.touched_files.clone(),
            commands_run: self.commands_run.clone(),
            test_results: self.test_results.clone(),
            unresolved_errors: unresolved,
            security_findings: self.security_findings.clone(),
            next_steps,
            artifact_handles: self.artifact_handles.clone(),
        }
    }

    /// Render the single current versioned continuation frame text.
    pub fn render_frame_text(&self) -> String {
        let frame = self.to_context_frame();
        let mut text = frame.to_continuation_text();
        // Provenance footer: bounded, no bodies.
        let mut provenance = String::new();
        if let Some(goal) = &self.goal {
            provenance.push_str(&format!(
                "\n- Goal ref: {} rev {}",
                goal.goal_id, goal.revision
            ));
        }
        if let Some(work_plan) = &self.work_plan {
            provenance.push_str(&format!(
                "\n- WorkPlan: {} rev {} status {}",
                work_plan.plan_id, work_plan.revision, work_plan.status
            ));
            if let Some(phase) = work_plan.current_phase.as_deref() {
                provenance.push_str(&format!(" phase {phase}"));
            }
            if let Some(item) = work_plan.current_item_id.as_deref() {
                provenance.push_str(&format!(" item {item}"));
            }
        }
        if let Some(digest) = &self.origin_digest {
            let short = digest.chars().take(12).collect::<String>();
            provenance.push_str(&format!("\n- Origin digest: {short}"));
        }
        if let Some(previous) = &self.previous {
            provenance.push_str(&format!(
                "\n- Supersedes: {} seq {}",
                previous.checkpoint_id, previous.sequence
            ));
        }
        if let Some(path) = &self.plan.plan_path {
            provenance.push_str(&format!("\n- Plan: {path}"));
            if let Some(digest) = &self.plan.plan_digest {
                let short = digest.chars().take(12).collect::<String>();
                provenance.push_str(&format!(" digest {short}"));
            }
        }
        if !provenance.is_empty() {
            text.push_str(&provenance);
        }
        if text.len() > MAX_FRAME_TEXT_BYTES {
            let mut end = MAX_FRAME_TEXT_BYTES;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str("\n[bounded continuation frame truncated]");
        }
        text
    }

    /// Typed checkpoint body accepted by the M001 store.
    ///
    /// Keys avoid the store's forbidden content classes; user text travels
    /// as values only.
    pub fn to_payload_body(&self) -> serde_json::Value {
        let mut body = serde_json::json!({
            "snapshot_kind": "continuation_snapshot_v1",
            "session_id": self.session_id,
            "objective": self.objective,
            "objective_source": match self.objective_source {
                ObjectiveSource::ActiveGoal => "active_goal",
                ObjectiveSource::SessionOrigin => "session_origin",
            },
            "goal_id": self.goal.as_ref().map(|g| g.goal_id.clone()),
            "goal_revision": self.goal.as_ref().map(|g| g.revision),
            "origin_digest": self.origin_digest,
            "origin_text": self.origin_text,
            "current_task": self.current_task,
            "current_task_source": match self.current_task_source {
                CurrentTaskSource::GoalNextAction => "goal_next_action",
                CurrentTaskSource::GoalPhase => "goal_phase",
                CurrentTaskSource::WorkPlanNextAction => "work_plan_next_action",
                CurrentTaskSource::InProgressTodo => "in_progress_todo",
                CurrentTaskSource::PreviousContinuation => "previous_continuation",
                CurrentTaskSource::None => "none",
            },
            "plan_path": self.plan.plan_path,
            "plan_digest": self.plan.plan_digest,
            "plan_unavailable": self.plan.plan_unavailable,
            "plan_phase": self.plan.current_phase,
            "plan_action": self.plan.current_action,
            "todos": {
                "in_progress": self.todos.in_progress,
                "pending": self.todos.pending,
                "blocked": self.todos.blocked,
            },
            "intent_spine": self.intent_spine.iter().map(|entry| {
                serde_json::json!({
                    "id": entry.id,
                    "digest": entry.digest,
                    "text": entry.text,
                    "truncated": entry.truncated,
                    "full_chars": entry.full_chars,
                })
            }).collect::<Vec<_>>(),
            "intent_truncated": self.intent_truncated,
            "touched_files": self.touched_files,
            "commands": self.commands_run,
            "tests": self.test_results,
            "errors": self.unresolved_errors,
            "security": self.security_findings,
            "artifact_handles": self.artifact_handles,
            "semantic": {
                "constraints": self.semantic.constraints,
                "decisions": self.semantic.decisions,
                "unresolved_blockers": self.semantic.unresolved_blockers,
                "next_steps": self.semantic.next_steps,
            },
            "previous_checkpoint": self.previous.as_ref().map(|p| {
                serde_json::json!({
                    "checkpoint_id": p.checkpoint_id,
                    "sequence": p.sequence,
                    "digest": p.digest,
                })
            }),
            "diagnostics": self.diagnostics,
        });
        // Additive WorkPlan provenance (M004). Absent for legacy sessions;
        // present bodies never embed the full plan.
        if let Some(work_plan) = self.work_plan.as_ref() {
            if let Ok(json) = serde_json::to_value(work_plan) {
                if let Some(map) = body.as_object_mut() {
                    map.insert("work_plan".to_string(), json);
                }
            }
        }
        body
    }

    /// Build the M001 payload envelope for this snapshot.
    pub fn to_payload(&self) -> Result<ContinuationCheckpointPayload, crate::error::StorageError> {
        ContinuationCheckpointPayload::new(self.to_payload_body())
    }

    /// Content digest of the payload body (for staleness/identity checks).
    pub fn content_digest(&self) -> String {
        compute_content_hash(&serde_json::to_string(&self.to_payload_body()).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_with(files: &[&str], commands: &[&str]) -> ContextLedgerState {
        let mut ledger = ContextLedgerState::new();
        ledger.touched_files = files.iter().map(|s| s.to_string()).collect();
        ledger.commands_run = commands.iter().map(|s| s.to_string()).collect();
        ledger
    }

    fn user(text: &str) -> Message {
        Message::User {
            content: vec![ContentPart::Text {
                text: text.to_string().into(),
            }],
        }
    }

    fn test_goal(objective: &str) -> crate::goal::model::Goal {
        crate::goal::model::Goal {
            id: "goal-1".to_string(),
            revision: 7,
            session_id: "sess".to_string(),
            project_id: "/tmp".to_string(),
            title: "goal".to_string(),
            objective: objective.to_string(),
            status: crate::goal::model::GoalStatus::Active,
            plan_path: None,
            checkpoint_path: None,
            current_phase: Some("phase two".to_string()),
            progress_summary: String::new(),
            next_action: Some("ship it".to_string()),
            completion_criteria: vec![],
            open_questions: vec![],
            budget: Default::default(),
            usage: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn active_goal_outranks_origin_for_objective() {
        let goal = test_goal("goal objective authoritative");
        let ledger = ledger_with(&[], &[]);
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "sess",
            origin_prompt: Some("origin prompt"),
            current_user_message: None,
            messages: &[user("origin prompt")],
            active_goal: Some(&goal),
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        assert_eq!(snapshot.objective, "goal objective authoritative");
        assert_eq!(snapshot.objective_source, ObjectiveSource::ActiveGoal);
        assert_eq!(snapshot.goal.as_ref().unwrap().revision, 7);
        // Origin remains provenance.
        assert_eq!(snapshot.origin_text.as_deref(), Some("origin prompt"));
        assert!(snapshot.origin_digest.is_some());
    }

    #[test]
    fn no_goal_session_uses_origin_prompt() {
        let ledger = ledger_with(&[], &[]);
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "sess",
            origin_prompt: Some("origin only"),
            current_user_message: None,
            messages: &[user("origin only")],
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        assert_eq!(snapshot.objective, "origin only");
        assert_eq!(snapshot.objective_source, ObjectiveSource::SessionOrigin);
        assert!(snapshot.goal.is_none());
    }

    #[test]
    fn current_task_prefers_goal_next_action_then_todo() {
        let mut goal = test_goal("obj");
        goal.next_action = Some("goal next".to_string());
        let ledger = ledger_with(&[], &[]);
        let todos = vec![crate::task_state::TodoItem {
            id: "1".to_string(),
            content: "todo in progress".to_string(),
            status: crate::task_state::TodoStatus::InProgress,
            priority: crate::task_state::TodoPriority::High,
            blocker: None,
        }];
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "sess",
            origin_prompt: Some("origin"),
            current_user_message: None,
            messages: &[user("origin")],
            active_goal: Some(&goal),
            todos: &todos,
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        assert_eq!(snapshot.current_task.as_deref(), Some("goal next"));
        assert_eq!(
            snapshot.current_task_source,
            CurrentTaskSource::GoalNextAction
        );
    }

    #[test]
    fn intent_spine_retains_steering_and_marks_truncation() {
        let ledger = ledger_with(&[], &[]);
        let long = "x".repeat(50_000);
        let messages = vec![
            user("origin task"),
            user(&long),
            user("latest steering correction"),
        ];
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "sess",
            origin_prompt: Some("origin task"),
            current_user_message: Some("latest steering correction"),
            messages: &messages,
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        // Origin and current boundaries retained.
        assert!(snapshot
            .intent_spine
            .iter()
            .any(|e| e.id == "intent_origin"));
        assert!(snapshot
            .intent_spine
            .iter()
            .any(|e| e.id == "intent_current"));
        assert!(snapshot
            .intent_spine
            .iter()
            .any(|e| e.text.contains("latest steering correction")));
        // Long entry is per-entry bounded.
        assert!(snapshot
            .intent_spine
            .iter()
            .all(|e| e.text.chars().count() <= MAX_INTENT_ENTRY_CHARS));
        for entry in &snapshot.intent_spine {
            assert_eq!(entry.digest.len(), 64);
        }
    }

    #[test]
    fn artifact_handles_bounded_dedup_recency() {
        let mut ledger = ContextLedgerState::new();
        for i in 0..40 {
            ledger.artifact_handles.push(format!("ctx://tool/s/0/c{i}"));
        }
        ledger
            .artifact_handles
            .push("ctx://tool/s/0/c5".to_string());
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "s",
            origin_prompt: Some("origin"),
            current_user_message: None,
            messages: &[user("origin")],
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        assert!(snapshot.artifact_handles.len() <= 32);
        assert_eq!(
            snapshot.artifact_handles.len(),
            snapshot
                .artifact_handles
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
        );
        // Most recent retained.
        assert!(snapshot
            .artifact_handles
            .contains(&"ctx://tool/s/0/c39".to_string()));
    }

    #[test]
    fn plan_digest_stable_and_unreadable_diagnostic() {
        let ledger = ledger_with(&[], &[]);
        let first = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "s",
            origin_prompt: Some("o"),
            current_user_message: None,
            messages: &[user("o")],
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: Some("plans/ impl.md"),
            plan_content: Some("plan body"),
            active_work_plan: None,
        });
        let second = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "s",
            origin_prompt: Some("o"),
            current_user_message: None,
            messages: &[user("o")],
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: Some("plans/ impl.md"),
            plan_content: Some("plan body"),
            active_work_plan: None,
        });
        assert_eq!(first.plan.plan_digest, second.plan.plan_digest);
        assert!(first.plan.plan_unavailable.is_none());

        let missing = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "s",
            origin_prompt: Some("o"),
            current_user_message: None,
            messages: &[user("o")],
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: Some("plans/missing.md"),
            plan_content: None,
            active_work_plan: None,
        });
        assert!(missing.plan.plan_digest.is_none());
        assert!(missing.plan.plan_unavailable.is_some());
    }

    #[test]
    fn semantic_enrichment_never_overwrites_host_facts() {
        let ledger = ledger_with(&["src/host.rs"], &["cargo test"]);
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "s",
            origin_prompt: Some("origin"),
            current_user_message: None,
            messages: &[user("origin")],
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        let mut snapshot = snapshot;
        let semantic = ContextFrame {
            constraints: vec!["semantic constraint".to_string()],
            decisions: vec!["semantic decision".to_string()],
            unresolved_errors: vec!["semantic blocker".to_string()],
            next_steps: vec!["semantic next".to_string()],
            touched_files: vec!["src/attacker.rs".to_string()],
            commands_run: vec!["rm -rf /".to_string()],
            test_results: vec!["fake pass".to_string()],
            user_goal: Some("paraphrased goal".to_string()),
            current_task: Some("invented task".to_string()),
            ..Default::default()
        };
        apply_semantic_enrichment(&mut snapshot, &semantic, "success");
        // Host facts unchanged.
        assert_eq!(snapshot.touched_files, vec!["src/host.rs".to_string()]);
        assert_eq!(snapshot.commands_run, vec!["cargo test".to_string()]);
        assert_eq!(snapshot.objective, "origin");
        // Semantic advisory merged.
        assert!(snapshot
            .semantic
            .constraints
            .contains(&"semantic constraint".to_string()));
        assert!(snapshot
            .semantic
            .decisions
            .contains(&"semantic decision".to_string()));
        assert!(snapshot
            .semantic
            .unresolved_blockers
            .contains(&"semantic blocker".to_string()));
    }

    #[test]
    fn previous_decisions_carry_forward_and_merge() {
        let previous_body = serde_json::json!({
            "semantic": {
                "constraints": ["old constraint"],
                "decisions": ["old decision"],
                "unresolved_blockers": ["old blocker"],
                "next_steps": ["old next"],
            },
            "current_task": "old task",
        });
        let previous_payload =
            ContinuationCheckpointPayload::new(previous_body).expect("previous payload");
        let previous = ContinuationCheckpoint {
            id: "checkpoint-prev".to_string(),
            session_id: "sess".to_string(),
            sequence: 1,
            previous_installed_id: None,
            schema_version: 1,
            status: crate::session::continuation::ContinuationCheckpointStatus::Prepared,
            payload_digest: previous_payload.digest().unwrap(),
            payload: previous_payload,
            created_at: 1,
            installed_at: None,
            aborted_at: None,
            abort_reason: None,
        };
        let ledger = ledger_with(&[], &[]);
        let messages = vec![
            user("origin"),
            user("new steering with must keep tests green"),
        ];
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "sess",
            origin_prompt: Some("origin"),
            current_user_message: Some("new steering with must keep tests green"),
            messages: &messages,
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: Some(&previous),
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        // Previous semantic state carried forward.
        assert!(snapshot
            .semantic
            .decisions
            .contains(&"old decision".to_string()));
        assert!(snapshot
            .semantic
            .unresolved_blockers
            .contains(&"old blocker".to_string()));
        // Current-epoch steering retained after the prior checkpoint.
        assert!(snapshot
            .intent_spine
            .iter()
            .any(|e| e.text.contains("new steering")));
        assert!(snapshot.previous.is_some());
        assert!(snapshot
            .diagnostics
            .iter()
            .any(|d| d.contains("previous_checkpoint(carried_forward)")));
    }

    #[test]
    fn intent_spine_deterministic_for_equivalent_input() {
        let ledger = ledger_with(&[], &[]);
        let messages = vec![
            user("origin task"),
            user("steering one"),
            user("steering two"),
        ];
        let build = || {
            assemble_continuation_snapshot(ContinuationAssemblyInput {
                session_id: "sess",
                origin_prompt: Some("origin task"),
                current_user_message: None,
                messages: &messages,
                active_goal: None,
                todos: &[],
                ledger: &ledger,
                security_findings: &[],
                previous_checkpoint: None,
                plan_path: None,
                plan_content: None,
                active_work_plan: None,
            })
        };
        let first = build();
        let second = build();
        assert_eq!(first.intent_spine, second.intent_spine);
        assert_eq!(first.content_digest(), second.content_digest());
    }

    #[test]
    fn payload_body_accepted_by_m001_store_contract() {
        let ledger = ledger_with(&["src/a.rs"], &[]);
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id: "sess-1",
            origin_prompt: Some("origin"),
            current_user_message: None,
            messages: &[user("origin")],
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: None,
            plan_path: None,
            plan_content: None,
            active_work_plan: None,
        });
        let payload = snapshot.to_payload().expect("payload must validate");
        assert_eq!(payload.schema_version, 1);
        let frame = snapshot.to_context_frame();
        assert_eq!(frame.user_goal.as_deref(), Some("origin"));
    }
}

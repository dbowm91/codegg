//! Release-gate evaluation and materialization policy for Project Work
//! Orders M002.
//!
//! Pure, UI/server/plugin/auth-free policy: deterministic gate latches,
//! finite-repeat bounds, sequence-hold rules, workspace-policy resolution,
//! model/policy narrowing, and deterministic idempotency keys. Durable
//! state transitions (CAS claim, latch persistence, linkage) live in
//! [`super::store::WorkOrderService`]; daemon wiring lives in
//! `src/core/work_order_coordinator.rs`. This module never creates a
//! session row, submits a job, allocates a worktree, or invokes a model.
//!
//! Long-term references: ADR-0005
//! (`plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`),
//! `plans/subsystems/project-work-orders-task-view-roadmap.md` M002.

use std::collections::BTreeSet;

use super::model::{
    ApprovalRequest, AttentionCode, GateJoin, GateKind, LaneFailurePolicy, OccurrenceState,
    ReleaseGateSet, WorkOrder, WorkOrderOccurrence, WorkspacePolicy, MAX_IDEMPOTENCY_KEY_LEN,
};

// ── Gate evaluation ────────────────────────────────────────────────────

/// Result of evaluating one occurrence's gate set at one wall-clock instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateEvaluation {
    /// `true` when the join policy is satisfied (occurrence may become ready).
    pub satisfied: bool,
    /// Gate kinds newly satisfied at this instant (caller should latch).
    pub newly_latched: Vec<GateKind>,
    /// Next wall-clock instant the coordinator should wake for this
    /// occurrence (`None` means no timer wake is required).
    pub next_check_at_ms: Option<i64>,
    /// Gate kinds still unsatisfied (diagnostics only).
    pub missing: Vec<GateKind>,
}

/// Delay deadline for one occurrence.
///
/// First occurrence anchors to work-order creation/activation; repeated
/// occurrences anchor to the prior terminal/release timestamp per repeat
/// policy. The caller persists the calculated deadline (in
/// `occurrence.not_before_ms` for delay gates) so restart does not
/// recalculate against a new clock origin.
pub fn delay_deadline_for_occurrence(
    work_order_created_at_ms: i64,
    occurrence_index: u64,
    prior_terminal_ms: Option<i64>,
    delay_secs: i64,
) -> i64 {
    let anchor = if occurrence_index == 0 {
        work_order_created_at_ms
    } else {
        prior_terminal_ms.unwrap_or(work_order_created_at_ms)
    };
    anchor.saturating_add(delay_secs.saturating_mul(1000))
}

/// Evaluate one occurrence's gate set.
///
/// `sequence_ready` is the caller-evaluated lane condition (all earlier lane
/// members required by the lane continuation policy are terminal/successful
/// enough to advance). Latches already recorded on the occurrence are
/// honoured: clock going backwards never un-satisfies a latched gate, and
/// once an occurrence has moved to claiming/terminal state later gate
/// changes cannot create another execution (enforced by the store CAS).
pub fn evaluate_occurrence_gates(
    gates: &ReleaseGateSet,
    occurrence: &WorkOrderOccurrence,
    work_order_created_at_ms: i64,
    prior_terminal_ms: Option<i64>,
    now_ms: i64,
    sequence_ready: bool,
) -> GateEvaluation {
    let latched: BTreeSet<GateKind> = occurrence.gate_latches.iter().copied().collect();
    let mut newly_latched = Vec::new();
    let mut missing = Vec::new();
    let mut next_check: Option<i64> = None;
    let mut per_gate_satisfied = Vec::with_capacity(gates.gates.len());

    for gate in &gates.gates {
        // A latched gate stays satisfied even if the clock moved backwards.
        if latched.contains(&gate.kind) {
            per_gate_satisfied.push(true);
            continue;
        }
        match gate.kind {
            GateKind::Immediate => {
                per_gate_satisfied.push(true);
                newly_latched.push(GateKind::Immediate);
            }
            GateKind::Delay => {
                let delay_secs = gate.delay_secs.unwrap_or(0);
                // Persisted deadline wins over recalculation so restart
                // cannot shift the release instant.
                let deadline = occurrence.not_before_ms.unwrap_or_else(|| {
                    delay_deadline_for_occurrence(
                        work_order_created_at_ms,
                        occurrence.occurrence_index,
                        prior_terminal_ms,
                        delay_secs,
                    )
                });
                if now_ms >= deadline {
                    per_gate_satisfied.push(true);
                    newly_latched.push(GateKind::Delay);
                } else {
                    per_gate_satisfied.push(false);
                    missing.push(GateKind::Delay);
                    next_check = Some(next_check.map_or(deadline, |prev: i64| prev.min(deadline)));
                }
            }
            GateKind::NotBefore => {
                let at = gate.not_before_ms.unwrap_or(i64::MAX);
                if now_ms >= at {
                    per_gate_satisfied.push(true);
                    newly_latched.push(GateKind::NotBefore);
                } else {
                    per_gate_satisfied.push(false);
                    missing.push(GateKind::NotBefore);
                    next_check = Some(next_check.map_or(at, |prev: i64| prev.min(at)));
                }
            }
            GateKind::SequenceReady => {
                if sequence_ready {
                    per_gate_satisfied.push(true);
                    newly_latched.push(GateKind::SequenceReady);
                } else {
                    per_gate_satisfied.push(false);
                    missing.push(GateKind::SequenceReady);
                }
            }
            GateKind::ExternalTrigger => {
                // M002 understands the latch field but exposes no network
                // endpoint (M005). Tests satisfy it through the internal
                // latch service method; an unlatched trigger never
                // self-satisfies.
                per_gate_satisfied.push(false);
                missing.push(GateKind::ExternalTrigger);
            }
        }
    }

    let satisfied = match gates.join {
        GateJoin::All => per_gate_satisfied.iter().all(|s| *s),
        GateJoin::Any => per_gate_satisfied.iter().any(|s| *s),
    };
    // A satisfied occurrence needs no further timer wake; an unsatisfied
    // one wakes at the earliest persisted deadline (if any).
    let next_check_at_ms = if satisfied { None } else { next_check };
    // Only report newly-latched gates when the caller will actually latch
    // them (i.e. the occurrence is still pre-claim). The store enforces
    // the monotonic claim boundary.
    GateEvaluation {
        satisfied,
        newly_latched: if satisfied { newly_latched } else { Vec::new() },
        next_check_at_ms,
        missing,
    }
}

/// Merge newly satisfied gate kinds into the persisted latch set.
pub fn merge_latches(current: &[GateKind], newly_latched: &[GateKind]) -> Vec<GateKind> {
    let mut set: BTreeSet<GateKind> = current.iter().copied().collect();
    for kind in newly_latched {
        set.insert(*kind);
    }
    set.into_iter().collect()
}

// ── Sequence ───────────────────────────────────────────────────────────

/// `true` when downstream work must hold: any predecessor required by the
/// lane continuation policy is not successful enough to advance.
///
/// Default (`HoldLane`) holds on failed/cancelled/needs-attention
/// predecessors. `ContinueLane` advances regardless of predecessor outcome
/// (terminal predecessors never block).
pub fn sequence_holds(
    predecessor_states: &[OccurrenceState],
    failure_policy: LaneFailurePolicy,
) -> bool {
    match failure_policy {
        LaneFailurePolicy::ContinueLane => false,
        LaneFailurePolicy::HoldLane => predecessor_states.iter().any(|state| {
            matches!(
                state,
                OccurrenceState::Failed
                    | OccurrenceState::Cancelled
                    | OccurrenceState::NeedsAttention
            )
        }),
    }
}

/// `true` when every predecessor occurrence is terminal (completed, failed,
/// or cancelled). Needs-attention is non-terminal and holds the lane until
/// an authorized actor retries, skips, or advances.
pub fn sequence_predecessors_terminal(predecessor_states: &[OccurrenceState]) -> bool {
    predecessor_states.iter().all(|state| {
        matches!(
            state,
            OccurrenceState::Completed | OccurrenceState::Failed | OccurrenceState::Cancelled
        )
    })
}

// ── Repeat ─────────────────────────────────────────────────────────────

/// `true` when the work order has exhausted its finite repeat budget.
pub fn is_repeat_exhausted(existing_occurrences: u64, repeat_count: u32) -> bool {
    existing_occurrences >= u64::from(repeat_count)
}

/// Next 0-based occurrence index for a work order with `existing`
/// occurrences already recorded.
pub fn next_occurrence_index(existing_occurrences: u64) -> u64 {
    existing_occurrences
}

// ── Idempotency keys ───────────────────────────────────────────────────

/// Deterministic canonical session id for one occurrence.
///
/// The title remains a bounded human label and is never identity; this key
/// is the durable exactly-once identity used with
/// `SessionStore::create_with_id` recovery (query-before-create on restart).
pub fn session_id_for_occurrence(occurrence_id: &str) -> String {
    truncate_key(&format!("wo-session-{occurrence_id}"))
}

/// Deterministic initial-turn submission key for one occurrence/session
/// pair, used with `JobSubmissionService::submit` recovery.
pub fn submission_key_for_occurrence(occurrence_id: &str) -> String {
    truncate_key(&format!("wo-turn-{occurrence_id}"))
}

fn truncate_key(raw: &str) -> String {
    let mut key: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if key.len() > MAX_IDEMPOTENCY_KEY_LEN {
        key.truncate(MAX_IDEMPOTENCY_KEY_LEN);
    }
    key
}

// ── Workspace policy ───────────────────────────────────────────────────

/// Resolved workspace action for one occurrence claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAction {
    /// Acquire a managed worktree for mutation-capable Git work.
    UseManagedWorktree,
    /// Share the project workspace (safe for read-only work).
    ShareReadOnly,
    /// Share the project workspace with scheduler contention ensuring one
    /// writer (mutation where isolation is unavailable or explicitly
    /// serialized).
    ShareSerialized,
    /// Cannot proceed safely; caller must record attention.
    NeedsAttention {
        code: AttentionCode,
        diagnostic: String,
    },
}

/// Resolve the requested workspace policy to one concrete action.
///
/// `is_git` reports whether the project root is a Git repository;
/// `is_mutation` reports whether the task is mutation-capable. Non-Git
/// mutation is never mislabeled as isolated: it serializes or requires
/// attention depending on the requested policy.
pub fn resolve_workspace_action(
    requested: Option<WorkspacePolicy>,
    is_git: bool,
    is_mutation: bool,
) -> WorkspaceAction {
    let policy = requested.unwrap_or(WorkspacePolicy::AutoIsolated);
    match (policy, is_git, is_mutation) {
        // Mutation-capable Git work defaults to managed-worktree isolation,
        // allocated lazily at claim/start (never at authoring time).
        (WorkspacePolicy::AutoIsolated, true, true) => WorkspaceAction::UseManagedWorktree,
        // Read-only work may share safely under existing leases/policy.
        (WorkspacePolicy::AutoIsolated, _, false) => WorkspaceAction::ShareReadOnly,
        (WorkspacePolicy::Shared, _, false) => WorkspaceAction::ShareReadOnly,
        // Shared mutation serializes through scheduler contention (one
        // writer); never claims worktree-equivalent isolation.
        (WorkspacePolicy::Shared, _, true) => WorkspaceAction::ShareSerialized,
        (WorkspacePolicy::Serialized, _, _) => WorkspaceAction::ShareSerialized,
        // Non-Git mutation has no copy/overlay isolation backend in M002:
        // fail closed to attention rather than copying directories ad hoc.
        (WorkspacePolicy::AutoIsolated, false, true) => WorkspaceAction::NeedsAttention {
            code: AttentionCode::MaterializationFailed,
            diagnostic: "isolation_unavailable: non-Git mutation has no managed-worktree backend; use serialized sharing or resolve to a Git repository"
                .to_owned(),
        },
    }
}

// ── Model / policy narrowing ───────────────────────────────────────────

/// Resolve the requested stable model identity against the current catalog.
///
/// Returns the effective model (`None` means daemon default) or an
/// attention code. Removed/unavailable models never silently fall back.
pub fn resolve_model(
    requested: Option<&str>,
    is_available: &dyn Fn(&str) -> bool,
) -> Result<Option<String>, AttentionCode> {
    match requested {
        None => Ok(None),
        Some(model) if is_available(model) => Ok(Some(model.to_owned())),
        Some(_) => Err(AttentionCode::ModelUnavailable),
    }
}

fn approval_rank(requested: ApprovalRequest) -> u8 {
    match requested {
        ApprovalRequest::Interactive => 0,
        ApprovalRequest::Automatic => 1,
        ApprovalRequest::Yolo => 2,
    }
}

fn sandbox_rank(requested: super::model::SandboxRequest) -> u8 {
    match requested {
        super::model::SandboxRequest::ReadOnly => 0,
        super::model::SandboxRequest::WorkspaceWrite => 1,
        super::model::SandboxRequest::FullHost => 2,
    }
}

/// Narrow the requested approval snapshot against the current ceiling.
///
/// Returns `(effective, narrowed)`. Later preference changes never widen an
/// existing work order: the effective mode is always the minimum of the
/// snapshot and the ceiling.
pub fn narrow_approval(
    requested: Option<ApprovalRequest>,
    ceiling_rank: Option<u8>,
) -> (Option<ApprovalRequest>, bool) {
    let Some(requested) = requested else {
        return (None, false);
    };
    let Some(ceiling) = ceiling_rank else {
        return (Some(requested), false);
    };
    if approval_rank(requested) > ceiling {
        let narrowed = match ceiling {
            0 => ApprovalRequest::Interactive,
            1 => ApprovalRequest::Automatic,
            _ => ApprovalRequest::Yolo,
        };
        (Some(narrowed), true)
    } else {
        (Some(requested), false)
    }
}

/// Narrow the requested sandbox snapshot against the current ceiling.
pub fn narrow_sandbox(
    requested: Option<super::model::SandboxRequest>,
    ceiling_rank: Option<u8>,
) -> (Option<super::model::SandboxRequest>, bool) {
    let Some(requested) = requested else {
        return (None, false);
    };
    let Some(ceiling) = ceiling_rank else {
        return (Some(requested), false);
    };
    if sandbox_rank(requested) > ceiling {
        let narrowed = match ceiling {
            0 => super::model::SandboxRequest::ReadOnly,
            1 => super::model::SandboxRequest::WorkspaceWrite,
            _ => super::model::SandboxRequest::FullHost,
        };
        (Some(narrowed), true)
    } else {
        (Some(requested), false)
    }
}

// ── Occurrence helpers ─────────────────────────────────────────────────

/// `true` when the occurrence is still pre-claim (gate changes may still
/// latch). Once claiming/terminal, later gate changes cannot create another
/// execution.
pub fn is_pre_claim(state: OccurrenceState) -> bool {
    matches!(state, OccurrenceState::Waiting | OccurrenceState::Ready)
}

/// Diagnostic hint for why an occurrence is still waiting.
pub fn waiting_diagnostic(_work_order: &WorkOrder, evaluation: &GateEvaluation) -> Option<String> {
    if evaluation.satisfied || evaluation.missing.is_empty() {
        return None;
    }
    let kinds: Vec<&str> = evaluation.missing.iter().map(|k| k.as_str()).collect();
    let mut text = format!("waiting on {}", kinds.join(", "));
    if text.len() > super::model::MAX_DIAGNOSTIC_CHARS {
        text.truncate(super::model::MAX_DIAGNOSTIC_CHARS);
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        GateJoin, GateKind, GateSpec, LaneFailurePolicy, OccurrenceState, ReleaseGateSet,
    };
    use super::*;
    use crate::identity::{ProjectId, WorkOrderId, WorkOrderOccurrenceId};

    fn occurrence_with_latches(latches: Vec<GateKind>) -> WorkOrderOccurrence {
        WorkOrderOccurrence {
            id: WorkOrderOccurrenceId::new(),
            work_order_id: WorkOrderId::new(),
            project_id: ProjectId::parse("project-1").unwrap(),
            occurrence_index: 0,
            state: OccurrenceState::Waiting,
            gate_latches: latches,
            not_before_ms: None,
            next_check_at_ms: None,
            session_id: None,
            job_id: None,
            workspace_id: None,
            worktree_id: None,
            attention_code: None,
            diagnostic: None,
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            claimed_at_ms: None,
            started_at_ms: None,
            terminal_at_ms: None,
        }
    }

    fn delay_set(secs: i64) -> ReleaseGateSet {
        ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::Delay,
                delay_secs: Some(secs),
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            }],
        }
    }

    #[test]
    fn immediate_gate_satisfies_at_once() {
        let set = ReleaseGateSet::immediate();
        let occurrence = occurrence_with_latches(vec![]);
        let eval = evaluate_occurrence_gates(&set, &occurrence, 1_000, None, 1_000, false);
        assert!(eval.satisfied);
        assert_eq!(eval.newly_latched, vec![GateKind::Immediate]);
        assert_eq!(eval.next_check_at_ms, None);
    }

    #[test]
    fn delay_deadline_anchors_first_and_repeat_occurrences() {
        assert_eq!(delay_deadline_for_occurrence(1_000, 0, None, 60), 61_000);
        // Repeats anchor to the prior terminal/release timestamp.
        assert_eq!(
            delay_deadline_for_occurrence(1_000, 1, Some(5_000), 60),
            65_000
        );
        // Missing prior terminal falls back to creation (fail closed to a
        // deterministic instant, never to "now").
        assert_eq!(delay_deadline_for_occurrence(1_000, 2, None, 60), 61_000);
    }

    #[test]
    fn delay_persists_deadline_across_reopen() {
        // First evaluation computes the deadline; the caller persists it in
        // not_before_ms. A later reopen must reuse the persisted instant
        // rather than recalculating against a new clock origin.
        let set = delay_set(60);
        let first = occurrence_with_latches(vec![]);
        let eval = evaluate_occurrence_gates(&set, &first, 1_000, None, 1_000, false);
        assert!(!eval.satisfied);
        assert_eq!(eval.next_check_at_ms, Some(61_000));
        let mut reopened = first.clone();
        reopened.not_before_ms = Some(61_000);
        let eval = evaluate_occurrence_gates(&set, &reopened, 9_999, None, 30_000, false);
        assert!(!eval.satisfied);
        assert_eq!(eval.next_check_at_ms, Some(61_000));
        let eval = evaluate_occurrence_gates(&set, &reopened, 9_999, None, 61_000, false);
        assert!(eval.satisfied);
    }

    #[test]
    fn not_before_latches_and_survives_backward_clock() {
        let set = ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::NotBefore,
                delay_secs: None,
                not_before_ms: Some(10_000),
                lane_id: None,
                trigger_ref: None,
            }],
        };
        let waiting = occurrence_with_latches(vec![]);
        let eval = evaluate_occurrence_gates(&set, &waiting, 1_000, None, 5_000, false);
        assert!(!eval.satisfied);
        assert_eq!(eval.next_check_at_ms, Some(10_000));
        let eval = evaluate_occurrence_gates(&set, &waiting, 1_000, None, 10_000, false);
        assert!(eval.satisfied);
        // Once latched, clock going backwards must not un-satisfy the gate.
        let latched = occurrence_with_latches(vec![GateKind::NotBefore]);
        let eval = evaluate_occurrence_gates(&set, &latched, 1_000, None, 1_000, false);
        assert!(eval.satisfied);
    }

    #[test]
    fn all_any_joins_combine_enabled_gates() {
        let both = ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![
                GateSpec {
                    kind: GateKind::NotBefore,
                    delay_secs: None,
                    not_before_ms: Some(10_000),
                    lane_id: None,
                    trigger_ref: None,
                },
                GateSpec {
                    kind: GateKind::SequenceReady,
                    delay_secs: None,
                    not_before_ms: None,
                    lane_id: None,
                    trigger_ref: None,
                },
            ],
        };
        let waiting = occurrence_with_latches(vec![]);
        // All requires both; time satisfied but sequence not.
        let eval = evaluate_occurrence_gates(&both, &waiting, 1_000, None, 20_000, false);
        assert!(!eval.satisfied);
        let eval = evaluate_occurrence_gates(&both, &waiting, 1_000, None, 20_000, true);
        assert!(eval.satisfied);
        let any = ReleaseGateSet {
            join: GateJoin::Any,
            ..both.clone()
        };
        let eval = evaluate_occurrence_gates(&any, &waiting, 1_000, None, 20_000, false);
        assert!(eval.satisfied);
    }

    #[test]
    fn sequence_default_holds_on_failure_and_attention() {
        for state in [
            OccurrenceState::Failed,
            OccurrenceState::Cancelled,
            OccurrenceState::NeedsAttention,
        ] {
            assert!(sequence_holds(&[state], LaneFailurePolicy::HoldLane));
        }
        assert!(!sequence_holds(
            &[OccurrenceState::Completed],
            LaneFailurePolicy::HoldLane
        ));
        // ContinueLane advances regardless.
        assert!(!sequence_holds(
            &[OccurrenceState::Failed],
            LaneFailurePolicy::ContinueLane
        ));
        assert!(sequence_predecessors_terminal(&[
            OccurrenceState::Completed,
            OccurrenceState::Failed,
        ]));
        assert!(!sequence_predecessors_terminal(&[
            OccurrenceState::Completed,
            OccurrenceState::NeedsAttention,
        ]));
    }

    #[test]
    fn repeat_budget_is_finite_and_exhausts_exactly() {
        assert!(is_repeat_exhausted(3, 3));
        assert!(!is_repeat_exhausted(2, 3));
        assert_eq!(next_occurrence_index(2), 2);
    }

    #[test]
    fn idempotency_keys_are_deterministic_and_bounded() {
        let a = session_id_for_occurrence("abc123");
        let b = session_id_for_occurrence("abc123");
        assert_eq!(a, b);
        assert!(a.len() <= MAX_IDEMPOTENCY_KEY_LEN);
        assert_ne!(a, submission_key_for_occurrence("abc123"));
        assert_ne!(
            session_id_for_occurrence("abc123"),
            session_id_for_occurrence("xyz789")
        );
    }

    #[test]
    fn workspace_policy_never_claims_non_git_isolation() {
        assert_eq!(
            resolve_workspace_action(None, true, true),
            WorkspaceAction::UseManagedWorktree
        );
        assert_eq!(
            resolve_workspace_action(None, true, false),
            WorkspaceAction::ShareReadOnly
        );
        assert_eq!(
            resolve_workspace_action(Some(WorkspacePolicy::Serialized), true, true),
            WorkspaceAction::ShareSerialized
        );
        match resolve_workspace_action(None, false, true) {
            WorkspaceAction::NeedsAttention { diagnostic, .. } => {
                assert!(diagnostic.contains("isolation_unavailable"));
            }
            other => panic!("non-Git mutation must need attention, got {other:?}"),
        }
    }

    #[test]
    fn removed_model_never_silently_falls_back() {
        let available = |model: &str| model == "openai/gpt-5";
        assert_eq!(resolve_model(None, &available).unwrap(), None);
        assert_eq!(
            resolve_model(Some("openai/gpt-5"), &available).unwrap(),
            Some("openai/gpt-5".to_owned())
        );
        assert_eq!(
            resolve_model(Some("removed/model"), &available).unwrap_err(),
            AttentionCode::ModelUnavailable
        );
    }

    #[test]
    fn approval_and_sandbox_narrow_but_never_widen() {
        let (effective, narrowed) = narrow_approval(Some(ApprovalRequest::Yolo), Some(0));
        assert_eq!(effective, Some(ApprovalRequest::Interactive));
        assert!(narrowed);
        let (effective, narrowed) = narrow_approval(Some(ApprovalRequest::Interactive), Some(2));
        assert_eq!(effective, Some(ApprovalRequest::Interactive));
        assert!(!narrowed);
    }
}

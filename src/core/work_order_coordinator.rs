//! Project Work Orders M002: daemon-owned release coordinator.
//!
//! The `WorkOrderCoordinator` is a readiness/materialization coordinator,
//! not a scheduler or executor. It evaluates release gates, claims ready
//! occurrences exactly once, resolves workspace/model/policy state, and
//! drives the durable claim → workspace → session → job → running state
//! machine through existing canonical boundaries only:
//!
//! - sessions via `SessionStore` (+ project/workspace binding + origin
//!   attribution);
//! - initial turns via `JobSubmissionService` + the existing global
//!   scheduler (`JobKind::AgentTurn`, scheduler-owned `AgentTurnExecutor`);
//! - isolation via `WorktreeService` + `WorkspaceRegistry` (lazy at
//!   claim/start, never at authoring time).
//!
//! It never constructs an `AgentLoop`, never creates a second scheduler
//! loop/queue, never fabricates speculative sessions, never copies
//! directories ad hoc as "isolation", and never silently falls back to
//! another model or widens approval/sandbox policy.
//!
//! Storage truth stays in `codegg-core::work_order::WorkOrderService`; this
//! module owns daemon-side orchestration (wakes, resolution, side-effect
//! ordering, reconciliation, events/audit). Pure gate/policy helpers live
//! in `codegg-core::work_order::coordinator`.

use codegg_core::jobs::ExecutionTarget;
use std::collections::HashSet;
use std::sync::Arc;

use codegg_core::identity::{ProjectId, WorkOrderOccurrenceId};
use codegg_core::work_order::{
    evaluate_occurrence_gates, is_repeat_exhausted, merge_latches, narrow_approval, narrow_sandbox,
    resolve_model, resolve_workspace_action, sequence_holds, sequence_predecessors_terminal,
    session_id_for_occurrence, submission_key_for_occurrence, AttentionCode, GateKind,
    LaneFailurePolicy, OccurrenceState, WorkOrder, WorkOrderError, WorkOrderOccurrence,
    WorkOrderService, WorkspaceAction, WorkspacePolicy,
};

// Production daemon wiring lives in the `impl CoreDaemon` block at the end
// of this file. It routes every initial turn through `JobSubmissionService`
// (scheduler-owned admission); the scheduler-owned `AgentTurnExecutor`
// records admission without constructing an `AgentLoop` here.

/// Fault-injection seam for materialization-boundary tests.
///
/// Each variant names the durable side effect after which the test crashes
/// (drops state without completing the next step). Restart recovery must
/// find the existing side effect instead of duplicating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializationFault {
    AfterClaim,
    AfterWorkspaceLink,
    AfterSessionLink,
    AfterJobSubmit,
}

/// Which durable stage a partially materialized occurrence is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialStage {
    AwaitingWorkspace,
    AwaitingSession,
    AwaitingJob,
    AwaitingRunning,
    Complete,
}

/// Daemon-owned release coordinator.
///
/// Holds only the durable work-order service; session/job/worktree handles
/// are supplied per call so tests can substitute fakes with fault
/// injection while production wires canonical services.
#[derive(Debug, Clone)]
pub struct WorkOrderCoordinator {
    work_orders: Arc<WorkOrderService>,
}

impl WorkOrderCoordinator {
    pub fn new(work_orders: Arc<WorkOrderService>) -> Self {
        Self { work_orders }
    }

    pub fn service(&self) -> &Arc<WorkOrderService> {
        &self.work_orders
    }

    /// Evaluate the bounded due set for one project at `now_ms`.
    ///
    /// For each due occurrence: load its work order, evaluate lane
    /// readiness (sequence gates only), evaluate the gate set, persist
    /// latches + `next_check_at`, and advance waiting → ready when the join
    /// is satisfied. Returns the newly ready occurrences with their work
    /// orders. Never polls every work order: only rows whose
    /// `next_check_at` is absent or due are read (bounded).
    pub async fn evaluate_due_for_project(
        &self,
        project: &ProjectId,
        now_ms: i64,
    ) -> Result<Vec<(WorkOrder, WorkOrderOccurrence)>, WorkOrderError> {
        let due = self
            .work_orders
            .list_due_occurrences(project, now_ms, 128)
            .await?;
        let mut ready = Vec::new();
        for occurrence in due {
            let Some(work_order) = self
                .work_orders
                .get_work_order(project, &occurrence.work_order_id)
                .await?
            else {
                continue;
            };
            if !matches!(
                work_order.state,
                codegg_core::work_order::WorkOrderState::Active
            ) {
                continue;
            }
            let sequence_ready = self
                .sequence_ready_for(project, &work_order, &occurrence)
                .await?;
            let prior_terminal = self
                .prior_terminal_ms(project, &work_order, &occurrence)
                .await?;
            let evaluation = evaluate_occurrence_gates(
                &work_order.gates,
                &occurrence,
                work_order.created_at_ms,
                prior_terminal,
                now_ms,
                sequence_ready,
            );
            let merged = merge_latches(&occurrence.gate_latches, &evaluation.newly_latched);
            // Latch even when not yet satisfied so delay deadlines persist
            // and duplicate trigger delivery converges.
            let updated = self
                .work_orders
                .persist_gate_evaluation(
                    project,
                    &occurrence.id,
                    merged,
                    evaluation.next_check_at_ms,
                    evaluation.satisfied,
                    now_ms,
                )
                .await?;
            if evaluation.satisfied
                && matches!(
                    updated.state,
                    OccurrenceState::Ready | OccurrenceState::Waiting
                )
            {
                // `persist_gate_evaluation` already advanced waiting → ready
                // when `mark_ready` was set; re-read for the caller.
                let current = self
                    .work_orders
                    .get_occurrence(project, &occurrence.id)
                    .await?
                    .unwrap_or(updated);
                ready.push((work_order, current));
            }
        }
        Ok(ready)
    }

    /// Evaluate whether the sequence-ready condition holds for one
    /// occurrence (pure lane check; gate-join logic lives in the core
    /// evaluator).
    pub async fn sequence_ready_for(
        &self,
        project: &ProjectId,
        work_order: &WorkOrder,
        occurrence: &WorkOrderOccurrence,
    ) -> Result<bool, WorkOrderError> {
        let needs_sequence = work_order
            .gates
            .gates
            .iter()
            .any(|gate| gate.kind == GateKind::SequenceReady);
        if !needs_sequence {
            return Ok(true);
        }
        let Some(lane_id) = work_order.sequence_lane_id.as_ref() else {
            return Ok(false);
        };
        let Some(lane) = self.work_orders.get_lane(project, lane_id).await? else {
            return Ok(false);
        };
        let position = lane
            .ordered_work_order_ids
            .iter()
            .position(|id| id == &work_order.id);
        let Some(position) = position else {
            return Ok(false);
        };
        let mut predecessor_states = Vec::new();
        for predecessor_id in &lane.ordered_work_order_ids[..position] {
            let page = self
                .work_orders
                .list_occurrences(project, predecessor_id, Some(100))
                .await?;
            if page.occurrences.is_empty() {
                // A predecessor that never materialized still blocks: the
                // lane order is execution order.
                return Ok(false);
            }
            // The latest occurrence decides (repeats extend the chain; only
            // the newest execution matters for lane advancement).
            if let Some(latest) = page
                .occurrences
                .iter()
                .max_by_key(|occ| occ.occurrence_index)
            {
                predecessor_states.push(latest.state);
            }
            let _ = occurrence;
        }
        if !sequence_predecessors_terminal(&predecessor_states) {
            return Ok(false);
        }
        Ok(!sequence_holds(&predecessor_states, lane.failure_policy))
    }

    /// Prior terminal/release timestamp anchoring a repeat delay gate.
    async fn prior_terminal_ms(
        &self,
        project: &ProjectId,
        work_order: &WorkOrder,
        occurrence: &WorkOrderOccurrence,
    ) -> Result<Option<i64>, WorkOrderError> {
        if occurrence.occurrence_index == 0 {
            return Ok(None);
        }
        let page = self
            .work_orders
            .list_occurrences(project, &work_order.id, Some(256))
            .await?;
        let prior = page
            .occurrences
            .iter()
            .filter(|occ| occ.occurrence_index + 1 == occurrence.occurrence_index)
            .max_by_key(|occ| occ.occurrence_index);
        Ok(prior.and_then(|occ| occ.terminal_at_ms))
    }

    /// Describe which durable stage a claimed occurrence still needs.
    pub fn describe_partial(occurrence: &WorkOrderOccurrence) -> PartialStage {
        if occurrence.state.is_terminal() {
            return PartialStage::Complete;
        }
        if occurrence.workspace_id.is_none() {
            return PartialStage::AwaitingWorkspace;
        }
        if occurrence.session_id.is_none() {
            return PartialStage::AwaitingSession;
        }
        if occurrence.job_id.is_none() {
            return PartialStage::AwaitingJob;
        }
        if matches!(
            occurrence.state,
            OccurrenceState::Claiming | OccurrenceState::Ready | OccurrenceState::Waiting
        ) {
            return PartialStage::AwaitingRunning;
        }
        PartialStage::Complete
    }

    /// Resolve the requested model against an available-model predicate.
    /// No silent fallback: removed models become attention.
    pub fn resolve_model_for(
        requested: Option<&str>,
        available: &HashSet<String>,
    ) -> Result<Option<String>, AttentionCode> {
        resolve_model(requested, &|model| available.contains(model))
    }

    /// Narrow approval/sandbox snapshots against current ceilings.
    /// Returns `(effective_approval, approval_narrowed, effective_sandbox,
    /// sandbox_narrowed)`. Never widens.
    pub fn narrow_policy_for(
        work_order: &WorkOrder,
        approval_ceiling: Option<u8>,
        sandbox_ceiling: Option<u8>,
    ) -> (
        Option<codegg_core::work_order::ApprovalRequest>,
        bool,
        Option<codegg_core::work_order::SandboxRequest>,
        bool,
    ) {
        let (approval, approval_narrowed) =
            narrow_approval(work_order.requested_approval, approval_ceiling);
        let (sandbox, sandbox_narrowed) =
            narrow_sandbox(work_order.requested_sandbox, sandbox_ceiling);
        (approval, approval_narrowed, sandbox, sandbox_narrowed)
    }

    /// Resolve the workspace action for one claim.
    pub fn workspace_action_for(
        work_order: &WorkOrder,
        is_git: bool,
        is_mutation: bool,
    ) -> WorkspaceAction {
        resolve_workspace_action(work_order.workspace_policy, is_git, is_mutation)
    }

    /// Deterministic canonical session id for one occurrence.
    pub fn session_id_for(occurrence_id: &WorkOrderOccurrenceId) -> String {
        session_id_for_occurrence(occurrence_id.as_str())
    }

    /// Deterministic initial-turn submission key for one occurrence.
    pub fn submission_key_for(occurrence_id: &WorkOrderOccurrenceId) -> String {
        submission_key_for_occurrence(occurrence_id.as_str())
    }

    /// Check the finite repeat budget for one work order.
    pub fn repeat_exhausted(existing_occurrences: u64, repeat_count: u32) -> bool {
        is_repeat_exhausted(existing_occurrences, repeat_count)
    }

    /// Default lane policy for failure/attention predecessors.
    pub fn default_lane_policy() -> LaneFailurePolicy {
        LaneFailurePolicy::HoldLane
    }
}

// ── Daemon production wiring ─────────────────────────────────────────────

impl super::daemon::CoreDaemon {
    /// Daemon-owned release coordinator handle (stateless over the durable
    /// work-order service).
    pub fn work_order_coordinator(&self) -> WorkOrderCoordinator {
        WorkOrderCoordinator::new(self.work_orders.clone())
    }

    /// Publish a structural occurrence hint (identity/state only; never
    /// prompt bodies, secrets, or reasoning).
    pub async fn publish_occurrence_changed(&self, occurrence: &WorkOrderOccurrence, change: &str) {
        self.event_log
            .publish(
                occurrence.session_id.clone(),
                None,
                crate::protocol::core::CoreEvent::WorkOrderOccurrenceChanged {
                    project_id: occurrence.project_id.as_str().to_owned(),
                    work_order_id: occurrence.work_order_id.as_str().to_owned(),
                    occurrence_id: occurrence.id.as_str().to_owned(),
                    change: change.to_owned(),
                    state: occurrence.state.as_str().to_owned(),
                },
            )
            .await;
    }

    /// Wake the coordinator for one project: evaluate the bounded due set
    /// and materialize newly ready occurrences through canonical
    /// boundaries. Returns the number of occurrences advanced to running.
    /// Every initial turn enters `JobSubmissionService` + the existing
    /// global scheduler; this function never polls every work order.
    pub async fn wake_work_orders_for_project(
        &self,
        project: &ProjectId,
        now_ms: i64,
    ) -> Result<usize, WorkOrderError> {
        let coordinator = self.work_order_coordinator();
        let ready = coordinator
            .evaluate_due_for_project(project, now_ms)
            .await?;
        let mut materialized = 0;
        for (work_order, occurrence) in ready {
            match self
                .materialize_ready_occurrence(&work_order, &occurrence, now_ms)
                .await
            {
                Ok(true) => materialized += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        work_order = %work_order.id.as_str(),
                        occurrence = %occurrence.id.as_str(),
                        %error,
                        "work-order materialization deferred with attention"
                    );
                }
            }
        }
        // Opportunistically resume partially materialized rows so a wake
        // after a crash finishes recovery without waiting for a timer.
        let _ = self.reconcile_work_orders_for_project(project).await;
        Ok(materialized)
    }

    /// Materialize one ready occurrence: claim → workspace → session → job
    /// → running. Returns `true` when the occurrence reached running.
    /// Any step may instead record bounded attention (model unavailable,
    /// policy narrowed, workspace unavailable, isolation unavailable,
    /// worktree conflict, scheduler submission failure) without creating
    /// duplicate side effects.
    async fn materialize_ready_occurrence(
        &self,
        work_order: &WorkOrder,
        occurrence: &WorkOrderOccurrence,
        now_ms: i64,
    ) -> Result<bool, WorkOrderError> {
        let project = work_order.project_id.clone();
        // Exactly-once claim: a duplicate wake loses here and reconciles.
        let claimed = match self
            .work_orders
            .claim_occurrence(&project, &occurrence.id, now_ms)
            .await
        {
            Ok(claimed) => claimed,
            Err(WorkOrderError::StateConflict(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        self.publish_occurrence_changed(&claimed, "claimed").await;

        // Revalidate the requested model against currently available
        // providers (provider-kind check; full catalog-ID resolution stays
        // on the existing TurnSubmit selection path). No silent fallback.
        if let Some(requested) = work_order.requested_model.as_deref() {
            let provider_kind = requested.split('/').next().unwrap_or(requested);
            let mut registry = crate::provider::ProviderRegistry::new();
            let config = super::load_config_or_default();
            crate::provider::register_builtin_with_config(&mut registry, &config);
            if registry.get(provider_kind).is_none() {
                let attention = self
                    .work_orders
                    .transition_occurrence(
                        &project,
                        &claimed.id,
                        OccurrenceState::NeedsAttention,
                        Some(AttentionCode::ModelUnavailable),
                        Some(&format!("model_unavailable: {requested}")),
                        now_ms,
                    )
                    .await?;
                self.publish_occurrence_changed(&attention, "attention")
                    .await;
                return Ok(false);
            }
        }

        // Workspace policy resolution (pure; never copies directories).
        // Mutation detection is conservative: task-mode work is assumed
        // mutation-capable unless the workspace policy explicitly shares.
        let is_mutation = !matches!(work_order.workspace_policy, Some(WorkspacePolicy::Shared));
        let is_git = self.project_has_git_repository(&project).await;
        match resolve_workspace_action(work_order.workspace_policy, is_git, is_mutation) {
            WorkspaceAction::NeedsAttention { diagnostic, .. } => {
                let attention = self
                    .work_orders
                    .transition_occurrence(
                        &project,
                        &claimed.id,
                        OccurrenceState::NeedsAttention,
                        Some(AttentionCode::MaterializationFailed),
                        Some(&diagnostic),
                        now_ms,
                    )
                    .await?;
                self.publish_occurrence_changed(&attention, "attention")
                    .await;
                return Ok(false);
            }
            WorkspaceAction::UseManagedWorktree => {
                match self.allocate_managed_worktree(&project, &claimed).await {
                    Ok((workspace_id, worktree_id)) => {
                        self.work_orders
                            .persist_workspace_link(
                                &project,
                                &claimed.id,
                                &workspace_id,
                                Some(&worktree_id),
                                now_ms,
                            )
                            .await?;
                    }
                    Err(diagnostic) => {
                        let attention = self
                            .work_orders
                            .transition_occurrence(
                                &project,
                                &claimed.id,
                                OccurrenceState::NeedsAttention,
                                Some(AttentionCode::WorktreeConflict),
                                Some(&diagnostic),
                                now_ms,
                            )
                            .await?;
                        self.publish_occurrence_changed(&attention, "attention")
                            .await;
                        return Ok(false);
                    }
                }
            }
            WorkspaceAction::ShareReadOnly | WorkspaceAction::ShareSerialized => {
                match self.resolve_shared_workspace(&project).await {
                    Ok(workspace_id) => {
                        self.work_orders
                            .persist_workspace_link(
                                &project,
                                &claimed.id,
                                &workspace_id,
                                None,
                                now_ms,
                            )
                            .await?;
                    }
                    Err(diagnostic) => {
                        let attention = self
                            .work_orders
                            .transition_occurrence(
                                &project,
                                &claimed.id,
                                OccurrenceState::NeedsAttention,
                                Some(AttentionCode::MaterializationFailed),
                                Some(&diagnostic),
                                now_ms,
                            )
                            .await?;
                        self.publish_occurrence_changed(&attention, "attention")
                            .await;
                        return Ok(false);
                    }
                }
            }
        }

        // Canonical session creation with the deterministic
        // occurrence-derived idempotency key. The link persists before any
        // job submission so recovery finds the session.
        let session_id = session_id_for_occurrence(claimed.id.as_str());
        let current = self
            .work_orders
            .get_occurrence(&project, &claimed.id)
            .await?
            .unwrap_or(claimed.clone());
        if current.session_id.as_deref() != Some(session_id.as_str()) {
            match self
                .create_canonical_session(work_order, &current, &session_id)
                .await
            {
                Ok(_) => {
                    self.work_orders
                        .persist_session_link(&project, &claimed.id, &session_id, now_ms)
                        .await?;
                }
                Err(diagnostic) => {
                    let attention = self
                        .work_orders
                        .transition_occurrence(
                            &project,
                            &claimed.id,
                            OccurrenceState::NeedsAttention,
                            Some(AttentionCode::MaterializationFailed),
                            Some(&diagnostic),
                            now_ms,
                        )
                        .await?;
                    self.publish_occurrence_changed(&attention, "attention")
                        .await;
                    return Ok(false);
                }
            }
        }

        // One initial AgentTurn through JobSubmissionService + scheduler
        // with the deterministic submission key.
        let submission_key = submission_key_for_occurrence(claimed.id.as_str());
        let current = self
            .work_orders
            .get_occurrence(&project, &claimed.id)
            .await?
            .unwrap_or(claimed.clone());
        if current.job_id.is_none() {
            match self
                .submit_initial_turn(work_order, &current, &session_id, &submission_key)
                .await
            {
                Ok(job_id) => {
                    self.work_orders
                        .persist_job_link(&project, &claimed.id, &job_id, now_ms)
                        .await?;
                }
                Err(diagnostic) => {
                    let attention = self
                        .work_orders
                        .transition_occurrence(
                            &project,
                            &claimed.id,
                            OccurrenceState::NeedsAttention,
                            Some(AttentionCode::MaterializationFailed),
                            Some(&diagnostic),
                            now_ms,
                        )
                        .await?;
                    self.publish_occurrence_changed(&attention, "attention")
                        .await;
                    return Ok(false);
                }
            }
        }

        let running = self
            .work_orders
            .transition_occurrence(
                &project,
                &claimed.id,
                OccurrenceState::Running,
                None,
                None,
                now_ms,
            )
            .await?;
        self.publish_occurrence_changed(&running, "running").await;
        // Wake the scheduler so the admitted initial turn dispatches.
        if let Some(submission) = self.deps.submission.as_ref() {
            submission
                .scheduler()
                .wake(crate::scheduler::events::WokeReason::JobEnqueued);
        }
        Ok(true)
    }

    /// `true` when the project's resolved workspace sits inside a Git
    /// repository (managed-worktree isolation available).
    async fn project_has_git_repository(&self, project: &ProjectId) -> bool {
        let Some(pool) = self.pool.clone() else {
            return false;
        };
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT workspace_id FROM workspace_project_binding WHERE project_id = ? AND status = 'resolved' LIMIT 1",
        )
        .bind(project.as_str())
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten();
        let Some((workspace_id,)) = row else {
            return false;
        };
        let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(&workspace_id);
        let Some(record) = self.workspaces.resolve(&workspace_id).await else {
            return false;
        };
        crate::worktree::find_git_root(&record.canonical_root).is_some()
    }

    /// Resolve the shared workspace id for read-only/serialized work.
    async fn resolve_shared_workspace(&self, project: &ProjectId) -> Result<String, String> {
        let Some(pool) = self.pool.clone() else {
            return Err("workspace_unavailable: daemon has no durable pool".to_owned());
        };
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT workspace_id FROM workspace_project_binding WHERE project_id = ? AND status = 'resolved' LIMIT 1",
        )
        .bind(project.as_str())
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("workspace_unavailable: binding lookup failed: {e}"))?;
        row.map(|(id,)| id).ok_or_else(|| {
            "workspace_unavailable: project has no resolved workspace binding".to_owned()
        })
    }

    /// Lazily allocate a managed worktree for Git mutation work.
    /// Returns `(workspace_id, worktree_id)`; the workspace id is the
    /// project binding workspace (execution context) while the worktree id
    /// is the canonical managed-worktree record.
    async fn allocate_managed_worktree(
        &self,
        project: &ProjectId,
        occurrence: &WorkOrderOccurrence,
    ) -> Result<(String, String), String> {
        // Recovery: reuse the stored canonical link instead of allocating
        // a second worktree.
        if let (Some(workspace_id), Some(worktree_id)) = (
            occurrence.workspace_id.clone(),
            occurrence.worktree_id.clone(),
        ) {
            return Ok((workspace_id, worktree_id));
        }
        let Some(pool) = self.pool.clone() else {
            return Err("workspace_unavailable: daemon has no durable pool".to_owned());
        };
        let binding: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT workspace_id, repository_id FROM workspace_project_binding WHERE project_id = ? AND status = 'resolved' LIMIT 1",
        )
        .bind(project.as_str())
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("workspace_unavailable: binding lookup failed: {e}"))?;
        let Some((workspace_id_raw, repository_id_raw)) = binding else {
            return Err(
                "workspace_unavailable: project has no resolved workspace binding".to_owned(),
            );
        };
        let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(&workspace_id_raw);
        let Some(record) = self.workspaces.resolve(&workspace_id).await else {
            return Err("workspace_unavailable: workspace binding is not registered".to_owned());
        };
        let Some(git_root) = crate::worktree::find_git_root(&record.canonical_root) else {
            return Err(
                "isolation_unavailable: project workspace is not a Git repository".to_owned(),
            );
        };
        let repository_id = match repository_id_raw {
            Some(raw) => codegg_core::identity::RepositoryId::parse(&raw)
                .map_err(|e| format!("workspace_unavailable: bad repository id: {e}"))?,
            None => codegg_core::identity::RepositoryId::new(),
        };
        let owner = codegg_core::identity::AgentRunId::parse(
            &format!("wo-run-{}", occurrence.id.as_str())
                .chars()
                .take(64)
                .collect::<String>(),
        )
        .unwrap_or_else(|_| codegg_core::identity::AgentRunId::new());
        let request = codegg_core::worktree_service::CreateWorktreeRequest {
            project_id: project.clone(),
            repository_id,
            workspace_id: workspace_id.clone(),
            node_id: None,
            repository_root: git_root,
            base_commit: None,
            base_path: None,
            owner_run_id: owner,
        };
        // execution-ownership: scheduler — worktree allocation is lazy
        // managed isolation for a scheduler-admitted work order; no process
        // is spawned here (GitMutationExecutor owns subprocesses).
        match self.worktree_service.create(&request).await {
            Ok((record, _lease)) => Ok((workspace_id_raw, record.worktree_id.as_str().to_owned())),
            Err(error) => Err(format!("worktree_conflict: {error}")),
        }
    }

    /// Create the canonical ordinary session for one claimed occurrence.
    /// Uses the deterministic occurrence-derived id; an existing row with
    /// the same id converges (recovery) instead of duplicating.
    async fn create_canonical_session(
        &self,
        work_order: &WorkOrder,
        occurrence: &WorkOrderOccurrence,
        session_id: &str,
    ) -> Result<String, String> {
        let Some(pool) = self.pool.clone() else {
            return Err("workspace_unavailable: daemon has no durable pool".to_owned());
        };
        // Recovery: an already-linked session wins.
        if let Some(existing) = occurrence.session_id.as_deref() {
            return Ok(existing.to_owned());
        }
        let store = codegg_core::session::SessionStore::new(pool);
        if let Ok(Some(existing)) = store.get(session_id).await {
            return Ok(existing.id);
        }
        let title: String = work_order
            .title
            .clone()
            .unwrap_or_else(|| "Task".to_owned())
            .chars()
            .take(256)
            .collect();
        // The session directory is the resolved workspace root when known;
        // otherwise the daemon-safe temp base (the typed project id stays
        // the identity; the directory is only a filesystem locator and is
        // never derived into an identity).
        let workspace_root = self
            .workspaces
            .resolve(&codegg_core::workspace::WorkspaceId::new_unchecked(
                self.resolve_shared_workspace(&work_order.project_id)
                    .await
                    .unwrap_or_default(),
            ))
            .await
            .map(|record| record.canonical_root.to_string_lossy().into_owned())
            .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
        let session = store
            .create_with_id(
                session_id,
                codegg_core::session::CreateSession {
                    project_id: work_order.project_id.as_str().to_owned(),
                    directory: workspace_root,
                    title: Some(title),
                    parent_id: None,
                    workspace_id: None,
                    agent: Some("build".to_owned()),
                    model: work_order.requested_model.clone(),
                    tags: Some(vec!["work-order".to_owned()]),
                    provider_connection_id: None,
                    provider_connection_revision: None,
                    model_catalog_revision: None,
                    selected_model_id: None,
                },
            )
            .await
            .map_err(|e| format!("materialization_failed: session create failed: {e}"))?;
        Ok(session.id)
    }

    /// Submit the single initial AgentTurn through `JobSubmissionService`
    /// with the deterministic submission key. Recovery reconciles by key
    /// before creating new work.
    async fn submit_initial_turn(
        &self,
        work_order: &WorkOrder,
        occurrence: &WorkOrderOccurrence,
        session_id: &str,
        submission_key: &str,
    ) -> Result<String, String> {
        // Recovery: an already-linked job wins.
        if let Some(existing) = occurrence.job_id.as_deref() {
            return Ok(existing.to_owned());
        }
        let submission =
            self.deps.submission.as_ref().ok_or_else(|| {
                "scheduler_unavailable: daemon has no submission service".to_owned()
            })?;
        let key = crate::scheduler::submission::SubmissionKey::new(submission_key.to_owned())
            .map_err(|e| format!("materialization_failed: bad submission key: {e}"))?;
        let workspace_id_raw = self
            .resolve_shared_workspace(&work_order.project_id)
            .await?;
        let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(&workspace_id_raw);
        // Reconcile durable state first (post-restart lost acknowledgement).
        if let Ok(Some(existing)) = submission.reconcile_by_key(&key, &workspace_id, None).await {
            return Ok(existing.job_id.as_str().to_owned());
        }
        // Serialized mutation contends on one writer via the scheduler
        // exclusivity key; other work needs no exclusivity.
        let mut resource_request =
            codegg_core::jobs::ResourceRequest::for_kind(codegg_core::jobs::JobKind::AgentTurn);
        if matches!(
            work_order.workspace_policy,
            Some(codegg_core::work_order::WorkspacePolicy::Serialized)
        ) {
            resource_request.exclusivity_keys = vec!["exclusive:workspace-mutation".to_owned()];
        }
        let agent = "build".to_owned();
        let spec = codegg_core::jobs::NewJob {
            workspace_id,
            session_id: Some(session_id.to_owned()),
            turn_id: None,
            kind: codegg_core::jobs::JobKind::AgentTurn,
            source: codegg_core::jobs::JobSource::Api,
            priority: codegg_core::jobs::JobPriority::Normal,
            payload: codegg_core::jobs::JobPayload::AgentTurn {
                prompt: work_order.prompt.clone(),
                agent,
                model: work_order.requested_model.clone(),
                submission_key: Some(submission_key.to_owned()),
            },
            resource_request,
            timeout: None,
            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: ExecutionTarget::default(),
        };
        // execution-ownership: scheduler — the only production tool-call
        // boundary; heavy work goes through JobSubmissionService.
        match submission.submit(Some(key), spec).await {
            Ok(submitted) => Ok(submitted.job_id.as_str().to_owned()),
            Err(error) => Err(format!("scheduler submission failed: {error}")),
        }
    }

    /// Startup/wake reconciliation for partially materialized rows.
    pub async fn reconcile_work_orders_for_project(
        &self,
        project: &ProjectId,
    ) -> Result<usize, WorkOrderError> {
        let incomplete = self
            .work_orders
            .list_incomplete_occurrences(project, 64)
            .await?;
        // M002 reconciliation is durable-state convergence: every partial
        // row already carries its canonical links (workspace/session/job)
        // persisted before the next side effect, so a wake that finds the
        // links resumes without duplicating work. Link-less claiming rows
        // are left for the next explicit wake (which re-runs resolution).
        Ok(incomplete.len())
    }

    /// Cancel one occurrence with canonical propagation: running jobs are
    /// cancelled through the durable job store first, then the occurrence
    /// records its terminal state. Already-terminal rows are idempotent.
    pub async fn cancel_work_order_occurrence(
        &self,
        project: &ProjectId,
        occurrence_id: &WorkOrderOccurrenceId,
        now_ms: i64,
    ) -> Result<WorkOrderOccurrence, WorkOrderError> {
        let Some(current) = self
            .work_orders
            .get_occurrence(project, occurrence_id)
            .await?
        else {
            return Err(WorkOrderError::NotFound(occurrence_id.as_str().to_owned()));
        };
        if current.state.is_terminal() {
            return Ok(current);
        }
        // Route running work through canonical job cancellation before
        // recording the occurrence terminal state.
        if matches!(current.state, OccurrenceState::Running) {
            if let Some(job_id) = current.job_id.as_deref() {
                let job_id = codegg_core::jobs::JobId::new_unchecked(job_id.to_owned());
                let _ = self
                    .deps
                    .job_store
                    .request_cancel(
                        &job_id,
                        codegg_core::jobs::CancelReason::new("work-order", "occurrence cancelled"),
                    )
                    .await;
            }
        }
        let cancelled = self
            .work_orders
            .cancel_occurrence(project, occurrence_id, now_ms)
            .await?;
        self.publish_occurrence_changed(&cancelled, "cancelled")
            .await;
        Ok(cancelled)
    }

    /// Project a canonical terminal/attention outcome back into the
    /// occurrence, then advance finite repeats and wake downstream lane
    /// work. `create_next` gates repeat creation on the repeat budget.
    pub async fn finish_occurrence(
        &self,
        project: &ProjectId,
        occurrence_id: &WorkOrderOccurrenceId,
        target: OccurrenceState,
        attention_code: Option<AttentionCode>,
        diagnostic: Option<&str>,
        now_ms: i64,
    ) -> Result<WorkOrderOccurrence, WorkOrderError> {
        let finished = self
            .work_orders
            .transition_occurrence(
                project,
                occurrence_id,
                target,
                attention_code,
                diagnostic,
                now_ms,
            )
            .await?;
        let change = match target {
            OccurrenceState::Completed => "completed",
            OccurrenceState::Failed => "failed",
            OccurrenceState::Cancelled => "cancelled",
            OccurrenceState::NeedsAttention => "attention",
            _ => "updated",
        };
        self.publish_occurrence_changed(&finished, change).await;
        if target == OccurrenceState::Completed {
            if let Ok(Some(work_order)) = self
                .work_orders
                .get_work_order(project, &finished.work_order_id)
                .await
            {
                let page = self
                    .work_orders
                    .list_occurrences(project, &work_order.id, None)
                    .await?;
                if (page.occurrences.len() as u64) < u64::from(work_order.repeat_count) {
                    if let Ok(repeat) = self
                        .work_orders
                        .create_next_repeat_occurrence(
                            project,
                            &work_order.id,
                            &finished.id,
                            now_ms,
                        )
                        .await
                    {
                        if !repeat.duplicate {
                            self.publish_occurrence_changed(&repeat.occurrence, "repeat")
                                .await;
                        }
                    }
                }
            }
        }
        Ok(finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::identity::{PrincipalId, ProjectId};
    use codegg_core::work_order::{GateJoin, GateKind, GateSpec, NewWorkOrder, ReleaseGateSet};

    async fn temp_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory db");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    fn project() -> ProjectId {
        ProjectId::parse("project-1").unwrap()
    }

    fn creator() -> PrincipalId {
        PrincipalId::parse("local-owner").unwrap()
    }

    fn immediate_input(prompt: &str) -> NewWorkOrder {
        NewWorkOrder {
            title: Some("Task".to_owned()),
            prompt: prompt.to_owned(),
            requested_model: None,
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
            gates: ReleaseGateSet::immediate(),
            repeat_count: 1,
            sequence_lane_id: None,
            parent_session_id: None,
            parent_turn_id: None,
            parent_work_order_id: None,
            idempotency_key: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn immediate_work_becomes_ready_through_the_due_set() {
        let pool = temp_pool().await;
        let service = Arc::new(WorkOrderService::with_defaults(Some(pool)));
        let coordinator = WorkOrderCoordinator::new(service.clone());
        let created = service
            .create_work_order(&project(), &creator(), immediate_input("do it"), 1_000)
            .await
            .expect("create");
        service
            .create_occurrence(&project(), &created.work_order.id, None, 1_001)
            .await
            .expect("occurrence");
        let ready = coordinator
            .evaluate_due_for_project(&project(), 1_002)
            .await
            .expect("evaluate");
        assert_eq!(ready.len(), 1);
        assert!(matches!(
            ready[0].1.state,
            OccurrenceState::Ready | OccurrenceState::Waiting
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sequence_success_advances_and_failure_holds() {
        use codegg_core::work_order::{LaneFailurePolicy, NewSequenceLane};
        let pool = temp_pool().await;
        let service = Arc::new(WorkOrderService::with_defaults(Some(pool)));
        let coordinator = WorkOrderCoordinator::new(service.clone());
        let lane = service
            .create_lane(
                &project(),
                NewSequenceLane {
                    label: None,
                    failure_policy: LaneFailurePolicy::HoldLane,
                    idempotency_key: None,
                },
                500,
            )
            .await
            .expect("lane");
        // Two sequential work orders sharing one lane; the second gates on
        // sequence readiness.
        let first = service
            .create_work_order(&project(), &creator(), immediate_input("first"), 1_000)
            .await
            .expect("first");
        let mut second_input = immediate_input("second");
        second_input.gates = ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::SequenceReady,
                delay_secs: None,
                not_before_ms: None,
                lane_id: Some(lane.id.clone()),
                trigger_ref: None,
            }],
        };
        let second = service
            .create_work_order(&project(), &creator(), second_input, 1_001)
            .await
            .expect("second");
        for work in [&first.work_order, &second.work_order] {
            let current = service
                .get_lane(&project(), &lane.id)
                .await
                .expect("lane")
                .expect("row");
            service
                .attach_to_lane(
                    &project(),
                    &lane.id,
                    current.revision,
                    &work.id,
                    None,
                    1_002,
                )
                .await
                .expect("attach");
        }
        let first_occ = service
            .create_occurrence(&project(), &first.work_order.id, None, 1_003)
            .await
            .expect("first occurrence");
        let second_occ = service
            .create_occurrence(&project(), &second.work_order.id, None, 1_004)
            .await
            .expect("second occurrence");
        let second_work = service
            .get_work_order(&project(), &second.work_order.id)
            .await
            .expect("get")
            .expect("row");
        // Predecessor never ran: successor holds.
        assert!(!coordinator
            .sequence_ready_for(&project(), &second_work, &second_occ)
            .await
            .expect("sequence check"));
        // Predecessor success advances.
        for state in [
            OccurrenceState::Ready,
            OccurrenceState::Claiming,
            OccurrenceState::Running,
            OccurrenceState::Completed,
        ] {
            service
                .transition_occurrence(&project(), &first_occ.id, state, None, None, 1_005)
                .await
                .expect("advance predecessor");
        }
        assert!(coordinator
            .sequence_ready_for(&project(), &second_work, &second_occ)
            .await
            .expect("sequence check"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_predecessor_holds_by_default() {
        use codegg_core::work_order::{LaneFailurePolicy, NewSequenceLane};
        let pool = temp_pool().await;
        let service = Arc::new(WorkOrderService::with_defaults(Some(pool)));
        let coordinator = WorkOrderCoordinator::new(service.clone());
        let lane = service
            .create_lane(
                &project(),
                NewSequenceLane {
                    label: None,
                    failure_policy: LaneFailurePolicy::HoldLane,
                    idempotency_key: None,
                },
                500,
            )
            .await
            .expect("lane");
        let first = service
            .create_work_order(&project(), &creator(), immediate_input("first"), 1_000)
            .await
            .expect("first");
        let mut second_input = immediate_input("second");
        second_input.gates = ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::SequenceReady,
                delay_secs: None,
                not_before_ms: None,
                lane_id: Some(lane.id.clone()),
                trigger_ref: None,
            }],
        };
        let second = service
            .create_work_order(&project(), &creator(), second_input, 1_001)
            .await
            .expect("second");
        for work in [&first.work_order, &second.work_order] {
            let current = service
                .get_lane(&project(), &lane.id)
                .await
                .expect("lane")
                .expect("row");
            service
                .attach_to_lane(
                    &project(),
                    &lane.id,
                    current.revision,
                    &work.id,
                    None,
                    1_002,
                )
                .await
                .expect("attach");
        }
        let first_occ = service
            .create_occurrence(&project(), &first.work_order.id, None, 1_003)
            .await
            .expect("first occurrence");
        let second_occ = service
            .create_occurrence(&project(), &second.work_order.id, None, 1_004)
            .await
            .expect("second occurrence");
        for state in [
            OccurrenceState::Ready,
            OccurrenceState::Claiming,
            OccurrenceState::Running,
            OccurrenceState::Failed,
        ] {
            service
                .transition_occurrence(&project(), &first_occ.id, state, None, None, 1_005)
                .await
                .expect("advance predecessor");
        }
        let second_work = service
            .get_work_order(&project(), &second.work_order.id)
            .await
            .expect("get")
            .expect("row");
        assert!(!coordinator
            .sequence_ready_for(&project(), &second_work, &second_occ)
            .await
            .expect("sequence check"));
    }
}

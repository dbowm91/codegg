//! Application-owned produce/verify coordination.
//!
//! The coordinator is deliberately thin: `TaskTool` remains the only child
//! submission boundary, `AgentRunStore` remains the execution record
//! authority, and the convergence store only records bounded coordination
//! state. Completion is driven by durable run terminal events; the event
//! subscription is a wake-up mechanism, not the source of truth.

use std::sync::Arc;

use codegg_core::agent_convergence::{
    assemble_verifier_evidence, AgentOrchestrationOwner, ConvergenceDecision, ConvergenceError,
    ConvergenceId, ConvergenceSpec, ConvergenceStatus, ConvergenceStore, NewConvergence,
    SemanticVerificationVerdict, VerifierEvidencePacket,
};
use codegg_core::agent_run::AgentRunStatus;
use codegg_core::agent_run_control::AgentRunControlKind;
use codegg_core::run_result::{AgentRunResult, AgentRunResultStatus, RepositoryState};
use serde_json::json;

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::tool::task::TaskTool;

const VERIFIER_AGENT: &str = "verifier";
const MAX_PRODUCERS_PER_CYCLE: usize = 3;
const VERIFIER_DENIED_TOOLS: &[&str] = &[
    "write",
    "edit",
    "replace",
    "multiedit",
    "apply_patch",
    "bash",
    "terminal",
    "python",
    "python_script",
    "git",
    "commit",
    "task",
    "goal_get",
    "goal_update_progress",
    "goal_request_completion",
    "question",
    "permission",
    "test",
    "repo_fetch",
    "research",
    "webfetch",
    "websearch",
];

#[derive(Clone)]
pub struct ConvergenceCoordinator {
    store: Arc<dyn ConvergenceStore>,
    task_tool: TaskTool,
    verifier_agent: String,
    verifier_model: Option<String>,
}

impl ConvergenceCoordinator {
    pub fn new(store: Arc<dyn ConvergenceStore>, task_tool: TaskTool) -> Self {
        Self {
            store,
            task_tool,
            verifier_agent: VERIFIER_AGENT.to_string(),
            verifier_model: None,
        }
    }

    fn configured_verifier(&self, input: &serde_json::Value) -> Self {
        let mut coordinator = self.clone();
        if let Some(agent) = input.get("verifier_agent").and_then(|value| value.as_str()) {
            coordinator.verifier_agent = agent.to_string();
        }
        coordinator.verifier_model = input
            .get("verifier_model")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        coordinator
    }

    pub async fn start(
        &self,
        input: serde_json::Value,
        call_identity: String,
    ) -> Result<String, ToolError> {
        let coordinator = self.configured_verifier(&input);
        coordinator.start_inner(input, call_identity).await
    }

    async fn start_inner(
        &self,
        input: serde_json::Value,
        call_identity: String,
    ) -> Result<String, ToolError> {
        let producer = input
            .get("producer")
            .and_then(|value| value.as_object())
            .ok_or_else(|| ToolError::Execution("converge requires one producer object".into()))?;
        let producer_prompt = producer
            .get("prompt")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolError::Execution("producer.prompt is required".into()))?;
        let producer_agent = producer
            .get("agent")
            .and_then(|value| value.as_str())
            .unwrap_or("general");
        let objective = input
            .get("objective")
            .and_then(|value| value.as_str())
            .unwrap_or(producer_prompt);
        let criteria = input
            .get("criteria")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .map(|value| {
                        value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                            ToolError::Execution("criteria must contain strings".into())
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let max_cycles = match input.get("max_cycles").and_then(|value| value.as_u64()) {
            Some(value) if value <= u64::from(codegg_core::agent_convergence::MAX_CYCLES) => {
                value as u8
            }
            Some(_) => {
                return Err(ToolError::Execution(
                    "max_cycles must be between 1 and 4".into(),
                ))
            }
            None => self
                .task_tool
                .orchestration_config()
                .default_max_cycles
                .clamp(1, codegg_core::agent_convergence::MAX_CYCLES),
        };
        if max_cycles == 0 || max_cycles > codegg_core::agent_convergence::MAX_CYCLES {
            return Err(ToolError::Execution(
                "max_cycles must be between 1 and 4".into(),
            ));
        }
        let strategy = input
            .get("strategy")
            .and_then(|value| value.as_str())
            .unwrap_or("single");
        if strategy != "single" {
            return Err(ToolError::Execution(
                "M003 currently supports only the bounded single producer strategy".into(),
            ));
        }
        if self
            .task_tool
            .orchestration_config()
            .max_producers_per_cycle
            .min(MAX_PRODUCERS_PER_CYCLE as u8)
            == 0
        {
            return Err(ToolError::Execution(
                "producer width must be at least one".into(),
            ));
        }
        if let Some(requested) = input
            .get("max_producers_per_cycle")
            .and_then(|value| value.as_u64())
        {
            if requested == 0 || requested > MAX_PRODUCERS_PER_CYCLE as u64 {
                return Err(ToolError::Execution(
                    "max_producers_per_cycle must be between 1 and 3".into(),
                ));
            }
        }
        let owner = self.owner()?;
        let spec = ConvergenceSpec::new(objective, criteria)
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        let record = self
            .store
            .create_or_get(NewConvergence {
                id: ConvergenceId::new(),
                owner,
                spec,
                max_cycles,
                idempotency_key: call_identity.clone(),
            })
            .await
            .map_err(to_tool_error)?;

        let cycle = match self
            .store
            .get_cycle(&record.id, 0)
            .await
            .map_err(to_tool_error)?
        {
            Some(cycle) => cycle,
            None => match self.store.create_cycle(&record.id, 0).await {
                Ok(cycle) => cycle,
                Err(ConvergenceError::CycleConflict) => self
                    .store
                    .get_cycle(&record.id, 0)
                    .await
                    .map_err(to_tool_error)?
                    .ok_or_else(|| {
                        ToolError::Execution("convergence cycle disappeared during retry".into())
                    })?,
                Err(error) => return Err(to_tool_error(error)),
            },
        };
        if !cycle.producer_run_ids.is_empty() || record.status != ConvergenceStatus::Pending {
            // A status/reconnect call is also a bounded restart reconciliation
            // point. It only advances from authoritative durable child state;
            // it never resubmits a child whose reference is already persisted.
            let _ = self.advance(&record.id).await;
            self.publish(&record.id).await;
            self.spawn_watcher(record.id.clone());
            return self.format_status(&record.id).await;
        }

        let producing = match self
            .store
            .transition(&record.id, record.revision, ConvergenceStatus::Producing)
            .await
        {
            Ok(record) => record,
            Err(ConvergenceError::RevisionConflict) => return self.format_status(&record.id).await,
            Err(error) => return Err(to_tool_error(error)),
        };
        let child_input = json!({
            "action": "spawn",
            "description": producer.get("description").and_then(|value| value.as_str()).unwrap_or("Convergence producer"),
            "prompt": producer_prompt,
            "agent": producer_agent,
            "model": producer.get("model").and_then(|value| value.as_str()),
        });
        let output = match self
            .task_tool
            .execute_convergence_child(child_input, format!("{call_identity}/producer"))
            .await
        {
            Ok(output) => output,
            Err(error) => {
                let _ = self
                    .store
                    .transition(&record.id, producing.revision, ConvergenceStatus::Failed)
                    .await;
                return Err(error);
            }
        };
        let producer_run_id = parse_run_id(&output).ok_or_else(|| {
            ToolError::Execution("convergence producer did not return a durable run handle".into())
        })?;
        self.store
            .set_producer_references(&record.id, 0, None, vec![producer_run_id])
            .await
            .map_err(to_tool_error)?;
        self.publish(&record.id).await;
        self.spawn_watcher(record.id.clone());
        self.format_status(&record.id).await
    }

    pub async fn status(&self, input: &serde_json::Value) -> Result<String, ToolError> {
        let id = self.id_from_input(input)?;
        let record = self
            .store
            .get(&id)
            .await
            .map_err(to_tool_error)?
            .ok_or_else(|| ToolError::Execution(format!("convergence '{id}' not found")))?;
        self.authorize(&record.owner)?;
        let _ = self.advance(&id).await;
        self.format_status(&id).await
    }

    pub async fn decide(&self, input: &serde_json::Value) -> Result<String, ToolError> {
        let id = self.id_from_input(input)?;
        let record = self
            .store
            .get(&id)
            .await
            .map_err(to_tool_error)?
            .ok_or_else(|| ToolError::Execution(format!("convergence '{id}' not found")))?;
        self.authorize(&record.owner)?;
        let decision = parse_decision(input.get("decision").and_then(|value| value.as_str()))?;
        let replan_base = input
            .get("replan_base")
            .and_then(|value| value.as_str())
            .unwrap_or("original");
        if replan_base != "original" && replan_base != "last_clean_result" {
            return Err(ToolError::Execution(
                "replan_base must be original or last_clean_result".into(),
            ));
        }
        let selected_replan_base =
            if decision == ConvergenceDecision::Replan && replan_base == "last_clean_result" {
                let cycle = self
                    .store
                    .get_cycle(&id, record.current_cycle)
                    .await
                    .map_err(to_tool_error)?
                    .ok_or_else(|| ToolError::Execution("replan source cycle is missing".into()))?;
                let run_id = cycle.producer_run_ids.first().ok_or_else(|| {
                    ToolError::Execution("replan source producer is missing".into())
                })?;
                Some(
                    self.load_repairable_result(run_id)
                        .await?
                        .result_commit
                        .ok_or_else(|| {
                            ToolError::Execution("replan source has no result commit".into())
                        })?,
                )
            } else {
                None
            };
        let updated = self
            .store
            .set_decision(&id, record.current_cycle, record.revision, decision)
            .await
            .map_err(to_tool_error)?;
        if let Some(base_commit) = selected_replan_base {
            let cycle = match self
                .store
                .get_cycle(&id, updated.current_cycle)
                .await
                .map_err(to_tool_error)?
            {
                Some(cycle) => cycle,
                None => self
                    .store
                    .create_cycle(&id, updated.current_cycle)
                    .await
                    .map_err(to_tool_error)?,
            };
            self.store
                .set_cycle_commits(&id, cycle.ordinal, Some(base_commit), None)
                .await
                .map_err(to_tool_error)?;
        }
        if matches!(
            decision,
            ConvergenceDecision::Repair | ConvergenceDecision::Replan
        ) {
            self.advance(&id).await?;
            self.spawn_watcher(id.clone());
        }
        self.publish(&id).await;
        Ok(format!(
            "Convergence {}: {} (decision: {})",
            updated.id,
            updated.status.as_str(),
            decision.decision_name()
        ))
    }

    pub async fn cancel(&self, input: &serde_json::Value) -> Result<String, ToolError> {
        let id = self.id_from_input(input)?;
        let record = self
            .store
            .get(&id)
            .await
            .map_err(to_tool_error)?
            .ok_or_else(|| ToolError::Execution(format!("convergence '{id}' not found")))?;
        self.authorize(&record.owner)?;
        if record.status.is_terminal() {
            return self.format_status(&id).await;
        }
        if let Some(cycle) = self
            .store
            .get_cycle(&id, record.current_cycle)
            .await
            .map_err(to_tool_error)?
        {
            if let Some(control) = self.task_tool.run_control() {
                let actor = self.task_tool.control_actor();
                let mut runs = cycle.producer_run_ids;
                if let Some(verifier) = cycle.verifier_run_id {
                    runs.push(verifier);
                }
                for run_id in runs {
                    if let Some(store) = self.task_tool.agent_run_store() {
                        if store
                            .get_run(&run_id)
                            .await
                            .map_err(|error| ToolError::Execution(error.to_string()))?
                            .is_some_and(|run| !run.status.is_terminal())
                        {
                            let _ = control
                                .send(
                                    &actor,
                                    run_id,
                                    AgentRunControlKind::Cancel,
                                    "convergence cancelled".into(),
                                    format!("convergence:{id}:cancel"),
                                )
                                .await;
                        }
                    }
                }
            }
        }
        let updated = self
            .store
            .transition(&id, record.revision, ConvergenceStatus::Cancelled)
            .await
            .map_err(to_tool_error)?;
        self.publish(&id).await;
        Ok(format!(
            "Convergence {}: {}",
            updated.id,
            updated.status.as_str()
        ))
    }

    fn owner(&self) -> Result<AgentOrchestrationOwner, ToolError> {
        let owner = self.task_tool.orchestration_owner().ok_or_else(|| {
            ToolError::Execution("convergence requires an orchestration owner".into())
        })?;
        Ok(match owner {
            crate::tool::task::AgentOrchestrationOwner::Turn {
                session_id,
                turn_id,
            } => AgentOrchestrationOwner::Turn {
                session_id,
                turn_id,
            },
            crate::tool::task::AgentOrchestrationOwner::Run { run_id } => {
                AgentOrchestrationOwner::Run { run_id }
            }
        })
    }

    fn authorize(&self, owner: &AgentOrchestrationOwner) -> Result<(), ToolError> {
        if &self.owner()? == owner {
            Ok(())
        } else {
            Err(ToolError::Execution(
                "convergence operation is unauthorized".into(),
            ))
        }
    }

    fn id_from_input(&self, input: &serde_json::Value) -> Result<ConvergenceId, ToolError> {
        let raw = input
            .get("convergence_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolError::Execution("convergence_id is required".into()))?;
        ConvergenceId::parse(raw).map_err(to_tool_error)
    }

    fn spawn_watcher(&self, id: ConvergenceId) {
        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.watch(id).await;
        });
    }

    async fn watch(&self, id: ConvergenceId) {
        let mut events = GlobalEventBus::subscribe();
        loop {
            match self.advance(&id).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(convergence_id = %id, %error, "convergence coordination failed; durable state remains for reconciliation");
                }
            }
            let Some(record) = self.store.get(&id).await.ok().flatten() else {
                return;
            };
            if record.status.is_terminal() || record.status == ConvergenceStatus::AwaitingDecision {
                return;
            }
            match events.recv().await {
                Ok(AppEvent::AgentRunTerminal { run_id, .. }) => {
                    if self.references_run(&id, &run_id).await {
                        continue;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    async fn advance(&self, id: &ConvergenceId) -> Result<bool, ToolError> {
        let Some(record) = self.store.get(id).await.map_err(to_tool_error)? else {
            return Ok(false);
        };
        if record.status.is_terminal() {
            return Ok(false);
        }
        if self.deadline_exceeded(&record) {
            let exhausted = self
                .store
                .transition(id, record.revision, ConvergenceStatus::Exhausted)
                .await
                .map_err(to_tool_error)?;
            self.publish(&exhausted.id).await;
            return Ok(true);
        }
        if record.status == ConvergenceStatus::AwaitingDecision {
            return Ok(false);
        }
        let Some(cycle) = self
            .store
            .get_cycle(id, record.current_cycle)
            .await
            .map_err(to_tool_error)?
        else {
            return Ok(false);
        };
        match record.status {
            ConvergenceStatus::Producing => self.advance_producer(&record, &cycle).await,
            ConvergenceStatus::Verifying => self.advance_verifier(&record, &cycle).await,
            ConvergenceStatus::Repairing => self.advance_next_cycle(&record, true).await,
            ConvergenceStatus::Replanning => self.advance_next_cycle(&record, false).await,
            ConvergenceStatus::Pending => Ok(false),
            _ => Ok(false),
        }
    }

    async fn advance_next_cycle(
        &self,
        record: &codegg_core::agent_convergence::ConvergenceRecord,
        repair: bool,
    ) -> Result<bool, ToolError> {
        if record.current_cycle >= record.max_cycles {
            let exhausted = self
                .store
                .transition(&record.id, record.revision, ConvergenceStatus::Exhausted)
                .await
                .map_err(to_tool_error)?;
            self.publish(&exhausted.id).await;
            return Ok(true);
        }
        if record.current_cycle >= 2 {
            let previous = self
                .store
                .get_cycle(&record.id, record.current_cycle - 1)
                .await
                .map_err(to_tool_error)?;
            let before = self
                .store
                .get_cycle(&record.id, record.current_cycle - 2)
                .await
                .map_err(to_tool_error)?;
            if previous.as_ref().is_some_and(|previous| {
                before
                    .as_ref()
                    .is_some_and(|before| cycle_fingerprint(previous) == cycle_fingerprint(before))
            }) {
                let exhausted = self
                    .store
                    .transition(&record.id, record.revision, ConvergenceStatus::Exhausted)
                    .await
                    .map_err(to_tool_error)?;
                self.publish(&exhausted.id).await;
                return Ok(true);
            }
        }
        let ordinal = record.current_cycle;
        let cycle = match self
            .store
            .get_cycle(&record.id, ordinal)
            .await
            .map_err(to_tool_error)?
        {
            Some(cycle) => cycle,
            None => match self.store.create_cycle(&record.id, ordinal).await {
                Ok(cycle) => cycle,
                Err(ConvergenceError::CycleConflict) => self
                    .store
                    .get_cycle(&record.id, ordinal)
                    .await
                    .map_err(to_tool_error)?
                    .ok_or_else(|| {
                        ToolError::Execution("next convergence cycle disappeared".into())
                    })?,
                Err(error) => return Err(to_tool_error(error)),
            },
        };
        if !cycle.producer_run_ids.is_empty() {
            return Ok(false);
        }
        let (base_commit, prior_result, prior_verdict) = if repair {
            let prior = self
                .store
                .get_cycle(&record.id, ordinal.saturating_sub(1))
                .await
                .map_err(to_tool_error)?
                .ok_or_else(|| {
                    ToolError::Execution("CannotRepairFromResult: prior cycle missing".into())
                })?;
            let run_id = prior.producer_run_ids.first().ok_or_else(|| {
                ToolError::Execution("CannotRepairFromResult: producer missing".into())
            })?;
            let result = self.load_repairable_result(run_id).await?;
            let commit = result.result_commit.clone().ok_or_else(|| {
                ToolError::Execution("CannotRepairFromResult: producer has no result commit".into())
            })?;
            (commit, Some(result), prior.verdict)
        } else {
            let source = cycle.source_base_commit.or(self
                .store
                .get_cycle(&record.id, 0)
                .await
                .map_err(to_tool_error)?
                .and_then(|cycle| cycle.source_base_commit));
            let commit = source.ok_or_else(|| {
                ToolError::Execution(
                    "NeedsAttention: original convergence base is not recorded".into(),
                )
            })?;
            self.validate_commit(&commit).await?;
            let prior_verdict = self
                .store
                .get_cycle(&record.id, ordinal.saturating_sub(1))
                .await
                .map_err(to_tool_error)?
                .and_then(|cycle| cycle.verdict);
            (commit, None, prior_verdict)
        };
        self.store
            .set_cycle_commits(&record.id, ordinal, Some(base_commit.clone()), None)
            .await
            .map_err(to_tool_error)?;
        let prompt = if repair {
            repair_prompt(
                record,
                prior_result.as_ref(),
                prior_verdict.as_ref(),
                &base_commit,
            )
        } else {
            replan_prompt(record, prior_verdict.as_ref(), &base_commit)
        };
        let child_input = json!({
            "action": "spawn",
            "description": if repair { "Convergence repair producer" } else { "Convergence replanned producer" },
            "prompt": prompt,
            "agent": "general",
        });
        let output = self
            .task_tool
            .execute_convergence_child_from_base(
                child_input,
                format!(
                    "{}:{}:{}",
                    record.id,
                    if repair { "repair" } else { "replan" },
                    ordinal
                ),
                base_commit,
            )
            .await?;
        let run_id = parse_run_id(&output).ok_or_else(|| {
            ToolError::Execution(
                "convergence continuation did not return a durable run handle".into(),
            )
        })?;
        self.store
            .set_producer_references(&record.id, ordinal, None, vec![run_id])
            .await
            .map_err(to_tool_error)?;
        self.store
            .transition(&record.id, record.revision, ConvergenceStatus::Producing)
            .await
            .map_err(to_tool_error)?;
        self.publish(&record.id).await;
        Ok(true)
    }

    fn deadline_exceeded(
        &self,
        record: &codegg_core::agent_convergence::ConvergenceRecord,
    ) -> bool {
        let max_wall_clock_ms = self
            .task_tool
            .orchestration_config()
            .max_wall_clock_ms
            .unwrap_or(codegg_config::schema::OrchestrationConfig::HARD_MAX_WALL_CLOCK_MS);
        let elapsed = chrono::Utc::now()
            .timestamp_millis()
            .saturating_sub(record.created_at);
        elapsed >= i64::try_from(max_wall_clock_ms).unwrap_or(i64::MAX)
    }

    async fn load_repairable_result(
        &self,
        run_id: &codegg_core::identity::AgentRunId,
    ) -> Result<AgentRunResult, ToolError> {
        let store = self.task_tool.agent_run_store().ok_or_else(|| {
            ToolError::Execution("CannotRepairFromResult: durable run store unavailable".into())
        })?;
        let run = store
            .get_run(run_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
            .ok_or_else(|| {
                ToolError::Execution("CannotRepairFromResult: prior run missing".into())
            })?;
        if !run.status.is_terminal() {
            return Err(ToolError::Execution(
                "CannotRepairFromResult: prior run is not terminal".into(),
            ));
        }
        let task = store
            .get_task(&run.task_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
            .ok_or_else(|| {
                ToolError::Execution("CannotRepairFromResult: prior task missing".into())
            })?;
        if task.repository_id != self.task_tool.repository_id() {
            return Err(ToolError::Execution(
                "CannotRepairFromResult: repository identity differs".into(),
            ));
        }
        let result = store
            .get_result(run_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
            .ok_or_else(|| {
                ToolError::Execution("CannotRepairFromResult: structured result missing".into())
            })?;
        if result.status != AgentRunResultStatus::Succeeded
            || result.repository_state != RepositoryState::Clean
            || result.result_commit.is_none()
            || result.worktree_id.is_none()
            || run.worktree_id != result.worktree_id
        {
            return Err(ToolError::Execution(
                "CannotRepairFromResult: result provenance is not clean and complete".into(),
            ));
        }
        self.validate_commit(result.result_commit.as_deref().unwrap())
            .await?;
        Ok(result)
    }

    async fn validate_commit(&self, commit: &str) -> Result<(), ToolError> {
        let root = self.task_tool.workspace_root().ok_or_else(|| {
            ToolError::Execution("CannotRepairFromResult: workspace root unavailable".into())
        })?;
        egggit::resolve_commit(&root, commit)
            .await
            .map(|_| ())
            .map_err(|error| {
                ToolError::Execution(format!(
                    "CannotRepairFromResult: commit is not resolvable: {error}"
                ))
            })
    }

    async fn advance_producer(
        &self,
        record: &codegg_core::agent_convergence::ConvergenceRecord,
        cycle: &codegg_core::agent_convergence::ConvergenceCycleRecord,
    ) -> Result<bool, ToolError> {
        let Some(run_id) = cycle.producer_run_ids.first() else {
            return Ok(false);
        };
        let Some(store) = self.task_tool.agent_run_store() else {
            return Err(ToolError::Execution(
                "durable run store is unavailable".into(),
            ));
        };
        let Some(run) = store
            .get_run(run_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
        else {
            return Ok(false);
        };
        if !run.status.is_terminal() {
            return Ok(false);
        }
        if run.status != AgentRunStatus::Completed {
            let next = if run.status == AgentRunStatus::Cancelled {
                ConvergenceStatus::Cancelled
            } else {
                ConvergenceStatus::Failed
            };
            self.store
                .transition(&record.id, record.revision, next)
                .await
                .map_err(to_tool_error)?;
            self.publish(&record.id).await;
            return Ok(true);
        }
        let Some(result) = store
            .get_result(run_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
        else {
            self.fail(record, "producer completed without a structured result")
                .await?;
            return Ok(true);
        };
        if result.status != codegg_core::run_result::AgentRunResultStatus::Succeeded {
            self.fail(record, "producer has no reviewable successful result")
                .await?;
            return Ok(true);
        }
        if result.repository_state != RepositoryState::Clean
            || result.result_commit.is_none()
            || result.worktree_id.is_none()
            || run.worktree_id != result.worktree_id
            || (cycle.source_base_commit.is_some()
                && cycle.source_base_commit != result.base_commit)
        {
            self.fail(
                record,
                "producer result is not clean or has invalid worktree provenance",
            )
            .await?;
            return Ok(true);
        }
        if let Some(result_commit) = result.result_commit.as_deref() {
            self.validate_commit(result_commit).await?;
        }
        let packet = assemble_verifier_evidence(&record.spec, std::slice::from_ref(&result))
            .map_err(to_tool_error)?;
        self.store
            .set_cycle_commits(
                &record.id,
                cycle.ordinal,
                result.base_commit.clone(),
                result.result_commit.clone(),
            )
            .await
            .map_err(to_tool_error)?;
        let verifying = self
            .store
            .transition(&record.id, record.revision, ConvergenceStatus::Verifying)
            .await
            .map_err(to_tool_error)?;
        let verifier_tool = self
            .task_tool
            .clone()
            .with_additional_denied_tools(VERIFIER_DENIED_TOOLS);
        let verifier_input = json!({
            "action": "spawn",
            "description": "Independent convergence verifier",
            "prompt": verifier_prompt(&packet)?,
            "agent": self.verifier_agent,
            "model": self.verifier_model.clone(),
        });
        let output = match verifier_tool
            .execute_convergence_child(verifier_input, format!("{}:verifier:0", record.id))
            .await
        {
            Ok(output) => output,
            Err(error) => {
                self.fail(&verifying, "verifier submission failed").await?;
                return Err(error);
            }
        };
        let verifier_run_id = parse_run_id(&output).ok_or_else(|| {
            ToolError::Execution("verifier did not return a durable run handle".into())
        })?;
        self.store
            .set_verifier_run(&record.id, cycle.ordinal, verifier_run_id)
            .await
            .map_err(to_tool_error)?;
        self.publish(&record.id).await;
        Ok(true)
    }

    async fn advance_verifier(
        &self,
        record: &codegg_core::agent_convergence::ConvergenceRecord,
        cycle: &codegg_core::agent_convergence::ConvergenceCycleRecord,
    ) -> Result<bool, ToolError> {
        let Some(verifier_run_id) = cycle.verifier_run_id.as_ref() else {
            return Ok(false);
        };
        let Some(store) = self.task_tool.agent_run_store() else {
            return Err(ToolError::Execution(
                "durable run store is unavailable".into(),
            ));
        };
        let Some(run) = store
            .get_run(verifier_run_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
        else {
            return Ok(false);
        };
        if !run.status.is_terminal() {
            return Ok(false);
        }
        if run.status != AgentRunStatus::Completed {
            self.fail(record, "verifier did not complete successfully")
                .await?;
            return Ok(true);
        }
        let verdict = match store
            .get_result(verifier_run_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
        {
            Some(result) => SemanticVerificationVerdict::parse_marked(&result.summary)
                .unwrap_or_else(|error| SemanticVerificationVerdict::Inconclusive {
                    reason: format!("verifier output was malformed: {error}"),
                    missing_evidence: vec!["typed convergence verdict".into()],
                }),
            None => SemanticVerificationVerdict::Inconclusive {
                reason: "verifier completed without a structured result".into(),
                missing_evidence: vec!["verifier result".into()],
            },
        };
        self.store
            .set_verdict(&record.id, cycle.ordinal, verdict)
            .await
            .map_err(to_tool_error)?;
        let current = self
            .store
            .get(&record.id)
            .await
            .map_err(to_tool_error)?
            .ok_or_else(|| {
                ToolError::Execution("convergence disappeared during verification".into())
            })?;
        self.store
            .transition(
                &record.id,
                current.revision,
                ConvergenceStatus::AwaitingDecision,
            )
            .await
            .map_err(to_tool_error)?;
        self.publish(&record.id).await;
        Ok(true)
    }

    async fn fail(
        &self,
        record: &codegg_core::agent_convergence::ConvergenceRecord,
        reason: &str,
    ) -> Result<(), ToolError> {
        tracing::warn!(convergence_id = %record.id, reason, "convergence failed");
        self.store
            .transition(&record.id, record.revision, ConvergenceStatus::Failed)
            .await
            .map_err(to_tool_error)?;
        self.publish(&record.id).await;
        Ok(())
    }

    async fn references_run(&self, id: &ConvergenceId, run_id: &str) -> bool {
        let Some(record) = self.store.get(id).await.ok().flatten() else {
            return false;
        };
        self.store
            .get_cycle(id, record.current_cycle)
            .await
            .ok()
            .flatten()
            .is_some_and(|cycle| {
                cycle
                    .producer_run_ids
                    .iter()
                    .any(|item| item.to_string() == run_id)
                    || cycle
                        .verifier_run_id
                        .as_ref()
                        .is_some_and(|item| item.to_string() == run_id)
            })
    }

    async fn format_status(&self, id: &ConvergenceId) -> Result<String, ToolError> {
        let record = self
            .store
            .get(id)
            .await
            .map_err(to_tool_error)?
            .ok_or_else(|| ToolError::Execution(format!("convergence '{id}' not found")))?;
        let cycle = self
            .store
            .get_cycle(id, record.current_cycle)
            .await
            .map_err(to_tool_error)?;
        let mut statuses = Vec::new();
        if let (Some(cycle), Some(store)) = (cycle.as_ref(), self.task_tool.agent_run_store()) {
            for run_id in &cycle.producer_run_ids {
                if let Some(run) = store
                    .get_run(run_id)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?
                {
                    statuses.push(run.status);
                }
            }
        }
        let summary = record.summary(cycle.as_ref(), &statuses);
        serde_json::to_string(&summary).map_err(|error| ToolError::Execution(error.to_string()))
    }

    async fn publish(&self, id: &ConvergenceId) {
        let Ok(Some(record)) = self.store.get(id).await else {
            return;
        };
        let cycle = self
            .store
            .get_cycle(id, record.current_cycle)
            .await
            .ok()
            .flatten();
        let mut statuses = Vec::new();
        if let (Some(cycle), Some(store)) = (cycle.as_ref(), self.task_tool.agent_run_store()) {
            for run_id in &cycle.producer_run_ids {
                if let Ok(Some(run)) = store.get_run(run_id).await {
                    statuses.push(run.status);
                }
            }
        }
        self.task_tool
            .publish_convergence_projection(&record.summary(cycle.as_ref(), &statuses));
    }
}

fn verifier_prompt(packet: &VerifierEvidencePacket) -> Result<String, ToolError> {
    let encoded = packet.encode_bounded().map_err(to_tool_error)?;
    Ok(format!(
        "You are an independent semantic verifier. You did not produce the artifact; challenge the producer's assumptions. The JSON below is host evidence assembled from the authoritative AgentRunResult, validation, Git, and artifact references. Claims absent from it are not facts. Do not claim to have run checks absent from host evidence. Pass means only no blocking semantic finding within scope; it is not goal completion, merge approval, or permission approval. Cite changed paths or evidence references where available. Missing evidence or uncertainty must be Inconclusive. You have read-only authority and must not request mutations, delegation, permission responses, or goal completion. Return exactly one marked JSON object and no surrounding prose: <convergence_verdict>{{\"kind\":\"pass\",\"summary\":\"...\",\"evidence_refs\":[]}}</convergence_verdict>. The kind must be pass, revise, or inconclusive.\n\nHOST EVIDENCE:\n{encoded}"
    ))
}

fn cycle_fingerprint(cycle: &codegg_core::agent_convergence::ConvergenceCycleRecord) -> String {
    let verdict = cycle
        .verdict
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
        .unwrap_or_default();
    format!(
        "{}:{}",
        cycle.result_commit.as_deref().unwrap_or_default(),
        verdict
    )
}

fn repair_prompt(
    record: &codegg_core::agent_convergence::ConvergenceRecord,
    result: Option<&AgentRunResult>,
    verdict: Option<&SemanticVerificationVerdict>,
    base_commit: &str,
) -> String {
    let (summary, paths) = result
        .map(|result| {
            (
                result.summary.clone(),
                result
                    .changed_paths
                    .iter()
                    .take(64)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    format!(
        "Repair the prior implementation against the original bounded objective and criteria. Start from exact Git base commit {base_commit}; do not use another checkout or the parent HEAD. Apply the independent verifier's actionable requests, then produce a durable clean result. Objective: {}. Criteria: {}. Verifier findings: {}. Prior producer summary: {}. Prior changed paths: {}",
        record.spec.objective,
        record.spec.criteria.join("; "),
        verdict
            .and_then(|verdict| serde_json::to_string(verdict).ok())
            .unwrap_or_else(|| "no structured verifier findings".into()),
        summary,
        paths.join(", ")
    )
}

fn replan_prompt(
    record: &codegg_core::agent_convergence::ConvergenceRecord,
    verdict: Option<&SemanticVerificationVerdict>,
    base_commit: &str,
) -> String {
    format!(
        "Replan the implementation from exact Git base commit {base_commit}. Reconsider the approach in light of the prior independent verification rather than applying a narrow blind patch. Objective: {}. Criteria: {}. Prior verifier lessons: {}.",
        record.spec.objective,
        record.spec.criteria.join("; "),
        verdict
            .and_then(|verdict| serde_json::to_string(verdict).ok())
            .unwrap_or_else(|| "no structured verifier lessons".into())
    )
}

fn parse_run_id(output: &str) -> Option<codegg_core::identity::AgentRunId> {
    output.lines().find_map(|line| {
        line.strip_prefix("Run: ")
            .and_then(|value| codegg_core::identity::AgentRunId::parse(value.trim()).ok())
    })
}

fn parse_decision(value: Option<&str>) -> Result<ConvergenceDecision, ToolError> {
    match value {
        Some("accept") => Ok(ConvergenceDecision::Accept),
        Some("stop") => Ok(ConvergenceDecision::Stop),
        Some("escalate") => Ok(ConvergenceDecision::Escalate),
        Some("repair") | Some("replan") => Ok(if value == Some("repair") {
            ConvergenceDecision::Repair
        } else {
            ConvergenceDecision::Replan
        }),
        _ => Err(ToolError::Execution(
            "decision must be accept, stop, escalate, repair, or replan".into(),
        )),
    }
}

fn to_tool_error(error: ConvergenceError) -> ToolError {
    ToolError::Execution(error.to_string())
}

trait DecisionName {
    fn decision_name(self) -> &'static str;
}

impl DecisionName for ConvergenceDecision {
    fn decision_name(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Repair => "repair",
            Self::Replan => "replan",
            Self::Stop => "stop",
            Self::Escalate => "escalate",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m002_verifier_ceiling_denies_mutation_and_authority_tools() {
        for tool in [
            "write",
            "bash",
            "git",
            "commit",
            "task",
            "permission",
            "goal_request_completion",
        ] {
            assert!(VERIFIER_DENIED_TOOLS.contains(&tool));
        }
    }

    #[test]
    fn m002_decisions_reject_future_cycle_actions() {
        assert!(parse_decision(Some("repair")).is_ok());
        assert!(parse_decision(Some("replan")).is_ok());
        assert!(parse_decision(Some("unknown")).is_err());
    }
}

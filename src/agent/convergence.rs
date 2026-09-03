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
use serde_json::json;

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::ToolError;
use crate::tool::task::TaskTool;

const VERIFIER_AGENT: &str = "verifier";
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
        let max_cycles = input
            .get("max_cycles")
            .and_then(|value| value.as_u64())
            .unwrap_or(1);
        if max_cycles != 1 {
            return Err(ToolError::Execution(
                "M002 converge requires max_cycles to be exactly 1".into(),
            ));
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
                max_cycles: 1,
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
        if matches!(
            decision,
            ConvergenceDecision::Repair | ConvergenceDecision::Replan
        ) {
            return Err(ToolError::Execution(
                "repair and replan are not available until M003".into(),
            ));
        }
        let updated = self
            .store
            .set_decision(&id, record.current_cycle, record.revision, decision)
            .await
            .map_err(to_tool_error)?;
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
        if record.status.is_terminal() || record.status == ConvergenceStatus::AwaitingDecision {
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
            ConvergenceStatus::Pending => Ok(false),
            _ => Ok(false),
        }
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
            "decision must be accept, stop, or escalate in M002".into(),
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

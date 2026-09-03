//! Daemon-side bridge between durable run control and a live `AgentLoop`.
//!
//! Every control is written to the core mailbox before this module touches a
//! live channel.  The live channels are an optimization; the mailbox and run
//! store remain authoritative across disconnects and restarts.

use codegg_core::agent_run::{AgentRunRecord, AgentRunStatus, AgentRunStore};
use codegg_core::agent_run_control::{
    AgentRunControlKind, AgentRunControlStore, AgentRunJournalEvent, AgentRunJournalEventKind,
    AgentRunMailboxMessage, InMemoryAgentRunControlStore, MailboxState, NewControlMessage,
    NewJournalEvent, SqliteAgentRunControlStore,
};
use codegg_core::identity::{AgentRunId, AgentRunMessageId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Mutex};

const MAX_NOTIFICATION_BYTES: usize = 2 * 1024;
const MAX_WAIT_MS: u64 = 30_000;

#[derive(Debug, Clone)]
pub struct LiveRunHandle {
    pub follow_up_tx: mpsc::Sender<String>,
    pub steer_tx: mpsc::Sender<String>,
    pub cancel_tx: watch::Sender<bool>,
    pub interrupt_flag: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LiveTurnOwner {
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct ControlActor {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub run_id: Option<AgentRunId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlOutcome {
    Queued(AgentRunMailboxMessage),
    Terminal(AgentRunStatus),
    Status(AgentRunRecord),
    Wait {
        run: AgentRunRecord,
        timed_out: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RunControlError {
    #[error("run control store error: {0}")]
    Store(#[from] codegg_core::agent_run_control::AgentRunControlStoreError),
    #[error("agent run store error: {0}")]
    RunStore(String),
    #[error("agent run '{0}' not found")]
    RunNotFound(String),
    #[error("run control is not authorized for this run")]
    Unauthorized,
    #[error("live run control channel is full")]
    ChannelFull,
    #[error("live run control channel is unavailable")]
    ChannelClosed,
}

pub struct RunControlService {
    runs: Arc<dyn AgentRunStore>,
    controls: Arc<dyn AgentRunControlStore>,
    live: Mutex<HashMap<AgentRunId, LiveRunHandle>>,
    live_turns: Mutex<HashMap<LiveTurnOwner, mpsc::Sender<String>>>,
    scheduler: Mutex<Option<Arc<crate::scheduler::JobScheduler>>>,
    groups: Mutex<Option<Arc<codegg_core::agent_run_group::AgentRunGroupService>>>,
}

impl RunControlService {
    pub fn in_memory(runs: Arc<dyn AgentRunStore>) -> Arc<Self> {
        Arc::new(Self {
            runs,
            controls: Arc::new(InMemoryAgentRunControlStore::new()),
            live: Mutex::new(HashMap::new()),
            live_turns: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(None),
            groups: Mutex::new(None),
        })
    }

    pub fn with_pool(runs: Arc<dyn AgentRunStore>, pool: sqlx::SqlitePool) -> Arc<Self> {
        Arc::new(Self {
            runs,
            controls: Arc::new(SqliteAgentRunControlStore::new(pool)),
            live: Mutex::new(HashMap::new()),
            live_turns: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(None),
            groups: Mutex::new(None),
        })
    }

    pub fn with_control_store(
        runs: Arc<dyn AgentRunStore>,
        controls: Arc<dyn AgentRunControlStore>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runs,
            controls,
            live: Mutex::new(HashMap::new()),
            live_turns: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(None),
            groups: Mutex::new(None),
        })
    }

    pub fn controls(&self) -> &Arc<dyn AgentRunControlStore> {
        &self.controls
    }

    pub async fn set_scheduler(&self, scheduler: Arc<crate::scheduler::JobScheduler>) {
        *self.scheduler.lock().await = Some(scheduler);
    }

    pub fn set_scheduler_sync(&self, scheduler: Arc<crate::scheduler::JobScheduler>) {
        if let Ok(mut slot) = self.scheduler.try_lock() {
            *slot = Some(scheduler);
        }
    }

    pub async fn set_group_service(
        &self,
        service: Arc<codegg_core::agent_run_group::AgentRunGroupService>,
    ) {
        *self.groups.lock().await = Some(service);
    }

    pub fn set_group_service_sync(
        &self,
        service: Arc<codegg_core::agent_run_group::AgentRunGroupService>,
    ) {
        if let Ok(mut slot) = self.groups.try_lock() {
            *slot = Some(service);
        }
    }

    pub async fn register_live(&self, run_id: AgentRunId, handle: LiveRunHandle) {
        self.live.lock().await.insert(run_id.clone(), handle);
        if let Err(error) = self.dispatch_pending(&run_id, true).await {
            tracing::warn!(run_id = %run_id, %error, "failed to replay run controls on live attach");
        }
    }

    pub async fn unregister_live(&self, run_id: &AgentRunId) {
        self.live.lock().await.remove(run_id);
    }

    pub async fn register_live_turn(
        &self,
        session_id: String,
        turn_id: String,
        follow_up_tx: mpsc::Sender<String>,
    ) {
        self.live_turns.lock().await.insert(
            LiveTurnOwner {
                session_id,
                turn_id,
            },
            follow_up_tx,
        );
    }

    pub async fn unregister_live_turn(
        &self,
        owner: &LiveTurnOwner,
        follow_up_tx: &mpsc::Sender<String>,
    ) {
        let mut live_turns = self.live_turns.lock().await;
        if live_turns
            .get(owner)
            .is_some_and(|registered| registered.same_channel(follow_up_tx))
        {
            live_turns.remove(owner);
        }
    }

    pub async fn authorize(
        &self,
        actor: &ControlActor,
        target: &AgentRunId,
    ) -> Result<AgentRunRecord, RunControlError> {
        let target_run = self
            .runs
            .get_run(target)
            .await
            .map_err(|e| RunControlError::RunStore(e.to_string()))?
            .ok_or_else(|| RunControlError::RunNotFound(target.to_string()))?;
        let task = self
            .runs
            .get_task(&target_run.task_id)
            .await
            .map_err(|e| RunControlError::RunStore(e.to_string()))?
            .ok_or_else(|| RunControlError::RunNotFound(target.to_string()))?;
        if let (Some(session_id), Some(turn_id)) =
            (actor.session_id.as_deref(), actor.turn_id.as_deref())
        {
            if task.originating_session_id == session_id
                && task.originating_turn_id.as_deref() == Some(turn_id)
                && target_run.parent_run_id.is_none()
            {
                return Ok(target_run);
            }
        }
        if let Some(actor_run_id) = actor.run_id.as_ref() {
            if target_run.parent_run_id.as_ref() == Some(actor_run_id)
                && target_run.run_id != *actor_run_id
            {
                return Ok(target_run);
            }
        }
        Err(RunControlError::Unauthorized)
    }

    pub async fn send(
        &self,
        actor: &ControlActor,
        target: AgentRunId,
        kind: AgentRunControlKind,
        payload: String,
        idempotency_key: String,
    ) -> Result<ControlOutcome, RunControlError> {
        let run = self.authorize(actor, &target).await?;
        if run.status.is_terminal() {
            return Ok(ControlOutcome::Terminal(run.status));
        }
        let message = self
            .controls
            .enqueue(NewControlMessage {
                message_id: AgentRunMessageId::new(),
                run_id: target.clone(),
                sender_run_id: actor.run_id.clone(),
                kind,
                payload,
                idempotency_key,
                causation_id: actor.run_id.as_ref().map(ToString::to_string),
            })
            .await?;
        self.append(
            target.clone(),
            AgentRunJournalEventKind::ControlQueued,
            Some(message.message_id.to_string()),
            None,
            [
                ("kind".into(), kind.as_str().into()),
                ("sequence".into(), message.sequence.to_string()),
            ],
        )
        .await?;
        if kind == AgentRunControlKind::Cancel {
            if let Err(error) = self.runs.request_cancel(&target).await {
                let current = self
                    .runs
                    .get_run(&target)
                    .await
                    .map_err(|e| RunControlError::RunStore(e.to_string()))?;
                if let Some(current) = current.filter(|run| run.status.is_terminal()) {
                    self.dispatch_pending(&target, false).await?;
                    return Ok(ControlOutcome::Terminal(current.status));
                }
                return Err(RunControlError::RunStore(error.to_string()));
            }
            if let Some(job_id) = run.job_id {
                if let Some(scheduler) = self.scheduler.lock().await.clone() {
                    if let Err(error) = scheduler.request_cancel(&job_id, "agent_run_control").await
                    {
                        tracing::warn!(
                            run_id = %target,
                            job_id = %job_id,
                            %error,
                            "scheduler cancel failed for agent run"
                        );
                    }
                }
            }
            self.append(
                target.clone(),
                AgentRunJournalEventKind::CancelRequested,
                Some(message.message_id.to_string()),
                None,
                [("source".into(), "run_control".into())],
            )
            .await?;
        }
        if let Ok(Some(task)) = self.runs.get_task(&run.task_id).await {
            crate::bus::global::GlobalEventBus::publish(
                crate::bus::events::AppEvent::AgentRunControlUpdated {
                    session_id: task.originating_session_id,
                    run_id: target.to_string(),
                    control_status: kind.as_str().into(),
                    cancellation_requested: kind == AgentRunControlKind::Cancel,
                },
            );
        }
        self.dispatch_pending(&target, false).await?;
        Ok(ControlOutcome::Queued(message))
    }

    pub async fn status(
        &self,
        actor: &ControlActor,
        target: AgentRunId,
    ) -> Result<ControlOutcome, RunControlError> {
        Ok(ControlOutcome::Status(
            self.authorize(actor, &target).await?,
        ))
    }

    pub async fn wait(
        &self,
        actor: &ControlActor,
        target: AgentRunId,
        timeout: Duration,
    ) -> Result<ControlOutcome, RunControlError> {
        let initial = self.authorize(actor, &target).await?;
        let timeout = timeout.min(Duration::from_millis(MAX_WAIT_MS));
        if initial.status.is_terminal() {
            return Ok(ControlOutcome::Wait {
                run: initial,
                timed_out: false,
            });
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let current = self.authorize(actor, &target).await?;
            if current.status.is_terminal() {
                return Ok(ControlOutcome::Wait {
                    run: current,
                    timed_out: false,
                });
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(ControlOutcome::Wait {
                    run: current,
                    timed_out: true,
                });
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn journal(
        &self,
        actor: &ControlActor,
        target: AgentRunId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<AgentRunJournalEvent>, RunControlError> {
        self.authorize(actor, &target).await?;
        Ok(self
            .controls
            .list_events(&target, after_sequence, limit)
            .await?)
    }

    pub async fn append(
        &self,
        run_id: AgentRunId,
        kind: AgentRunJournalEventKind,
        causation_id: Option<String>,
        correlation_id: Option<String>,
        metadata: impl IntoIterator<Item = (String, String)>,
    ) -> Result<AgentRunJournalEvent, RunControlError> {
        let metadata = metadata.into_iter().take(32).collect();
        Ok(self
            .controls
            .append_event(NewJournalEvent {
                event_id: AgentRunMessageId::new(),
                run_id,
                kind,
                causation_id,
                correlation_id,
                metadata,
            })
            .await?)
    }

    pub async fn record_terminal(
        &self,
        run_id: AgentRunId,
        status: AgentRunStatus,
        summary: &str,
    ) -> Result<(), RunControlError> {
        self.append(
            run_id.clone(),
            AgentRunJournalEventKind::CompletionProduced,
            None,
            None,
            [
                ("status".into(), status.as_str().into()),
                ("summary".into(), bounded(summary, MAX_NOTIFICATION_BYTES)),
            ],
        )
        .await?;
        let run = self
            .runs
            .get_run(&run_id)
            .await
            .map_err(|e| RunControlError::RunStore(e.to_string()))?
            .ok_or_else(|| RunControlError::RunNotFound(run_id.to_string()))?;
        let notice = format!(
            "Child run {} finished with status {}. Summary: {}",
            run_id,
            status.as_str(),
            bounded(summary, MAX_NOTIFICATION_BYTES)
        );
        if let Some(parent) = run.parent_run_id {
            if let Some(handle) = self.live.lock().await.get(&parent).cloned() {
                if let Err(error) = handle.follow_up_tx.try_send(notice.clone()) {
                    tracing::warn!(
                        run_id = %run_id,
                        parent_run_id = %parent,
                        %error,
                        "child completion notice dropped (parent channel full/closed)"
                    );
                }
            }
        } else if let Some(task) = self
            .runs
            .get_task(&run.task_id)
            .await
            .map_err(|e| RunControlError::RunStore(e.to_string()))?
        {
            if let Some(turn_id) = task.originating_turn_id {
                let owner = LiveTurnOwner {
                    session_id: task.originating_session_id,
                    turn_id,
                };
                if let Some(follow_up_tx) = self.live_turns.lock().await.get(&owner).cloned() {
                    if let Err(error) = follow_up_tx.try_send(notice) {
                        tracing::warn!(
                            run_id = %run_id,
                            %error,
                            "child completion notice dropped (turn channel full/closed)"
                        );
                    }
                }
            }
        }
        if let Some(groups) = self.groups.lock().await.clone() {
            match groups.member_changed_with_notifications(&run_id).await {
                Ok(summaries) => {
                    for (summary, notification_claimed) in summaries {
                        if let Err(error) = self.publish_group_projection(&summary).await {
                            tracing::warn!(
                                group_id = %summary.group.group_id,
                                %error,
                                "failed to publish run group projection"
                            );
                        }
                        if summary.group.status.is_terminal() && notification_claimed {
                            let notice = format!(
                                "Run group {} finished with status {}: {}/{} successful, {} failed.",
                                summary.group.group_id,
                                summary.group.status.as_str(),
                                summary.successful,
                                summary.members.len(),
                                summary.failed,
                            );
                            self.send_group_notice(&summary, notice).await?;
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(run_id = %run_id, %error, "failed to recompute run groups");
                }
            }
        }
        Ok(())
    }

    async fn dispatch_pending(
        &self,
        run_id: &AgentRunId,
        replay_delivered: bool,
    ) -> Result<(), RunControlError> {
        if let Some(run) = self
            .runs
            .get_run(run_id)
            .await
            .map_err(|e| RunControlError::RunStore(e.to_string()))?
        {
            if run.status.is_terminal() {
                for message in self.controls.pending(run_id).await? {
                    self.controls.supersede(&message.message_id).await?;
                }
                return Ok(());
            }
        }
        let Some(handle) = self.live.lock().await.get(run_id).cloned() else {
            return Ok(());
        };
        for message in self.controls.pending(run_id).await? {
            if message.state == MailboxState::Delivered && !replay_delivered {
                continue;
            }
            let delivered = self.controls.mark_delivered(&message.message_id).await?;
            let sent: Result<(), ()> = match delivered.kind {
                AgentRunControlKind::Message => handle
                    .follow_up_tx
                    .try_send(format!("Message from parent: {}", delivered.payload))
                    .map_err(|_| ()),
                AgentRunControlKind::Interrupt => {
                    handle.interrupt_flag.store(true, Ordering::SeqCst);
                    handle.steer_tx.try_send("Interrupt requested by the owning parent; reconsider before the next safe action.".into()).map_err(|_| ())
                }
                AgentRunControlKind::Cancel => handle.cancel_tx.send(true).map_err(|_| ()),
            };
            if sent.is_err() {
                return Err(RunControlError::ChannelClosed);
            }
            self.controls.acknowledge(&delivered.message_id).await?;
            if let Err(error) = self
                .append(
                    run_id.clone(),
                    AgentRunJournalEventKind::ControlDelivered,
                    Some(delivered.message_id.to_string()),
                    None,
                    [("sequence".into(), delivered.sequence.to_string())],
                )
                .await
            {
                tracing::warn!(
                    run_id = %run_id,
                    message_id = %delivered.message_id,
                    %error,
                    "failed to journal control delivery"
                );
            }
        }
        Ok(())
    }

    async fn send_group_notice(
        &self,
        summary: &codegg_core::agent_run_group::AgentRunGroupSummary,
        notice: String,
    ) -> Result<(), RunControlError> {
        match &summary.group.owner {
            codegg_core::agent_run_group::AgentRunGroupOwner::Run { run_id } => {
                if let Some(handle) = self.live.lock().await.get(run_id).cloned() {
                    if let Err(error) = handle.follow_up_tx.try_send(notice) {
                        tracing::warn!(
                            group_id = %summary.group.group_id,
                            %error,
                            "group completion notice dropped (run channel full/closed)"
                        );
                    }
                }
            }
            codegg_core::agent_run_group::AgentRunGroupOwner::Turn {
                session_id,
                turn_id,
            } => {
                let owner = LiveTurnOwner {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                };
                if let Some(follow_up_tx) = self.live_turns.lock().await.get(&owner).cloned() {
                    if let Err(error) = follow_up_tx.try_send(notice) {
                        tracing::warn!(
                            group_id = %summary.group.group_id,
                            %error,
                            "group completion notice dropped (turn channel full/closed)"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    async fn publish_group_projection(
        &self,
        summary: &codegg_core::agent_run_group::AgentRunGroupSummary,
    ) -> Result<(), RunControlError> {
        let session_id = match &summary.group.owner {
            codegg_core::agent_run_group::AgentRunGroupOwner::Turn { session_id, .. } => {
                session_id.clone()
            }
            codegg_core::agent_run_group::AgentRunGroupOwner::Run { run_id } => {
                let owner = self
                    .runs
                    .get_run(run_id)
                    .await
                    .map_err(|e| RunControlError::RunStore(e.to_string()))?
                    .ok_or_else(|| RunControlError::RunNotFound(run_id.to_string()))?;
                self.runs
                    .get_task(&owner.task_id)
                    .await
                    .map_err(|e| RunControlError::RunStore(e.to_string()))?
                    .map(|task| task.originating_session_id)
                    .ok_or_else(|| RunControlError::RunNotFound(owner.task_id.to_string()))?
            }
        };
        crate::bus::global::GlobalEventBus::publish(
            crate::bus::events::AppEvent::AgentRunGroupUpdated {
                session_id,
                group: codegg_core::projection_replay::run_group_summary(
                    summary,
                    chrono::Utc::now().timestamp_millis(),
                ),
            },
        );
        Ok(())
    }
}

fn bounded(value: &str, max: usize) -> String {
    value
        .chars()
        .scan(0usize, |used, ch| {
            let next = *used + ch.len_utf8();
            if next > max {
                None
            } else {
                *used = next;
                Some(ch)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::agent_run::{AgentRunBudget, AgentRunStore, NewAgentRun, NewAgentTask};
    use codegg_core::agent_run_control::InMemoryAgentRunControlStore;
    use codegg_core::identity::{AgentTaskId, ProjectId};
    use codegg_core::workspace::WorkspaceId;

    async fn running_run(runs: &Arc<dyn AgentRunStore>, session: &str) -> AgentRunRecord {
        let task_id = AgentTaskId::new();
        let run_id = AgentRunId::new();
        let submission = runs
            .create_or_get(
                NewAgentTask {
                    task_id,
                    parent_task_id: None,
                    originating_session_id: session.into(),
                    originating_turn_id: Some("owner-turn".into()),
                    project_id: ProjectId::new(),
                    repository_id: None,
                    workspace_id: WorkspaceId::new_unchecked("control-test-workspace"),
                    requested_agent: "general".into(),
                    delegation_key: format!("control-test-{run_id}"),
                    request_fingerprint: format!("control-test-{run_id}"),
                    description: "control test".into(),
                },
                NewAgentRun {
                    run_id,
                    parent_run_id: None,
                    depth: 1,
                    workspace_id: WorkspaceId::new_unchecked("control-test-workspace"),
                    agent_name: "general".into(),
                    agent_digest: None,
                    provider: "test".into(),
                    model: "test".into(),
                    authority_digest: "authority".into(),
                    budget: AgentRunBudget::default(),
                },
            )
            .await
            .unwrap();
        runs.transition_task(
            &submission.task.task_id,
            codegg_core::agent_run::AgentTaskStatus::Queued,
        )
        .await
        .unwrap();
        runs.transition(&submission.run.run_id, AgentRunStatus::Queued)
            .await
            .unwrap();
        runs.transition(&submission.run.run_id, AgentRunStatus::Preparing)
            .await
            .unwrap();
        runs.transition(&submission.run.run_id, AgentRunStatus::Running)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn controls_are_authorized_delivered_at_boundary_and_restart_safe() {
        let runs: Arc<dyn AgentRunStore> =
            Arc::new(codegg_core::agent_run::InMemoryAgentRunStore::new());
        let run = running_run(&runs, "owner-session").await;
        let controls: Arc<dyn AgentRunControlStore> = Arc::new(InMemoryAgentRunControlStore::new());
        let service = RunControlService::with_control_store(runs.clone(), controls.clone());
        let (follow_tx, mut follow_rx) = mpsc::channel(4);
        let (steer_tx, mut steer_rx) = mpsc::channel(4);
        let (cancel_tx, _cancel_rx) = watch::channel(false);
        let flag = Arc::new(AtomicBool::new(false));
        service
            .register_live(
                run.run_id.clone(),
                LiveRunHandle {
                    follow_up_tx: follow_tx,
                    steer_tx,
                    cancel_tx,
                    interrupt_flag: flag.clone(),
                },
            )
            .await;
        service
            .send(
                &ControlActor {
                    session_id: Some("owner-session".into()),
                    turn_id: Some("owner-turn".into()),
                    run_id: None,
                },
                run.run_id.clone(),
                AgentRunControlKind::Message,
                "continue".into(),
                "message-1".into(),
            )
            .await
            .unwrap();
        assert_eq!(
            follow_rx.recv().await.unwrap(),
            "Message from parent: continue"
        );
        service
            .send(
                &ControlActor {
                    session_id: Some("owner-session".into()),
                    turn_id: Some("owner-turn".into()),
                    run_id: None,
                },
                run.run_id.clone(),
                AgentRunControlKind::Interrupt,
                String::new(),
                "interrupt-1".into(),
            )
            .await
            .unwrap();
        assert!(flag.load(Ordering::SeqCst));
        assert!(steer_rx
            .recv()
            .await
            .unwrap()
            .contains("Interrupt requested"));
        assert!(matches!(
            service
                .send(
                    &ControlActor {
                        session_id: Some("other".into()),
                        turn_id: Some("other-turn".into()),
                        run_id: None
                    },
                    run.run_id.clone(),
                    AgentRunControlKind::Message,
                    "no".into(),
                    "bad".into()
                )
                .await,
            Err(RunControlError::Unauthorized)
        ));
        service.unregister_live(&run.run_id).await;
        service
            .send(
                &ControlActor {
                    session_id: Some("owner-session".into()),
                    turn_id: Some("owner-turn".into()),
                    run_id: None,
                },
                run.run_id.clone(),
                AgentRunControlKind::Message,
                "after restart".into(),
                "restart-1".into(),
            )
            .await
            .unwrap();
        let restarted = RunControlService::with_control_store(runs.clone(), controls);
        let (follow_tx, mut follow_rx) = mpsc::channel(4);
        let (steer_tx, _steer_rx) = mpsc::channel(4);
        let (cancel_tx, _cancel_rx) = watch::channel(false);
        restarted
            .register_live(
                run.run_id.clone(),
                LiveRunHandle {
                    follow_up_tx: follow_tx,
                    steer_tx,
                    cancel_tx,
                    interrupt_flag: Arc::new(AtomicBool::new(false)),
                },
            )
            .await;
        assert_eq!(
            follow_rx.recv().await.unwrap(),
            "Message from parent: after restart"
        );
    }

    #[tokio::test]
    async fn wait_timeout_and_terminal_race_are_bounded() {
        let runs: Arc<dyn AgentRunStore> =
            Arc::new(codegg_core::agent_run::InMemoryAgentRunStore::new());
        let run = running_run(&runs, "owner-session").await;
        let service = RunControlService::in_memory(runs.clone());
        let actor = ControlActor {
            session_id: Some("owner-session".into()),
            turn_id: Some("owner-turn".into()),
            run_id: None,
        };
        let outcome = service
            .wait(&actor, run.run_id.clone(), Duration::from_millis(1))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ControlOutcome::Wait {
                timed_out: true,
                ..
            }
        ));
        runs.finish(
            &run.run_id,
            codegg_core::agent_run::AgentRunTerminalOutcome::Completed,
            Some("done".into()),
            None,
            None,
        )
        .await
        .unwrap();
        let outcome = service
            .wait(&actor, run.run_id, Duration::from_secs(1))
            .await
            .unwrap();
        assert!(
            matches!(outcome, ControlOutcome::Wait { timed_out: false, run } if run.status == AgentRunStatus::Completed)
        );
    }

    #[tokio::test]
    async fn top_level_completion_routes_to_the_exact_live_originating_turn() {
        let runs: Arc<dyn AgentRunStore> =
            Arc::new(codegg_core::agent_run::InMemoryAgentRunStore::new());
        let run = running_run(&runs, "owner-session").await;
        let service = RunControlService::in_memory(runs.clone());
        let (owner_tx, mut owner_rx) = mpsc::channel(4);
        let (other_tx, mut other_rx) = mpsc::channel(4);
        service
            .register_live_turn("owner-session".into(), "owner-turn".into(), owner_tx)
            .await;
        service
            .register_live_turn("owner-session".into(), "other-turn".into(), other_tx)
            .await;

        runs.finish(
            &run.run_id,
            codegg_core::agent_run::AgentRunTerminalOutcome::Completed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        service
            .record_terminal(run.run_id, AgentRunStatus::Completed, "done")
            .await
            .unwrap();

        assert!(owner_rx.recv().await.unwrap().contains("finished"));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), other_rx.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn turn_owned_group_publishes_and_notifies_once() {
        let runs: Arc<dyn AgentRunStore> =
            Arc::new(codegg_core::agent_run::InMemoryAgentRunStore::new());
        let first = running_run(&runs, "owner-session").await;
        let second = running_run(&runs, "owner-session").await;
        let groups = codegg_core::agent_run_group::AgentRunGroupService::in_memory(runs.clone());
        let groups_for_claim = groups.clone();
        let created = groups
            .create(codegg_core::agent_run_group::NewAgentRunGroup {
                group_id: codegg_core::identity::AgentRunGroupId::new(),
                root_run_id: first.run_id.clone(),
                owner_run_id: first.run_id.clone(),
                owner: codegg_core::agent_run_group::AgentRunGroupOwner::Turn {
                    session_id: "owner-session".into(),
                    turn_id: "owner-turn".into(),
                },
                member_run_ids: vec![first.run_id.clone(), second.run_id.clone()],
                join_policy: codegg_core::agent_run_group::RunJoinPolicy::All,
                cancel_remaining_on_satisfaction: false,
                idempotency_key: "turn-group-test".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            created.group.status,
            codegg_core::agent_run_group::RunGroupStatus::Running
        );

        let service = RunControlService::in_memory(runs.clone());
        service.set_group_service_sync(groups);
        let (follow_tx, mut follow_rx) = mpsc::channel(8);
        service
            .register_live_turn("owner-session".into(), "owner-turn".into(), follow_tx)
            .await;
        let mut events = crate::bus::global::GlobalEventBus::subscribe();

        for run in [&first, &second] {
            runs.finish(
                &run.run_id,
                codegg_core::agent_run::AgentRunTerminalOutcome::Completed,
                None,
                None,
                None,
            )
            .await
            .unwrap();
            service
                .record_terminal(run.run_id.clone(), AgentRunStatus::Completed, "done")
                .await
                .unwrap();
        }

        let notices = [
            follow_rx.recv().await.unwrap(),
            follow_rx.recv().await.unwrap(),
            follow_rx.recv().await.unwrap(),
        ];
        assert!(notices.iter().any(|notice| notice.contains("Run group")));
        assert!(notices.iter().any(|notice| notice.contains("Child run")));

        let projection = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let crate::bus::events::AppEvent::AgentRunGroupUpdated { group, .. } =
                    events.recv().await.unwrap()
                {
                    if group.group_id == created.group.group_id.to_string()
                        && group.status == "completed"
                    {
                        break group;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(projection.status, "completed");

        let (_, claimed) = groups_for_claim
            .member_changed_with_notifications(&second.run_id)
            .await
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert!(!claimed);
    }

    #[tokio::test]
    async fn authorization_is_direct_owner_scoped() {
        let runs: Arc<dyn AgentRunStore> =
            Arc::new(codegg_core::agent_run::InMemoryAgentRunStore::new());
        let parent = running_run(&runs, "owner-session").await;
        let parent_task = runs.get_task(&parent.task_id).await.unwrap().unwrap();
        let child = runs
            .create_or_get(
                NewAgentTask {
                    task_id: AgentTaskId::new(),
                    parent_task_id: Some(parent.task_id.clone()),
                    originating_session_id: "owner-session".into(),
                    originating_turn_id: Some("owner-turn".into()),
                    project_id: parent_task.project_id.clone(),
                    repository_id: None,
                    workspace_id: parent.workspace_id.clone(),
                    requested_agent: "general".into(),
                    delegation_key: "control-child".into(),
                    request_fingerprint: "control-child".into(),
                    description: "child".into(),
                },
                NewAgentRun {
                    run_id: AgentRunId::new(),
                    parent_run_id: Some(parent.run_id.clone()),
                    depth: 2,
                    workspace_id: parent.workspace_id.clone(),
                    agent_name: "general".into(),
                    agent_digest: None,
                    provider: "test".into(),
                    model: "test".into(),
                    authority_digest: "authority".into(),
                    budget: AgentRunBudget::default(),
                },
            )
            .await
            .unwrap();
        let service = RunControlService::in_memory(runs.clone());
        let parent_actor = ControlActor {
            session_id: None,
            turn_id: None,
            run_id: Some(parent.run_id.clone()),
        };
        assert!(service
            .authorize(&parent_actor, &child.run.run_id)
            .await
            .is_ok());
        assert!(matches!(
            service.authorize(&parent_actor, &parent.run_id).await,
            Err(RunControlError::Unauthorized)
        ));
        let child_actor = ControlActor {
            session_id: None,
            turn_id: None,
            run_id: Some(child.run.run_id.clone()),
        };
        assert!(matches!(
            service.authorize(&child_actor, &parent.run_id).await,
            Err(RunControlError::Unauthorized)
        ));
        assert!(matches!(
            service
                .authorize(
                    &ControlActor {
                        session_id: Some("owner-session".into()),
                        turn_id: None,
                        run_id: None,
                    },
                    &parent.run_id,
                )
                .await,
            Err(RunControlError::Unauthorized)
        ));
    }

    #[tokio::test]
    async fn sqlite_control_state_is_available_after_service_recreation() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        codegg_core::session::schema::migrate(&pool).await.unwrap();
        let runs: Arc<dyn AgentRunStore> = Arc::new(
            codegg_core::agent_run::SqliteAgentRunStore::new(pool.clone()),
        );
        let run = running_run(&runs, "sqlite-owner").await;
        let service = RunControlService::with_pool(runs.clone(), pool.clone());
        service
            .send(
                &ControlActor {
                    session_id: Some("sqlite-owner".into()),
                    turn_id: Some("owner-turn".into()),
                    run_id: None,
                },
                run.run_id.clone(),
                AgentRunControlKind::Message,
                "survives".into(),
                "sqlite-retry".into(),
            )
            .await
            .unwrap();
        service
            .append(
                run.run_id.clone(),
                AgentRunJournalEventKind::SafeBoundary,
                None,
                None,
                [("boundary".into(), "test".into())],
            )
            .await
            .unwrap();
        drop(service);
        let recreated = RunControlService::with_pool(runs, pool);
        assert_eq!(
            recreated.controls().pending(&run.run_id).await.unwrap()[0].payload,
            "survives"
        );
        assert_eq!(
            recreated
                .journal(
                    &ControlActor {
                        session_id: Some("sqlite-owner".into()),
                        turn_id: Some("owner-turn".into()),
                        run_id: None,
                    },
                    run.run_id,
                    0,
                    10,
                )
                .await
                .unwrap()
                .len(),
            2
        );
    }
}

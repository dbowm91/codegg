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

#[derive(Debug, Clone, Default)]
pub struct ControlActor {
    pub session_id: Option<String>,
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
    scheduler: Mutex<Option<Arc<crate::scheduler::JobScheduler>>>,
}

impl RunControlService {
    pub fn in_memory(runs: Arc<dyn AgentRunStore>) -> Arc<Self> {
        Arc::new(Self {
            runs,
            controls: Arc::new(InMemoryAgentRunControlStore::new()),
            live: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(None),
        })
    }

    pub fn with_pool(runs: Arc<dyn AgentRunStore>, pool: sqlx::SqlitePool) -> Arc<Self> {
        Arc::new(Self {
            runs,
            controls: Arc::new(SqliteAgentRunControlStore::new(pool)),
            live: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(None),
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
            scheduler: Mutex::new(None),
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

    pub async fn register_live(&self, run_id: AgentRunId, handle: LiveRunHandle) {
        self.live.lock().await.insert(run_id.clone(), handle);
        if let Err(error) = self.dispatch_pending(&run_id, true).await {
            tracing::warn!(run_id = %run_id, %error, "failed to replay run controls on live attach");
        }
    }

    pub async fn unregister_live(&self, run_id: &AgentRunId) {
        self.live.lock().await.remove(run_id);
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
        if actor
            .session_id
            .as_deref()
            .is_some_and(|session| session == task.originating_session_id)
        {
            return Ok(target_run);
        }
        let Some(mut ancestor) = actor.run_id.clone() else {
            return Err(RunControlError::Unauthorized);
        };
        loop {
            if ancestor == *target {
                return Err(RunControlError::Unauthorized);
            }
            let current = self
                .runs
                .get_run(&ancestor)
                .await
                .map_err(|e| RunControlError::RunStore(e.to_string()))?
                .ok_or(RunControlError::Unauthorized)?;
            if current.parent_run_id.as_ref() == Some(target) {
                return Ok(target_run);
            }
            let Some(parent) = current.parent_run_id else {
                return Err(RunControlError::Unauthorized);
            };
            ancestor = parent;
        }
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
                    let _ = self.dispatch_pending(&target, false).await;
                    return Ok(ControlOutcome::Terminal(current.status));
                }
                return Err(RunControlError::RunStore(error.to_string()));
            }
            if let Some(job_id) = run.job_id {
                if let Some(scheduler) = self.scheduler.lock().await.clone() {
                    let _ = scheduler.request_cancel(&job_id, "agent_run_control").await;
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
        let parent = self
            .runs
            .get_run(&run_id)
            .await
            .map_err(|e| RunControlError::RunStore(e.to_string()))?
            .and_then(|run| run.parent_run_id);
        if let Some(parent) = parent {
            if let Some(handle) = self.live.lock().await.get(&parent).cloned() {
                let notice = format!(
                    "Child run {} finished with status {}. Summary: {}",
                    run_id,
                    status.as_str(),
                    bounded(summary, MAX_NOTIFICATION_BYTES)
                );
                let _ = handle.follow_up_tx.try_send(notice);
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
                    let _ = self.controls.supersede(&message.message_id).await?;
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
                AgentRunControlKind::Cancel => {
                    let _ = handle.cancel_tx.send(true);
                    Ok(())
                }
            };
            if sent.is_err() {
                return Err(RunControlError::ChannelClosed);
            }
            self.controls.acknowledge(&delivered.message_id).await?;
            let _ = self
                .append(
                    run_id.clone(),
                    AgentRunJournalEventKind::ControlDelivered,
                    Some(delivered.message_id.to_string()),
                    None,
                    [("sequence".into(), delivered.sequence.to_string())],
                )
                .await;
        }
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
                    originating_turn_id: None,
                    project_id: ProjectId::new(),
                    repository_id: None,
                    workspace_id: WorkspaceId::new_unchecked("control-test-workspace"),
                    requested_agent: "general".into(),
                    delegation_key: format!("control-test-{run_id}"),
                    description: "control test".into(),
                },
                NewAgentRun {
                    run_id,
                    parent_run_id: None,
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

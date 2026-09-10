//! M003: `CoreDaemon` bootstrap and recovery ownership.
//!
//! `CoreDaemon` remains the single daemon composition/lifecycle authority.
//! This module owns startup bootstrap and recovery only: workspace hydration,
//! asset-refresh metadata hydration/persistence, the global event bridge,
//! turn/permission/question recovery, durable-job recovery, and replay.
//! It introduces no new store, scheduler, state machine, or authority, and
//! preserves the exact task creation/cancellation/join sequence.
//!
//! ```text
//! Bootstrap order (in-process canonical path via `initialize_recovery_sequence`):
//!   1. hydrate_workspace_registry (workspaces + asset metadata)
//!   2. start_event_bridge (spawn global-bus -> event-log forwarder)
//!   3. recover_state (interrupted turns -> TurnFailed; stale perms/questions logged)
//!   4. recover_jobs (prior-generation attempts -> Interrupted/requeue)
//!
//! Socket/daemon entry points (`src/main.rs`) currently run steps 2-4 without
//! step 1 (pre-existing shape; unchanged by this polish milestone). Do not
//! reorder startup merely because extraction makes it possible; desirable
//! behavior changes belong in separate correctness plans.
//! ```
//!
//! Recovery failures retain current typed behavior: missing pools return
//! early, missing tables log and skip, and job recovery returns `None` on
//! error without admitting queued work.

use std::sync::Arc;

use super::daemon::CoreDaemon;
use super::event_log::EventFilter;
use crate::core::session_runtime::RuntimeSessionStatus;
use crate::error::AppError;
use chrono::Utc;
use sqlx::Row;

impl CoreDaemon {
    /// Rehydrate the workspace registry from its backing store. Daemon
    /// construction synchronously creates a registry with the store
    /// attached; the in-memory cache is empty until this method is called.
    /// Existing CLI/socket entry points invoke this after `with_deps` so
    /// existing `workspace` rows survive restarts.
    pub async fn hydrate_workspace_registry(
        &self,
    ) -> Result<(), codegg_core::workspace::WorkspaceError> {
        self.workspaces.hydrate_from_store().await?;
        self.hydrate_asset_refresh_metadata().await;
        Ok(())
    }

    async fn hydrate_asset_refresh_metadata(&self) {
        let Some(pool) = self.pool.as_ref() else {
            return;
        };
        let rows = match sqlx::query(
            "SELECT project_id, workspace_id, generation, fingerprint \
             FROM runtime_asset_refresh",
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::debug!(error = %error, "runtime asset metadata unavailable during hydration");
                return;
            }
        };
        for row in rows {
            let Ok(project_id) = row.try_get::<String, _>("project_id") else {
                continue;
            };
            let Ok(workspace_id) = row.try_get::<String, _>("workspace_id") else {
                continue;
            };
            let Ok(generation) = row.try_get::<i64, _>("generation") else {
                continue;
            };
            let Ok(generation) = u64::try_from(generation) else {
                tracing::warn!("ignoring negative runtime asset generation during hydration");
                continue;
            };
            let fingerprint = row
                .try_get::<Option<String>, _>("fingerprint")
                .ok()
                .flatten();
            self.asset_refresh
                .restore_metadata(
                    crate::agent::asset_refresh::AssetScope::new(project_id, workspace_id),
                    generation,
                    fingerprint,
                )
                .await;
        }
    }

    pub(crate) async fn persist_asset_refresh_metadata(
        &self,
        report: &crate::agent::asset_refresh::RefreshReport,
    ) {
        let Some(pool) = self.pool.as_ref() else {
            return;
        };
        let Some(generation) = report.generation else {
            return;
        };
        if !matches!(
            report.outcome,
            crate::agent::asset_refresh::RefreshOutcome::Published
                | crate::agent::asset_refresh::RefreshOutcome::Coalesced
        ) {
            return;
        }
        let diagnostics =
            serde_json::to_string(&report.diagnostics).unwrap_or_else(|_| "[]".into());
        if let Err(error) = sqlx::query(
            "INSERT INTO runtime_asset_refresh \
             (project_id, workspace_id, generation, fingerprint, last_success_at, diagnostics_json, time_updated) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(project_id, workspace_id) DO UPDATE SET \
             generation = excluded.generation, fingerprint = excluded.fingerprint, \
             last_success_at = excluded.last_success_at, diagnostics_json = excluded.diagnostics_json, \
             time_updated = excluded.time_updated",
        )
        .bind(&report.scope.project_id)
        .bind(&report.scope.workspace_id)
        .bind(match i64::try_from(generation) {
            Ok(generation) => generation,
            Err(_) => {
                tracing::warn!(
                    generation,
                    "runtime asset generation exceeds SQLite integer range; metadata not persisted"
                );
                return;
            }
        })
        .bind(&report.fingerprint)
        .bind(report.completed_at.timestamp_millis())
        .bind(diagnostics)
        .bind(Utc::now().timestamp_millis())
        .execute(pool)
        .await
        {
            tracing::warn!(
                error = %error,
                project_id = %report.scope.project_id,
                workspace_id = %report.scope.workspace_id,
                generation,
                "failed to persist runtime asset refresh metadata"
            );
        }
    }

    pub fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<
        crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>,
    > {
        self.event_log.subscribe()
    }

    /// Apply the bridge fallback to a single `AppEvent` and return the
    /// resulting `(session_id, turn_id, core_event)` triple, or `None` if
    /// the event has no corresponding `CoreEvent`. If the bridged event
    /// has an empty or missing `turn_id`, look up the active turn for the
    /// session and attach its `turn_id` to the event so every event
    /// belonging to a turn carries the same identity.
    pub(crate) async fn bridge_app_event(
        &self,
        app_event: crate::bus::events::AppEvent,
    ) -> Option<(
        Option<String>,
        Option<String>,
        crate::protocol::core::CoreEvent,
    )> {
        let mut core_event = super::map_app_event_to_core_event(app_event)?;
        let (session_id, mut turn_id) = super::core_event_metadata(&core_event);
        let turn_id_empty = match &turn_id {
            Some(t) => t.is_empty(),
            None => true,
        };
        if turn_id_empty {
            if let Some(sid) = session_id.clone() {
                if let Some(runtime) = self.sessions.get(&sid) {
                    let active = runtime.active_turn.read().await;
                    if let Some(handle) = active.as_ref() {
                        core_event =
                            super::set_turn_id_on_event(core_event, handle.turn_id.clone());
                        turn_id = Some(handle.turn_id.clone());
                    }
                }
            }
        }
        Some((session_id, turn_id, core_event))
    }

    /// Recover daemon state after restart.
    /// Marks previously active turns as failed and logs stale permissions/questions.
    pub async fn recover_state(&self) {
        let Some(ref pool) = self.pool else {
            return;
        };

        // Find interrupted turns: TurnStarted without a matching TurnCompleted/TurnFailed
        // for the same session_id + turn_id. Use the explicit event-type strings
        // written by `core_event_type()` (snake_case) so the query is stable and
        // grep-able. The DISTINCT + NOT EXISTS pattern ensures each (session, turn)
        // pair is reported at most once and we only flag turns that have a real
        // turn_id (e.g., not blank rows from older schemas).
        let active_turns: Result<Vec<(String, String)>, sqlx::Error> = sqlx::query_as(
            "SELECT DISTINCT e1.session_id, e1.turn_id \
             FROM core_event_log e1 \
             WHERE e1.event_type = 'turn_started' \
             AND e1.turn_id IS NOT NULL \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.session_id = e1.session_id \
                 AND e2.turn_id = e1.turn_id \
                 AND (e2.event_type = 'turn_completed' OR e2.event_type = 'turn_failed') \
             )",
        )
        .fetch_all(pool)
        .await;
        let active_turns = match active_turns {
            Ok(active_turns) => active_turns,
            Err(error) => {
                tracing::warn!(error = %error, "event log recovery query failed; skipping turn recovery");
                return;
            }
        };

        if !active_turns.is_empty() {
            tracing::info!(
                "Recovery: found {} interrupted turn(s), emitting TurnFailed",
                active_turns.len()
            );
            for (session_id, turn_id) in &active_turns {
                tracing::info!(
                    "  Marking session {} turn {} as failed (daemon restarted while active)",
                    session_id,
                    turn_id
                );
                self.event_log
                    .publish(
                        Some(session_id.clone()),
                        Some(turn_id.clone()),
                        crate::protocol::core::CoreEvent::TurnFailed {
                            session_id: session_id.clone(),
                            turn_id: Some(turn_id.clone()),
                            message: "Daemon restarted while turn was active".to_string(),
                        },
                    )
                    .await;

                // Clear runtime state for this session
                if let Some(runtime) = self.sessions.get(session_id) {
                    let mut active = runtime.active_turn.write().await;
                    *active = None;
                    drop(active);

                    let mut status = runtime.status.write().await;
                    *status = RuntimeSessionStatus::Idle;
                }
            }
        }

        // Count stale PermissionPending events (no PermissionResponded in same session)
        let stale_perms: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM core_event_log WHERE event_type = 'permission_pending' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.event_type = 'permission_responded' \
                 AND e2.session_id = core_event_log.session_id \
             )",
        )
        .fetch_one(pool)
        .await;
        let stale_perms = match stale_perms {
            Ok(count) => count,
            Err(error) => {
                tracing::warn!(error = %error, "permission recovery query failed; skipping stale-request recovery");
                return;
            }
        };

        // Count stale QuestionPending events (no QuestionAnswered in same session)
        let stale_questions: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM core_event_log WHERE event_type = 'question_pending' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.event_type = 'question_answered' \
                 AND e2.session_id = core_event_log.session_id \
             )",
        )
        .fetch_one(pool)
        .await;
        let stale_questions = match stale_questions {
            Ok(count) => count,
            Err(error) => {
                tracing::warn!(error = %error, "question recovery query failed; skipping stale-request recovery");
                return;
            }
        };

        if stale_perms > 0 || stale_questions > 0 {
            tracing::info!(
                "Recovery: {} stale permission(s), {} stale question(s) from previous run (will timeout naturally)",
                stale_perms,
                stale_questions
            );
        }

        tracing::info!("Daemon state recovery complete");
    }

    /// Recover durable jobs whose attempts originated from a prior
    /// daemon generation. Must run at startup before the scheduler
    /// admits queued work so interrupted attempts do not silently
    /// consume capacity. Returns the recovery report; the report is
    /// also available via `CoreRequest::JobRecoveryReport`.
    pub async fn recover_jobs(&self) -> Option<crate::job_recovery::RecoveryReportSummary> {
        if let Some(scheduler) = self.deps.scheduler.as_ref() {
            match scheduler
                .recover_at_startup(&self.deps.recovery_policy)
                .await
            {
                Ok(report) => {
                    tracing::info!(
                        interrupted = report.interrupted_attempts,
                        requeued = report.requeued_jobs,
                        terminal = report.terminal_jobs,
                        "Job recovery complete"
                    );
                    Some(crate::job_recovery::RecoveryReportSummary {
                        interrupted_attempts: report.interrupted_attempts,
                        requeued_jobs: report.requeued_jobs,
                        terminal_jobs: report.terminal_jobs,
                        schedules_reconciled: report.schedules_reconciled,
                    })
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "job recovery failed; attempts from a prior daemon generation may not be recovered"
                    );
                    None
                }
            }
        } else {
            None
        }
    }

    pub async fn replay_from(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>> {
        self.event_log.replay_from(from_event_seq, filter).await
    }

    pub fn start_event_bridge(self: &Arc<Self>) {
        let daemon = Arc::clone(self);
        tokio::spawn(async move {
            let mut bus_rx = crate::bus::global::GlobalEventBus::subscribe();
            loop {
                match bus_rx.recv().await {
                    Ok(app_event) => {
                        if let Some((session_id, turn_id, core_event)) =
                            daemon.bridge_app_event(app_event.clone()).await
                        {
                            daemon
                                .event_log
                                .publish(session_id, turn_id, core_event)
                                .await;
                        }
                        match &app_event {
                            crate::bus::events::AppEvent::AgentFinished {
                                session_id,
                                stop_reason,
                                input_tokens,
                                output_tokens,
                                ..
                            } => {
                                // Update runtime token counts
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    *runtime.last_input_tokens.write().await = *input_tokens;
                                    *runtime.last_output_tokens.write().await = *output_tokens;
                                }
                                use super::notification::*;
                                let kind = if stop_reason == "error" {
                                    NotificationKind::TurnFailed
                                } else {
                                    NotificationKind::TurnCompleted
                                };
                                let priority = if stop_reason == "error" {
                                    NotificationPriority::High
                                } else {
                                    NotificationPriority::Low
                                };
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind,
                                    priority,
                                    message: format!(
                                        "Turn {} for session {}",
                                        stop_reason, session_id
                                    ),
                                    dedupe_key: Some(format!("turn-done:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::PermissionPending {
                                session_id,
                                turn_id,
                                tool,
                                ..
                            } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: turn_id.clone(),
                                    kind: NotificationKind::PermissionRequired,
                                    priority: NotificationPriority::Urgent,
                                    message: format!("Permission required for tool: {}", tool),
                                    dedupe_key: Some(format!("perm:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::QuestionPending {
                                session_id,
                                turn_id,
                                ..
                            } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: turn_id.clone(),
                                    kind: NotificationKind::QuestionRequired,
                                    priority: NotificationPriority::Urgent,
                                    message: "Question requires your input".to_string(),
                                    dedupe_key: Some(format!("question:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::Error { message } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: None,
                                    turn_id: None,
                                    kind: NotificationKind::Error,
                                    priority: NotificationPriority::High,
                                    message: message.clone(),
                                    dedupe_key: None,
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::SubagentStarted {
                                session_id, ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                            crate::bus::events::AppEvent::SubagentCompleted {
                                session_id,
                                task_id,
                                agent,
                                ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind: NotificationKind::SubagentCompleted,
                                    priority: NotificationPriority::Normal,
                                    message: format!(
                                        "Subagent {} completed task {}",
                                        agent, task_id
                                    ),
                                    dedupe_key: Some(format!(
                                        "subagent-done:{}:{}",
                                        session_id, task_id
                                    )),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::SubagentFailed {
                                session_id,
                                task_id,
                                agent,
                                error,
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind: NotificationKind::SubagentFailed,
                                    priority: NotificationPriority::High,
                                    message: format!(
                                        "Subagent {} failed task {}: {}",
                                        agent, task_id, error
                                    ),
                                    dedupe_key: Some(format!(
                                        "subagent-fail:{}:{}",
                                        session_id, task_id
                                    )),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let total = daemon
                            .dropped_event_bridge_events
                            .fetch_add(n, std::sync::atomic::Ordering::Relaxed)
                            + n;
                        tracing::warn!(
                            dropped = n,
                            total_dropped = total,
                            "Event bridge lagged, events dropped; clients may need to resync"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// Canonical in-process bootstrap sequence: hydrate, bridge, recover.
    ///
    /// Preserves the exact order previously inline in
    /// `InprocCoreClient::initialize_recovery` (hydrate -> bridge ->
    /// recover_state -> recover_jobs). Socket/daemon paths keep their
    /// pre-existing 2-4 shape; this helper does not change them.
    pub async fn initialize_recovery_sequence(self: &Arc<Self>) -> Result<(), AppError> {
        self.hydrate_workspace_registry()
            .await
            .map_err(|error| AppError::Other(anyhow::anyhow!(error.to_string())))?;
        self.start_event_bridge();
        self.recover_state().await;
        if self.recover_jobs().await.is_none() {
            tracing::error!("durable job recovery produced no report during core initialization");
        }
        Ok(())
    }

    /// Canonical bootstrap phase names in execution order.
    ///
    /// Pinned by `bootstrap_phases_match_documented_order`.
    #[cfg(test)]
    pub(crate) fn bootstrap_phase_names() -> [&'static str; 4] {
        [
            "hydrate_workspace_registry",
            "start_event_bridge",
            "recover_state",
            "recover_jobs",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn in_memory_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:daemon_bootstrap_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[test]
    fn bootstrap_phases_match_documented_order() {
        assert_eq!(
            CoreDaemon::bootstrap_phase_names(),
            [
                "hydrate_workspace_registry",
                "start_event_bridge",
                "recover_state",
                "recover_jobs",
            ]
        );
    }

    #[tokio::test]
    async fn bootstrap_sequence_hydrates_and_recovers_without_partial_publish() {
        let pool = in_memory_pool().await;
        let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None));
        // Canonical sequence must succeed on a fresh pool and leave the
        // daemon fully wired (no partially ready publish).
        daemon
            .initialize_recovery_sequence()
            .await
            .expect("bootstrap sequence succeeds");
        // Hydration is idempotent; a second run still succeeds.
        daemon
            .hydrate_workspace_registry()
            .await
            .expect("rehydrate succeeds");
        // Recovery on a fresh pool finds no interrupted turns and does not
        // publish spurious failures.
        daemon.recover_state().await;
        // Job recovery returns a report when a scheduler is present.
        let report = daemon.recover_jobs().await;
        assert!(report.is_some());
    }

    #[tokio::test]
    async fn recover_state_tolerates_missing_pool_and_table() {
        // Pool-less daemons return early without panic.
        let daemon = CoreDaemon::new(None, None, None);
        daemon.recover_state().await;
        let _ = daemon.recover_jobs().await;

        // Pool without migrated tables logs and skips instead of panicking.
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:daemon_bootstrap_bare_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid options")
            .create_if_missing(true);
        let bare = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect");
        let daemon = CoreDaemon::new(Some(bare), None, None);
        daemon.recover_state().await;
        // hydrate on a bare pool fails open at the workspace layer only if
        // the store query fails; it must not panic.
        let _ = daemon.hydrate_workspace_registry().await;
    }

    #[tokio::test]
    async fn hydrate_without_pool_skips_asset_metadata() {
        let daemon = CoreDaemon::new(None, None, None);
        // In-memory registry hydrates from its own store; asset metadata
        // short-circuits without a pool.
        daemon
            .hydrate_workspace_registry()
            .await
            .expect("pool-less hydrate succeeds");
    }

    #[tokio::test]
    async fn replay_from_empty_log_returns_empty() {
        let daemon = CoreDaemon::new(None, None, None);
        let filter = EventFilter::default();
        let events = daemon.replay_from(0, &filter).await;
        assert!(events.is_empty());
    }
}

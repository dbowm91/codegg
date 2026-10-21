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

        if let Some(store) = self
            .pool
            .clone()
            .map(codegg_core::session_control::SessionControllerStore::new)
        {
            if let Err(error) = store
                .interrupt_incomplete_turn_receipts(chrono::Utc::now().timestamp_millis())
                .await
            {
                tracing::error!(error = %error, "failed to mark prior turn receipts interrupted");
            }
        }

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
                            // The daemon restart is the failure: it is not a
                            // provider failure and it is not a usage fact.
                            // No provider usage is observable here — the
                            // process that metered it is gone — so the
                            // usage stays absent rather than zero.
                            category: Some(
                                crate::protocol::core::TurnFailureCategory::DaemonRestarted,
                            ),
                            usage: None,
                        },
                    )
                    .await;

                // Clear runtime state for this session and release
                // the M004 controller lease (terminal wins: the failed
                // turn no longer has a controller).
                self.release_turn_controller(session_id, turn_id).await;
            }
        }

        // M004: reconcile the controller domain against terminal
        // state and trustworthy origin attribution (stale leases for
        // terminal turns are released; active turns without a lease
        // derive one only from trustworthy attribution, otherwise
        // control fails closed until explicit takeover).
        self.reconcile_session_controllers(&active_turns).await;

        // M003 (ADR-0038): the bounded restart twin. Reconciling stranded
        // `delivering` rows and delivering what is now claimable must run
        // **after** the controller reconciliation above, because delivery
        // requires the session to be free: the claim and the admission both
        // check for a live turn, and a session still holding a stale lease at
        // this point would release every entry it tried to deliver and defer it
        // to a trigger that never comes.
        //
        // It is also **after** `interrupt_incomplete_turn_receipts`, so a receipt
        // the daemon killed mid-turn is already marked `interrupted` when the
        // sweep asks whether a turn started. Reconciling first would read a
        // receipt that had not yet been told the truth about itself.
        self.reconcile_stranded_queue_deliveries().await;

        // M007 (ADR-0039): the bounded queue advance task, spawned once the
        // restart twin has finished so the two never race over the same row.
        //
        // It parks on `queue_schedule_changed` with no timer armed while nothing
        // is scheduled, so an idle daemon costs nothing — which is why this is a
        // spawn and not a `tokio::time::interval`. A `Weak` means a shutting-down
        // daemon simply never restarts it.
        if let Some(daemon) = self.idle_wake_cell.get().and_then(|weak| weak.upgrade()) {
            tokio::spawn(async move {
                daemon.run_queue_advance_task().await;
            });
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
    ///
    /// Also heals the idle-completion hint gap (best-effort): durable
    /// terminal notices committed before a crash but never forwarded
    /// get their persisted live hint republished here, deduped by
    /// notice id.
    pub async fn recover_jobs(&self) -> Option<crate::job_recovery::RecoveryReportSummary> {
        // C004 crash-gap healing first: republish live hints lost between
        // the durable notice commit and the forwarder publish. Failure
        // only warns; job recovery below proceeds regardless.
        let hints = self.reconcile_idle_completion_hints().await;
        if hints > 0 {
            tracing::info!(hints, "idle completion hints republished during recovery");
        }
        // C005 (ADR-0031) restart twin: re-evaluate delegated wakes for
        // recent terminals that may have missed their fanout wake (crash
        // between commit and fanout, forwarder abort). CAS-gated and
        // bounded; missed wakes otherwise heal the same way on the next
        // restart. Never fails daemon recovery.
        let wakes = self.reconcile_idle_continuation_wakes().await;
        if wakes > 0 {
            tracing::info!(wakes, "idle-continuation wakes dispatched during recovery");
        }
        let recovery = if let Some(scheduler) = self.deps.scheduler.as_ref() {
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
        };
        self.reconcile_deleted_session_scratch().await;
        recovery
    }

    /// Resume best-effort cleanup watchers for soft-deleted sessions after a
    /// daemon restart. The cleanup helper serializes with restore and rechecks
    /// durable run/job liveness before touching each owner tree.
    async fn reconcile_deleted_session_scratch(&self) {
        let Some(pool) = self.pool.as_ref() else {
            return;
        };
        let mut after_id = String::new();
        loop {
            let session_ids = match sqlx::query_scalar::<_, String>(
                "SELECT id FROM session WHERE time_deleted IS NOT NULL AND id > ? ORDER BY id LIMIT 256",
            )
            .bind(&after_id)
            .fetch_all(pool)
            .await
            {
                Ok(session_ids) => session_ids,
                Err(error) => {
                    tracing::warn!(?error, "deleted-session scratch recovery scan deferred");
                    return;
                }
            };
            if session_ids.is_empty() {
                return;
            }
            let page_is_full = session_ids.len() == 256;
            for session_id in &session_ids {
                crate::core::daemon_sessions::spawn_deleted_session_scratch_reclaimer(
                    session_id.clone(),
                    pool.clone(),
                    self.deps.agent_run_store.clone(),
                    self.deps.job_store.clone(),
                );
            }
            if !page_is_full {
                return;
            }
            after_id = session_ids.last().cloned().unwrap_or_default();
        }
    }

    /// C004 crash-gap healing (ADR-0031 §Decision.5): republish persisted
    /// `BackgroundJobCompleted` hints for durable terminal notices that
    /// have no hint in `core_event_log`.
    ///
    /// Covers the crash between the notice commit and the forwarder
    /// publish, forwarder abort, and lagged/closed broadcast receivers.
    /// Bounded (512 newest durable notices via
    /// `durable_notices_for_hint_recovery`), deduped by notice id: never
    /// a second notice row, never a turn. Best-effort: failures only warn
    /// and never fail daemon recovery. Returns the republished count.
    pub async fn reconcile_idle_completion_hints(&self) -> usize {
        let Some(notifier) = self.deps.job_notification_service.clone() else {
            return 0;
        };
        let notices = match notifier.durable_notices_for_hint_recovery().await {
            Ok(notices) => notices,
            Err(error) => {
                tracing::warn!(?error, "idle hint reconciliation skipped");
                return 0;
            }
        };
        let mut republished = 0;
        for notice in &notices {
            if self
                .event_log
                .has_persisted_background_completion(&notice.notice_id)
                .await
            {
                continue;
            }
            Self::publish_idle_completion_hint(&self.event_log, &self.notification_router, notice)
                .await;
            republished += 1;
        }
        if republished > 0 {
            tracing::info!(
                republished,
                total_notices = notices.len(),
                "republished missing idle completion hints"
            );
        }
        republished
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
                                // Resolve the session name for the announcement. The
                                // rendered message is spoken aloud, so it must not
                                // carry a raw uuid; the fallback is a short prefix.
                                let session_title = super::notification::resolve_session_title(
                                    daemon.pool.as_ref(),
                                    session_id.as_str(),
                                )
                                .await;
                                let spoken_name = super::notification::spoken_session_name(
                                    session_title.as_deref(),
                                    Some(session_id.as_str()),
                                );
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind,
                                    priority,
                                    message: format!("Turn {stop_reason} for {spoken_name}"),
                                    dedupe_key: Some(format!("turn-done:{}", session_id)),
                                    created_at: Utc::now(),
                                    session_title,
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
                                    session_title: None,
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
                                    session_title: None,
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
                                    session_title: None,
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
                                session_id,
                                child_instance_id,
                                originating_turn_id,
                                task_id,
                                agent,
                                description,
                                display_name,
                                profile,
                                usefulness,
                                run_id,
                                ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    let node = codegg_protocol::projection::dto::AgentTreeNodeProjection {
                                        child_instance_id: child_instance_id.clone(),
                                        originating_turn_id: originating_turn_id.clone(),
                                        task_id: *task_id,
                                        agent: agent.clone(),
                                        description: description.clone(),
                                        status: codegg_protocol::projection::dto::AgentTreeStatus::Running,
                                        parent_task_id: None,
                                        created_at: Utc::now().timestamp_millis(),
                                        completed_at: None,
                                        result_summary: None,
                                        display_name: display_name.clone(),
                                        profile: profile.clone(),
                                        usefulness: usefulness.clone(),
                                        run_id: run_id.clone(),
                                    };
                                    let mut children = runtime.subagents.write().await;
                                    if !children.contains_key(child_instance_id) {
                                        if children.len()
                                            < codegg_protocol::projection::MAX_PROJECTION_SESSION_SUBAGENTS
                                        {
                                            children.insert(child_instance_id.clone(), node);
                                        } else if let Some(terminal_key) = children
                                            .iter()
                                            .filter(|(_, existing)| {
                                                !matches!(
                                                    existing.status,
                                                    codegg_protocol::projection::dto::AgentTreeStatus::Running
                                                )
                                            })
                                            .min_by_key(|(_, existing)| {
                                                existing.completed_at.unwrap_or(existing.created_at)
                                            })
                                            .map(|(key, _)| key.clone())
                                        {
                                            children.remove(&terminal_key);
                                            children.insert(child_instance_id.clone(), node);
                                        } else {
                                            runtime.subagent_overflow.fetch_add(
                                                1,
                                                std::sync::atomic::Ordering::Relaxed,
                                            );
                                        }
                                    }
                                    runtime.active_subagent_count.store(
                                        children.values().filter(|existing| matches!(
                                            existing.status,
                                            codegg_protocol::projection::dto::AgentTreeStatus::Running
                                        )).count(),
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                }
                            }
                            crate::bus::events::AppEvent::SubagentProgress {
                                session_id,
                                child_instance_id,
                                message,
                                ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    if let Some(node) =
                                        runtime.subagents.write().await.get_mut(child_instance_id)
                                    {
                                        if matches!(
                                            node.status,
                                            codegg_protocol::projection::dto::AgentTreeStatus::Running
                                        ) {
                                            node.description = message.clone();
                                        }
                                    }
                                }
                            }
                            crate::bus::events::AppEvent::SubagentCompleted {
                                session_id,
                                child_instance_id,
                                task_id,
                                agent,
                                result_summary,
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    let mut children = runtime.subagents.write().await;
                                    if let Some(node) = children.get_mut(child_instance_id) {
                                        if matches!(node.status, codegg_protocol::projection::dto::AgentTreeStatus::Running) {
                                            node.status = codegg_protocol::projection::dto::AgentTreeStatus::Completed;
                                            node.result_summary = Some(result_summary.clone());
                                            node.completed_at = Some(Utc::now().timestamp_millis());
                                            runtime.active_subagent_count.store(
                                                children.values().filter(|existing| matches!(
                                                    existing.status,
                                                    codegg_protocol::projection::dto::AgentTreeStatus::Running
                                                )).count(),
                                                std::sync::atomic::Ordering::Relaxed,
                                            );
                                        }
                                    }
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
                                    session_title: None,
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
                                child_instance_id,
                                task_id,
                                agent,
                                error,
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                    let mut children = runtime.subagents.write().await;
                                    if let Some(node) = children.get_mut(child_instance_id) {
                                        if matches!(node.status, codegg_protocol::projection::dto::AgentTreeStatus::Running) {
                                            node.status = codegg_protocol::projection::dto::AgentTreeStatus::Failed;
                                            node.result_summary = Some(error.clone());
                                            node.completed_at = Some(Utc::now().timestamp_millis());
                                            runtime.active_subagent_count.store(
                                                children.values().filter(|existing| matches!(
                                                    existing.status,
                                                    codegg_protocol::projection::dto::AgentTreeStatus::Running
                                                )).count(),
                                                std::sync::atomic::Ordering::Relaxed,
                                            );
                                        }
                                    }
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
                                    session_title: None,
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
    async fn startup_recovery_marks_receipts_interrupted_without_replay() {
        let pool = in_memory_pool().await;
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) \
             VALUES ('project-recovery', '/tmp/project-recovery', '[]', ?, ?)",
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("project row");
        sqlx::query(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) \
             VALUES ('session-recovery', 'project-recovery', 'recovery', '/tmp/project-recovery', 'Recovery', '1', ?, ?)",
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("session row");
        let principal =
            codegg_core::identity::PrincipalId::parse("principal-recovery").expect("principal");
        let store = codegg_core::session_control::SessionControllerStore::new(pool.clone());
        store
            .admit_with_promotions(
                "session-recovery",
                "submission-recovery",
                &"a".repeat(64),
                "turn-recovery",
                &principal,
                Some("client-recovery"),
                "project-recovery",
                "workspace-recovery",
                &[],
                now,
            )
            .await
            .expect("durable accepted receipt");

        let daemon = CoreDaemon::new(Some(pool), None, None);
        daemon.recover_state().await;
        let receipt = store
            .get_turn_receipt("submission-recovery")
            .await
            .expect("receipt lookup")
            .expect("receipt exists");
        assert_eq!(receipt.status, "interrupted");
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

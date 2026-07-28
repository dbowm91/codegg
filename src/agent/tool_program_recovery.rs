//! Tool Program notification recovery loop.
//!
//! Extracted from `AgentLoop::inject_pending_notifications` so the
//! boundary can be exercised by recovery tests and a daemon-mode
//! recovery fixture request. The loop drives notifications from
//! Pending / Claimed to Delivered, with crash-safe recovery for two
//! interleaving points:
//!
//! - Crash after the parent-session event is appended but before
//!   `mark_injected` is called. Process B reconstructs the event with
//!   a fresh `created_at`; semantic equality in `EventStore` accepts
//!   the append and the loop observes an existing event, marks the
//!   notification injected, and acknowledges.
//! - Crash after `mark_injected` but before `acknowledge`. The loop
//!   observes `injected_event_id` is set and just acknowledges.

use codegg_core::session::EventStore;
use codegg_protocol::projection::dto::NotificationClassification;

use crate::scheduler::tool_program_notifications::{
    NotificationState, ToolProgramNotificationService,
};

/// Per-call report from [`inject_recoverable_notifications`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InjectNotificationsReport {
    pub considered: usize,
    pub injected: usize,
    pub recovered_via_event: usize,
    pub already_injected: usize,
    pub leased: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

/// Reconcile Pending and Claimed notifications for `session_id`,
/// driving each to Delivered by appending the parent-session event,
/// marking injected, and acknowledging.
///
/// `on_message` is invoked for every freshly-injected notification
/// text. Existing-tenant messages (mark-already-set) do not call
/// `on_message`; the persisted event remains the source of truth.
pub async fn inject_recoverable_notifications<F>(
    event_store: Option<&EventStore>,
    notification_service: &ToolProgramNotificationService,
    session_id: &str,
    mut on_message: F,
) -> InjectNotificationsReport
where
    F: FnMut(String),
{
    let mut report = InjectNotificationsReport::default();

    let recoverable = match notification_service
        .recoverable_for_session(session_id)
        .await
    {
        Ok(recoverable) => recoverable,
        Err(error) => {
            report
                .errors
                .push(format!("load durable notifications: {error}"));
            return report;
        }
    };
    if recoverable.is_empty() {
        return report;
    }
    report.considered = recoverable.len();

    for notification in &recoverable {
        let Some(injection_key) = notification.injection_key.as_deref() else {
            report.errors.push(format!(
                "notification {} missing injection key",
                notification.notification_id
            ));
            report.skipped += 1;
            continue;
        };
        let event_id = format!("tp-event:{injection_key}");

        if notification_service
            .is_injected(&notification.notification_id)
            .await
        {
            match notification_service
                .acknowledge(&notification.notification_id)
                .await
            {
                Ok(true) => report.already_injected += 1,
                Ok(false) => {
                    report.errors.push(format!(
                        "acknowledge after existing inject failed: {}",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                }
                Err(error) => {
                    report.errors.push(format!(
                        "acknowledge after existing inject error: {}: {error}",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                }
            }
            continue;
        }

        let Some(event_store) = event_store else {
            report
                .errors
                .push("durable session store unavailable".to_string());
            report.skipped += 1;
            continue;
        };

        // Recovery: a matching event exists from an earlier append
        // (crash after append before mark_injected).
        if event_store.has_event(&event_id).await.unwrap_or(false) {
            if let Err(error) = notification_service
                .mark_injected(&notification.notification_id, &event_id)
                .await
            {
                report.errors.push(format!(
                    "mark injected after existing event failed: {}: {error}",
                    notification.notification_id
                ));
                report.skipped += 1;
                continue;
            }
            match notification_service
                .acknowledge(&notification.notification_id)
                .await
            {
                Ok(true) => report.recovered_via_event += 1,
                Ok(false) => {
                    report.errors.push(format!(
                        "acknowledge after existing event failed: {}",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                }
                Err(error) => {
                    report.errors.push(format!(
                        "acknowledge after existing event error: {}: {error}",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                }
            }
            continue;
        }

        if !matches!(notification.state, NotificationState::Pending) {
            // Claimed without an event in the parent session and
            // within the lease window: leave for lease expiry.
            report.leased += 1;
            continue;
        }

        match notification_service
            .claim(&notification.notification_id)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                report.skipped += 1;
                continue;
            }
            Err(error) => {
                report.errors.push(format!(
                    "claim failed for {}: {error}",
                    notification.notification_id
                ));
                report.skipped += 1;
                continue;
            }
        }

        let text = match notification.classification {
            NotificationClassification::Completed => format!(
                "Background program {} completed successfully: {}",
                notification.program_id, notification.summary
            ),
            NotificationClassification::IncompleteRecoverable => format!(
                "Background program {} is incomplete but recoverable ({}): {}",
                notification.program_id,
                notification.failure_class.as_deref().unwrap_or("unknown"),
                notification.summary
            ),
            NotificationClassification::FailedTerminal => format!(
                "Background program {} failed terminally ({}): {}",
                notification.program_id,
                notification.failure_class.as_deref().unwrap_or("unknown"),
                notification.summary
            ),
        };

        let event = crate::session::events::SessionEvent::ToolProgramNotification(
            crate::session::events::ToolProgramNotificationEvent {
                meta: crate::session::events::EventMeta {
                    id: event_id.clone(),
                    session_id: session_id.to_string(),
                    created_at: chrono::Utc::now(),
                },
                injection_key: injection_key.to_string(),
                notification_id: notification.notification_id.clone(),
                program_id: notification.program_id.clone(),
                content: text.clone(),
            },
        );

        if let Err(error) = event_store.append_idempotent(&event).await {
            report.errors.push(format!(
                "append event failed for {}: {error}",
                notification.notification_id
            ));
            report.skipped += 1;
            continue;
        }
        crate::test_failpoint::hit("tool_program_after_session_append");
        if let Err(error) = notification_service
            .mark_injected(&notification.notification_id, &event_id)
            .await
        {
            report.errors.push(format!(
                "mark injected failed for {}: {error}",
                notification.notification_id
            ));
            report.skipped += 1;
            continue;
        }
        on_message(text);
        match notification_service
            .acknowledge(&notification.notification_id)
            .await
        {
            Ok(true) => report.injected += 1,
            Ok(false) => {
                report.errors.push(format!(
                    "acknowledge failed for {}",
                    notification.notification_id
                ));
                report.skipped += 1;
            }
            Err(error) => {
                report.errors.push(format!(
                    "acknowledge error for {}: {error}",
                    notification.notification_id
                ));
                report.skipped += 1;
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::tool_program_notifications::{
        NotificationState, ProgramHandle, ToolProgramNotification,
    };
    use codegg_core::session::schema::migrate;

    fn notification(
        id: &str,
        state: NotificationState,
        injection_key: Option<&str>,
    ) -> ToolProgramNotification {
        let now = chrono::Utc::now().timestamp_millis();
        ToolProgramNotification {
            notification_id: id.into(),
            program_id: id.into(),
            job_id: format!("job-{id}"),
            session_id: "session-test".into(),
            agent_id: None,
            turn_id: None,
            status: "completed".into(),
            summary: format!("summary-{id}"),
            failure_class: None,
            success: true,
            classification: codegg_protocol::projection::dto::NotificationClassification::Completed,
            payload_digest: format!("sha256:{id}"),
            program_handle: ProgramHandle {
                program_id: id.into(),
                job_id: format!("job-{id}"),
                status: "terminal".into(),
                submitted_at: now,
                timeout_ms: 1_000,
                inspect_ref: id.into(),
                cancel_ref: format!("job-{id}"),
            },
            state,
            created_at: now,
            updated_at: now,
            claim_owner: None,
            claim_lease_until: None,
            delivered_at: None,
            retry_count: 0,
            injection_key: injection_key.map(str::to_string),
            injected_event_id: None,
        }
    }

    async fn fresh_pool() -> sqlx::SqlitePool {
        use std::str::FromStr;
        let name = format!("codegg_recovery_{}", uuid::Uuid::new_v4().simple());
        let url = format!("file:{name}?mode=memory&cache=shared");
        let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite connect options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect to in-memory sqlite");
        migrate(&pool).await.expect("run migrations");
        seed_test_session(&pool, "session-test").await;
        pool
    }

    async fn seed_test_session(pool: &sqlx::SqlitePool, session_id: &str) {
        sqlx::query("INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, '{}', '[]', 0, 0)")
            .bind("project-test")
            .execute(pool)
            .await
            .expect("seed project");
        sqlx::query(
            "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, ?, ?, '1', 0, 0)",
        )
        .bind(session_id)
        .bind("project-test")
        .bind("test")
        .bind("/tmp/test")
        .bind("Test")
        .execute(pool)
        .await
        .expect("seed session");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pending_notification_drives_to_delivered() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-a",
                NotificationState::Pending,
                Some("inj-a"),
            ))
            .await
            .unwrap();

        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let messages_clone = messages.clone();
        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", move |text| {
                messages_clone.lock().unwrap().push(text)
            })
            .await;
        assert_eq!(report.injected, 1);
        assert_eq!(report.considered, 1);
        assert_eq!(report.recovered_via_event, 0);
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
        assert_eq!(messages.lock().unwrap().len(), 1);

        let stored = service.get("tp-a").await.unwrap().unwrap();
        assert_eq!(stored.state, NotificationState::Delivered);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claimed_with_existing_event_is_recovered_without_reappending() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        // Simulate process A: record + claim + append event + crash before
        // mark_injected. The notification stays in Claimed state with no
        // injected_event_id.
        service
            .record_notification(notification(
                "tp-b",
                NotificationState::Pending,
                Some("inj-b"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-b").await.unwrap());

        // The event is appended by process A with a fresh created_at.
        let now_a = chrono::Utc::now().timestamp_millis();
        let event_a = crate::session::events::SessionEvent::ToolProgramNotification(
            crate::session::events::ToolProgramNotificationEvent {
                meta: crate::session::events::EventMeta {
                    id: "tp-event:inj-b".into(),
                    session_id: "session-test".into(),
                    created_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_a)
                        .unwrap(),
                },
                injection_key: "inj-b".into(),
                notification_id: "tp-b".into(),
                program_id: "tp-b".into(),
                content: "Background program tp-b completed successfully: summary-tp-b".into(),
            },
        );
        store.append_idempotent(&event_a).await.unwrap();

        // Process B: a fresh service against the same pool. Recovery
        // detects the existing event and marks the notification
        // injected without re-appending.
        let service_b = ToolProgramNotificationService::with_pool(store.pool().clone());
        let recovered_state = service_b.get("tp-b").await.unwrap().unwrap().state;
        assert_eq!(recovered_state, NotificationState::Claimed);

        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let messages_clone = messages.clone();
        let report = inject_recoverable_notifications(
            Some(&store),
            &service_b,
            "session-test",
            move |text| messages_clone.lock().unwrap().push(text),
        )
        .await;
        assert_eq!(report.recovered_via_event, 1);
        assert_eq!(report.injected, 0);
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
        assert_eq!(
            messages.lock().unwrap().len(),
            0,
            "no new message: persisted event is the source of truth"
        );

        let stored = service_b.get("tp-b").await.unwrap().unwrap();
        assert_eq!(stored.state, NotificationState::Delivered);
        assert_eq!(stored.injected_event_id.as_deref(), Some("tp-event:inj-b"));

        // No duplicate event was appended.
        let events = store.list_for_session("session-test").await.unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claimed_within_lease_without_event_is_left_for_expiry() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-c",
                NotificationState::Pending,
                Some("inj-c"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-c").await.unwrap());

        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let messages_clone = messages.clone();
        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", move |text| {
                messages_clone.lock().unwrap().push(text)
            })
            .await;
        assert_eq!(report.leased, 1);
        assert_eq!(report.injected, 0);
        assert_eq!(report.recovered_via_event, 0);
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
        let _ = (&messages); // suppress unused-warnings under clippy
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mark_injected_only_acknowledges_without_emitting_message() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-d",
                NotificationState::Pending,
                Some("inj-d"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-d").await.unwrap());
        service
            .mark_injected("tp-d", "tp-event:inj-d")
            .await
            .unwrap();

        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let messages_clone = messages.clone();
        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", move |text| {
                messages_clone.lock().unwrap().push(text)
            })
            .await;
        assert_eq!(report.already_injected, 1);
        assert_eq!(report.injected, 0);
        assert_eq!(report.recovered_via_event, 0);
        assert_eq!(
            messages.lock().unwrap().len(),
            0,
            "injected_event_id set: no new message"
        );
        let stored = service.get("tp-d").await.unwrap().unwrap();
        assert_eq!(stored.state, NotificationState::Delivered);
    }
}

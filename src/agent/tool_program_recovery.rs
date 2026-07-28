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
//!
//! M017: recovery no longer relies on `has_event` for semantic
//! confirmation. An existing event is loaded and semantically
//! confirmed before any state transition. Query failures are
//! propagated as typed errors rather than converted to absence.

use codegg_core::session::events::{EventMeta, ToolProgramNotificationEvent};
use codegg_core::session::EventStore;
use codegg_protocol::projection::dto::NotificationClassification;

use crate::scheduler::tool_program_notifications::{
    NotificationState, ToolProgramNotification, ToolProgramNotificationService,
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
    /// M017: notification that was confirmed and then injected.
    pub inject_confirmed: usize,
    /// M017: durable event exists but semantic confirmation failed.
    pub semantic_collisions: usize,
    /// M017: event-store query or deserialization errors.
    pub store_errors: usize,
    pub errors: Vec<String>,
}

/// Reconstruct the expected `ToolProgramNotificationEvent` from a
/// durable notification record. All recovery branches must use this
/// helper to produce the expected event — no branch may reconstruct
/// different content for the same durable notification.
///
/// The event content matches the formatter used for first delivery so
/// that `semantic_equals` in `EventStore` can confirm the durable
/// event.
pub fn expected_notification_event(
    notification: &ToolProgramNotification,
    session_id: &str,
) -> Result<codegg_core::session::SessionEvent, NotificationRecoveryError> {
    let injection_key = notification.injection_key.as_deref().ok_or_else(|| {
        NotificationRecoveryError::MissingInjectionKey {
            notification_id: notification.notification_id.clone(),
        }
    })?;

    let event_id = format!("tp-event:{injection_key}");
    let content = format_notification_content(notification);

    Ok(codegg_core::session::SessionEvent::ToolProgramNotification(
        ToolProgramNotificationEvent {
            meta: EventMeta {
                id: event_id,
                session_id: session_id.to_string(),
                created_at: chrono::Utc::now(),
            },
            injection_key: injection_key.to_string(),
            notification_id: notification.notification_id.clone(),
            program_id: notification.program_id.clone(),
            content,
        },
    ))
}

/// Format notification content consistently for both first delivery
/// and restart recovery. This formatter is the single source of
/// truth for the `content` field in `ToolProgramNotificationEvent`.
pub fn format_notification_content(notification: &ToolProgramNotification) -> String {
    match notification.classification {
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
    }
}

/// Error type for notification recovery operations.
#[derive(Debug, thiserror::Error)]
pub enum NotificationRecoveryError {
    #[error("notification {notification_id} missing injection key")]
    MissingInjectionKey { notification_id: String },

    #[error("notification {notification_id} has mismatched injected_event_id: expected {expected}, got {actual}")]
    MismatchedInjectedEventId {
        notification_id: String,
        expected: String,
        actual: String,
    },

    #[error("event-store query failed for {event_id}: {source}")]
    EventStoreQuery {
        event_id: String,
        #[source]
        source: codegg_providers::error::StorageError,
    },

    #[error("event-store semantic collision for {event_id}")]
    SemanticCollision { event_id: String },

    #[error("event-store identity collision for {event_id}")]
    IdentityCollision { event_id: String },

    #[error("malformed stored payload for {event_id}: {message}")]
    MalformedPayload { event_id: String, message: String },

    #[error("CAS conflict on {notification_id}: {detail}")]
    CasConflict {
        notification_id: String,
        detail: String,
    },
}

/// Reconcile Pending and Claimed notifications for `session_id`,
/// driving each to Delivered by appending the parent-session event,
/// marking injected, and acknowledging.
///
/// `on_message` is invoked for every freshly-injected notification
/// text. Existing-tenant messages (mark-already-set) do not call
/// `on_message`; the persisted event remains the source of truth.
///
/// M017: recovery now follows a strict state machine per notification:
///
/// - **Already injected**: verify the stored `injected_event_id`
///   matches the stable expected ID, then confirm the durable event
///   semantically before acknowledging.
/// - **Claimed**: use `confirm_existing` — `SemanticMatch` marks and
///   acknowledges; `Absent` leaves the notification for lease expiry;
///   errors/collisions are reported without state transitions.
/// - **Pending**: claim, reconstruct expected event, append
///   idempotently, mark injected, acknowledge.
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
        let expected_event_id = format!("tp-event:{injection_key}");

        // ── Already-injected branch ──────────────────────────────
        // M017-C16: verify the stored injected_event_id matches the
        // stable expected ID, then confirm the durable event before
        // acknowledging.
        if notification_service
            .is_injected(&notification.notification_id)
            .await
        {
            // Load the full notification to verify injected_event_id.
            match notification_service
                .get(&notification.notification_id)
                .await
            {
                Ok(Some(full)) => {
                    if let Some(ref stored_id) = full.injected_event_id {
                        if stored_id != &expected_event_id {
                            report.errors.push(format!(
                                "mismatched injected_event_id for {}: expected {}, got {}",
                                notification.notification_id, expected_event_id, stored_id
                            ));
                            report.skipped += 1;
                            continue;
                        }
                    } else {
                        // is_injected was true but get shows no
                        // injected_event_id — race or cache
                        // inconsistency. Fall through to the
                        // event-store path.
                    }
                }
                Ok(None) => {
                    report.errors.push(format!(
                        "already-injected notification {} not found in store",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                    continue;
                }
                Err(error) => {
                    report.errors.push(format!(
                        "load notification for inject verification {}: {error}",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                    continue;
                }
            }

            // Confirm the durable event semantically.
            let Some(event_store) = event_store else {
                report
                    .errors
                    .push("durable session store unavailable".to_string());
                report.skipped += 1;
                continue;
            };
            let expected = match expected_notification_event(notification, session_id) {
                Ok(e) => e,
                Err(e) => {
                    report.errors.push(format!(
                        "reconstruct expected event for {}: {e}",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                    continue;
                }
            };
            match event_store.confirm_existing(&expected).await {
                Ok(codegg_core::session::ConfirmExistingEvent::SemanticMatch) => {
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
                }
                Ok(codegg_core::session::ConfirmExistingEvent::Absent) => {
                    report.errors.push(format!(
                        "already-injected notification {} has no durable event",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                }
                Err(e) => {
                    report.errors.push(format!(
                        "event confirmation failed for already-injected {}: {e}",
                        notification.notification_id
                    ));
                    report.store_errors += 1;
                }
            }
            continue;
        }

        // ── No event store available ─────────────────────────────
        let Some(event_store) = event_store else {
            report
                .errors
                .push("durable session store unavailable".to_string());
            report.skipped += 1;
            continue;
        };

        // ── Claimed branch (not injected) ────────────────────────
        // M017-C19: Claimed notification with no event remains
        // claimed for lease expiry; do not insert.
        // M017-C20/C21: semantic collisions and query errors are
        // reported without state transitions.
        if matches!(notification.state, NotificationState::Claimed) {
            let expected = match expected_notification_event(notification, session_id) {
                Ok(e) => e,
                Err(e) => {
                    report.errors.push(format!(
                        "reconstruct expected event for {}: {e}",
                        notification.notification_id
                    ));
                    report.skipped += 1;
                    continue;
                }
            };
            match event_store.confirm_existing(&expected).await {
                Ok(codegg_core::session::ConfirmExistingEvent::SemanticMatch) => {
                    // M017-C18: matching event → mark injected,
                    // then acknowledge.
                    if let Err(error) = notification_service
                        .mark_injected(&notification.notification_id, &expected_event_id)
                        .await
                    {
                        report.errors.push(format!(
                            "mark injected after confirm failed: {}: {error}",
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
                                "acknowledge after confirm failed: {}",
                                notification.notification_id
                            ));
                            report.skipped += 1;
                        }
                        Err(error) => {
                            report.errors.push(format!(
                                "acknowledge after confirm error: {}: {error}",
                                notification.notification_id
                            ));
                            report.skipped += 1;
                        }
                    }
                }
                Ok(codegg_core::session::ConfirmExistingEvent::Absent) => {
                    // M017-C19: no event in parent session and
                    // within lease window → leave for expiry.
                    report.leased += 1;
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("semantic collision") || msg.contains("identity collision") {
                        report.semantic_collisions += 1;
                        report.errors.push(format!(
                            "semantic collision for claimed notification {}: {e}",
                            notification.notification_id
                        ));
                    } else {
                        report.store_errors += 1;
                        report.errors.push(format!(
                            "event-store error for claimed notification {}: {e}",
                            notification.notification_id
                        ));
                    }
                }
            }
            continue;
        }

        // ── Pending branch ───────────────────────────────────────
        // M017-C22: claim before event insertion.
        // M017-C23: mark only after append succeeds.
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

        let expected = match expected_notification_event(notification, session_id) {
            Ok(e) => e,
            Err(e) => {
                report.errors.push(format!(
                    "reconstruct expected event for {}: {e}",
                    notification.notification_id
                ));
                report.skipped += 1;
                continue;
            }
        };

        if let Err(error) = event_store.append_idempotent(&expected).await {
            report.errors.push(format!(
                "append event failed for {}: {error}",
                notification.notification_id
            ));
            report.skipped += 1;
            continue;
        }
        crate::test_failpoint::hit("tool_program_after_session_append");
        if let Err(error) = notification_service
            .mark_injected(&notification.notification_id, &expected_event_id)
            .await
        {
            report.errors.push(format!(
                "mark injected failed for {}: {error}",
                notification.notification_id
            ));
            report.skipped += 1;
            continue;
        }
        let text = format_notification_content(notification);
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

    fn notification_with_classification(
        id: &str,
        state: NotificationState,
        injection_key: Option<&str>,
        classification: NotificationClassification,
        failure_class: Option<&str>,
        success: bool,
    ) -> ToolProgramNotification {
        let now = chrono::Utc::now().timestamp_millis();
        ToolProgramNotification {
            notification_id: id.into(),
            program_id: id.into(),
            job_id: format!("job-{id}"),
            session_id: "session-test".into(),
            agent_id: None,
            turn_id: None,
            status: if success { "completed" } else { "failed" }.into(),
            summary: format!("summary-{id}"),
            failure_class: failure_class.map(str::to_string),
            success,
            classification,
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

        // M017: insert the durable event so confirm_existing passes.
        let event = expected_notification_event(
            &service.get("tp-d").await.unwrap().unwrap(),
            "session-test",
        )
        .unwrap();
        store.append_idempotent(&event).await.unwrap();

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

    // ── M017 Work Package A: semantic-confirmation tests ──────────

    /// M017-A2: Claimed notification with same ID but different
    /// content must not advance to Delivered.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claimed_with_same_id_different_content_is_not_marked() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-e",
                NotificationState::Pending,
                Some("inj-e"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-e").await.unwrap());

        // Insert an event with the same ID but different content.
        let event = crate::session::events::SessionEvent::ToolProgramNotification(
            crate::session::events::ToolProgramNotificationEvent {
                meta: crate::session::events::EventMeta {
                    id: "tp-event:inj-e".into(),
                    session_id: "session-test".into(),
                    created_at: chrono::Utc::now(),
                },
                injection_key: "inj-e".into(),
                notification_id: "tp-e".into(),
                program_id: "tp-e".into(),
                content: "DIFFERENT CONTENT".into(),
            },
        );
        store.append_idempotent(&event).await.unwrap();

        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", |_| {}).await;
        assert_eq!(
            report.semantic_collisions, 1,
            "semantic collision must be counted"
        );
        assert_eq!(report.recovered_via_event, 0);
        assert_eq!(report.injected, 0);
        assert!(!report.errors.is_empty());

        let stored = service.get("tp-e").await.unwrap().unwrap();
        assert_eq!(
            stored.state,
            NotificationState::Claimed,
            "must remain Claimed"
        );
        assert!(
            stored.injected_event_id.is_none(),
            "must not be marked injected"
        );
    }

    /// M017-A3: Claimed notification with same ID but different
    /// notification_id must fail closed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claimed_with_different_notification_id_fails_closed() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-f",
                NotificationState::Pending,
                Some("inj-f"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-f").await.unwrap());

        // Insert event with same ID but different notification_id.
        let event = crate::session::events::SessionEvent::ToolProgramNotification(
            crate::session::events::ToolProgramNotificationEvent {
                meta: crate::session::events::EventMeta {
                    id: "tp-event:inj-f".into(),
                    session_id: "session-test".into(),
                    created_at: chrono::Utc::now(),
                },
                injection_key: "inj-f".into(),
                notification_id: "DIFFERENT-NOTIFICATION".into(),
                program_id: "tp-f".into(),
                content: "Background program tp-f completed successfully: summary-tp-f".into(),
            },
        );
        store.append_idempotent(&event).await.unwrap();

        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", |_| {}).await;
        assert_eq!(report.semantic_collisions, 1);
        assert_eq!(report.recovered_via_event, 0);

        let stored = service.get("tp-f").await.unwrap().unwrap();
        assert_eq!(stored.state, NotificationState::Claimed);
        assert!(stored.injected_event_id.is_none());
    }

    /// M017-A4: Claimed notification with same ID but wrong event
    /// variant must fail closed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claimed_with_wrong_event_variant_fails_closed() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-g",
                NotificationState::Pending,
                Some("inj-g"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-g").await.unwrap());

        // Insert an AgentMessage with the same event ID.
        let event = crate::session::events::SessionEvent::AgentMessage(
            crate::session::events::AgentMessageEvent {
                meta: crate::session::events::EventMeta {
                    id: "tp-event:inj-g".into(),
                    session_id: "session-test".into(),
                    created_at: chrono::Utc::now(),
                },
                content: "not a notification".into(),
            },
        );
        store.append_idempotent(&event).await.unwrap();

        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", |_| {}).await;
        // Event-variant mismatch results in identity collision
        // error from confirm_existing.
        assert!(report.semantic_collisions + report.store_errors > 0);
        assert_eq!(report.recovered_via_event, 0);

        let stored = service.get("tp-g").await.unwrap().unwrap();
        assert_eq!(stored.state, NotificationState::Claimed);
    }

    /// M017-A5: Claimed notification with malformed stored payload
    /// must fail closed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claimed_with_malformed_stored_payload_fails_closed() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool.clone());

        service
            .record_notification(notification(
                "tp-h",
                NotificationState::Pending,
                Some("inj-h"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-h").await.unwrap());

        // Insert a malformed payload directly.
        sqlx::query(
            "INSERT INTO session_events (id, session_id, created_at, event_type, payload_json) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("tp-event:inj-h")
        .bind("session-test")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind("tool_program_notification")
        .bind("{not valid json")
        .execute(&pool)
        .await
        .unwrap();

        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", |_| {}).await;
        assert!(
            report.store_errors > 0 || report.semantic_collisions > 0,
            "malformed payload must not advance state: {:?}",
            report
        );
        assert_eq!(report.recovered_via_event, 0);

        let stored = service.get("tp-h").await.unwrap().unwrap();
        assert_eq!(stored.state, NotificationState::Claimed);
    }

    /// M017-A7: Already-injected notification with a missing durable
    /// event must not acknowledge.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn already_injected_with_missing_event_does_not_acknowledge() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-i",
                NotificationState::Pending,
                Some("inj-i"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-i").await.unwrap());
        // Mark injected but do NOT insert the event.
        service
            .mark_injected("tp-i", "tp-event:inj-i")
            .await
            .unwrap();

        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", |_| {}).await;
        assert_eq!(report.already_injected, 0, "must not acknowledge");
        assert!(!report.errors.is_empty(), "must report missing event");

        let stored = service.get("tp-i").await.unwrap().unwrap();
        assert_eq!(
            stored.state,
            NotificationState::Claimed,
            "must remain Claimed"
        );
    }

    /// M017-A8: Already-injected notification with mismatched
    /// injected_event_id must not acknowledge.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn already_injected_with_mismatched_event_id_does_not_acknowledge() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-j",
                NotificationState::Pending,
                Some("inj-j"),
            ))
            .await
            .unwrap();
        assert!(service.claim("tp-j").await.unwrap());
        // Mark injected with a WRONG event ID.
        service
            .mark_injected("tp-j", "tp-event:WRONG")
            .await
            .unwrap();

        // Insert the correct event so confirm_existing would pass if
        // reached, but the mismatched ID blocks the path.
        let event = expected_notification_event(
            &service.get("tp-j").await.unwrap().unwrap(),
            "session-test",
        )
        .unwrap();
        store.append_idempotent(&event).await.unwrap();

        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", |_| {}).await;
        assert_eq!(report.already_injected, 0, "must not acknowledge");
        assert!(!report.errors.is_empty());

        let stored = service.get("tp-j").await.unwrap().unwrap();
        assert_eq!(stored.state, NotificationState::Claimed);
    }

    /// M017-A9: Pending notification append collision must not mark
    /// or acknowledge.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pending_append_collision_does_not_mark_or_acknowledge() {
        let pool = fresh_pool().await;
        let store = EventStore::new(pool.clone());
        let service = ToolProgramNotificationService::with_pool(pool);

        service
            .record_notification(notification(
                "tp-k",
                NotificationState::Pending,
                Some("inj-k"),
            ))
            .await
            .unwrap();

        // Pre-insert an event with the same ID but different content
        // to force an append collision.
        let collision_event = crate::session::events::SessionEvent::ToolProgramNotification(
            crate::session::events::ToolProgramNotificationEvent {
                meta: crate::session::events::EventMeta {
                    id: "tp-event:inj-k".into(),
                    session_id: "session-test".into(),
                    created_at: chrono::Utc::now(),
                },
                injection_key: "inj-k".into(),
                notification_id: "tp-k".into(),
                program_id: "tp-k".into(),
                content: "COLLISION".into(),
            },
        );
        store.append_idempotent(&collision_event).await.unwrap();

        let report =
            inject_recoverable_notifications(Some(&store), &service, "session-test", |_| {}).await;
        assert_eq!(report.injected, 0, "must not inject on collision");
        assert!(!report.errors.is_empty(), "must report append error");

        let stored = service.get("tp-k").await.unwrap().unwrap();
        // The claim succeeded but the append collision prevented
        // mark_injected. The notification may be Claimed (if claim
        // succeeded) or still Pending (if claim also failed).
        assert!(
            matches!(
                stored.state,
                NotificationState::Claimed | NotificationState::Pending
            ),
            "must not advance to Delivered: {:?}",
            stored.state
        );
    }

    /// M017: Expected event reconstruction helper produces consistent
    /// content for all classification variants.
    #[test]
    fn expected_notification_event_has_consistent_content() {
        let completed = notification_with_classification(
            "n1",
            NotificationState::Pending,
            Some("ik1"),
            NotificationClassification::Completed,
            None,
            true,
        );
        let event = expected_notification_event(&completed, "sess").unwrap();
        let content = match &event {
            codegg_core::session::SessionEvent::ToolProgramNotification(e) => e.content.clone(),
            _ => panic!("expected ToolProgramNotification"),
        };
        assert_eq!(
            content,
            "Background program n1 completed successfully: summary-n1"
        );

        let failed = notification_with_classification(
            "n2",
            NotificationState::Pending,
            Some("ik2"),
            NotificationClassification::FailedTerminal,
            Some("timeout"),
            false,
        );
        let event = expected_notification_event(&failed, "sess").unwrap();
        let content = match &event {
            codegg_core::session::SessionEvent::ToolProgramNotification(e) => e.content.clone(),
            _ => panic!("expected ToolProgramNotification"),
        };
        assert_eq!(
            content,
            "Background program n2 failed terminally (timeout): summary-n2"
        );

        let incomplete = notification_with_classification(
            "n3",
            NotificationState::Pending,
            Some("ik3"),
            NotificationClassification::IncompleteRecoverable,
            Some("network"),
            false,
        );
        let event = expected_notification_event(&incomplete, "sess").unwrap();
        let content = match &event {
            codegg_core::session::SessionEvent::ToolProgramNotification(e) => e.content.clone(),
            _ => panic!("expected ToolProgramNotification"),
        };
        assert_eq!(
            content,
            "Background program n3 is incomplete but recoverable (network): summary-n3"
        );
    }
}

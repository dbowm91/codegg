//! Generic job terminal notification recovery (async-background-jobs M001).
//!
//! Mirrors the verified Tool Program M017 recovery loop without mutating its
//! tables: Pending / Claimed notices are driven to Delivered by appending
//! the parent-session event, marking injected, and acknowledging.
//!
//! Crash-safe for two interleavings:
//! - Crash after the parent-session event is appended but before
//!   `mark_injected`: the next loop reconstructs the expected event,
//!   `confirm_existing` observes a semantic match, marks injected, and
//!   acknowledges without re-injecting.
//! - Crash after `mark_injected` but before `acknowledge`: the loop
//!   observes `injected_event_id` and acknowledges directly.

use codegg_core::session::events::{EventMeta, JobTerminalNotificationEvent};
use codegg_core::session::EventStore;

use crate::scheduler::job_notifications::{
    format_job_notice_content, injection_event_id_for, JobNotificationService, JobTerminalNotice,
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InjectJobNotificationsReport {
    pub considered: usize,
    pub injected: usize,
    pub recovered_via_event: usize,
    pub already_injected: usize,
    pub leased: usize,
    pub skipped: usize,
    pub inject_confirmed: usize,
    pub semantic_collisions: usize,
    pub store_errors: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum JobNotificationRecoveryError {
    #[error("event-store query failed for {event_id}: {source}")]
    EventStoreQuery {
        event_id: String,
        #[source]
        source: codegg_providers::error::StorageError,
    },
    #[error("event-store semantic collision for {event_id}")]
    SemanticCollision { event_id: String },
    #[error("CAS conflict on {notice_id}: {detail}")]
    CasConflict { notice_id: String, detail: String },
}

pub fn expected_job_event(
    notice: &JobTerminalNotice,
    session_id: &str,
) -> codegg_core::session::SessionEvent {
    let content = format_job_notice_content(notice);
    let event_id = injection_event_id_for(notice);
    codegg_core::session::SessionEvent::JobTerminalNotification(JobTerminalNotificationEvent {
        meta: EventMeta {
            id: event_id,
            session_id: session_id.to_string(),
            created_at: chrono::Utc::now(),
        },
        injection_key: notice.injection_key.clone(),
        notice_id: notice.notice_id.clone(),
        job_id: notice.job_id.clone(),
        content,
    })
}

/// Reconcile Pending and Claimed job notices for `session_id`.
pub async fn inject_recoverable_job_notifications(
    event_store: Option<&EventStore>,
    service: &JobNotificationService,
    session_id: &str,
    mut on_message: impl FnMut(String),
) -> InjectJobNotificationsReport {
    let mut report = InjectJobNotificationsReport::default();
    let recoverable = match service.recoverable_for_session(session_id).await {
        Ok(v) => v,
        Err(e) => {
            report.store_errors += 1;
            report.errors.push(format!("recoverable load failed: {e}"));
            return report;
        }
    };
    report.considered = recoverable.len();
    let Some(store) = event_store else {
        report.skipped = recoverable.len();
        return report;
    };
    for notice in recoverable {
        // Invalid cross-session messages fail closed: the service only
        // returns notices for this session index, but double-check the
        // owning session before injecting.
        if notice.session_id != session_id {
            report.skipped += 1;
            continue;
        }
        let expected = expected_job_event(&notice, session_id);
        let expected_id = expected.meta().id.clone();
        // Already-injected path: durable mark exists, just acknowledge.
        if service.is_injected(&notice.notice_id).await {
            match service.acknowledge(&notice.notice_id).await {
                Ok(true) => {
                    report.already_injected += 1;
                }
                Ok(false) => {
                    report.leased += 1;
                }
                Err(e) => {
                    report.store_errors += 1;
                    report.errors.push(format!(
                        "ack already-injected {} failed: {e}",
                        notice.notice_id
                    ));
                }
            }
            continue;
        }
        // Confirm an existing durable event (crash after append).
        match store.confirm_existing(&expected).await {
            Ok(codegg_core::session::ConfirmExistingEvent::SemanticMatch) => {
                if let Err(e) = service.mark_injected(&notice.notice_id, &expected_id).await {
                    // Another worker won the mark race; treat as leased.
                    report.leased += 1;
                    report
                        .errors
                        .push(format!("mark_injected race for {}: {e}", notice.notice_id));
                    continue;
                }
                match service.acknowledge(&notice.notice_id).await {
                    Ok(true) => {
                        report.recovered_via_event += 1;
                    }
                    Ok(false) => {
                        report.leased += 1;
                    }
                    Err(e) => {
                        report.store_errors += 1;
                        report
                            .errors
                            .push(format!("ack recovered {} failed: {e}", notice.notice_id));
                    }
                }
                continue;
            }
            Ok(codegg_core::session::ConfirmExistingEvent::Absent) => {}
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("semantic collision") {
                    report.semantic_collisions += 1;
                    report.errors.push(format!(
                        "semantic collision for {}: {msg}",
                        notice.notice_id
                    ));
                    continue;
                }
                report.store_errors += 1;
                report
                    .errors
                    .push(format!("confirm {} failed: {msg}", notice.notice_id));
                continue;
            }
        }
        // Fresh path: claim (Pending->Claimed), append, mark, ack.
        // If already Claimed by a live owner, leave for lease expiry.
        match service.claim_as(&notice.notice_id, "job-recovery").await {
            Ok(true) => {}
            Ok(false) => {
                report.leased += 1;
                continue;
            }
            Err(e) => {
                report.store_errors += 1;
                report
                    .errors
                    .push(format!("claim {} failed: {e}", notice.notice_id));
                continue;
            }
        }
        if let Err(e) = store.append_idempotent(&expected).await {
            let msg = e.to_string();
            if msg.contains("identity collision") || msg.contains("semantic collision") {
                report.semantic_collisions += 1;
            } else {
                report.store_errors += 1;
            }
            report
                .errors
                .push(format!("append {} failed: {msg}", notice.notice_id));
            continue;
        }
        // Failpoint for crash-after-append/before-mark coverage.
        crate::test_failpoint::hit("job_notification_after_append_before_mark");
        if let Err(e) = service.mark_injected(&notice.notice_id, &expected_id).await {
            report.store_errors += 1;
            report
                .errors
                .push(format!("mark_injected {} failed: {e}", notice.notice_id));
            continue;
        }
        // Deliver to the live turn (safe boundary): the caller decides the
        // exact provider message shape; we provide the authenticated text.
        if let codegg_core::session::SessionEvent::JobTerminalNotification(evt) = &expected {
            on_message(evt.content.clone());
            report.injected += 1;
            report.inject_confirmed += 1;
        }
        match service.acknowledge(&notice.notice_id).await {
            Ok(true) => {}
            Ok(false) => {
                report.leased += 1;
            }
            Err(e) => {
                report.store_errors += 1;
                report
                    .errors
                    .push(format!("ack {} failed: {e}", notice.notice_id));
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{JobId, JobKind, JobPriority, JobRecord, JobSource, JobState};
    use codegg_core::workspace::WorkspaceId;

    fn job_record(session: &str) -> JobRecord {
        let now = chrono::Utc::now();
        JobRecord {
            job_id: JobId::new_unchecked("job-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: Some(session.to_string()),
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: codegg_core::jobs::JobPayload::Test {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: None,
                scope: None,
                parent_run_id: None,
            },
            resource_request: codegg_core::jobs::ResourceRequest::for_kind(JobKind::Test),
            state: JobState::Completed,
            current_attempt_id: None,
            attempt_count: 0,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            labels: Default::default(),
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
            target: Default::default(),
            timeout: None,
            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
        }
    }

    #[tokio::test]
    async fn expected_event_is_stable() {
        let svc = JobNotificationService::new();
        svc.subscribe(&JobId::new_unchecked("job-1"), "test", "s1", None, "inv-1")
            .await
            .unwrap();
        let notice = svc
            .record_terminal(&job_record("s1"), "ok", None)
            .await
            .unwrap()
            .unwrap();
        let a = expected_job_event(&notice, "s1");
        let b = expected_job_event(&notice, "s1");
        assert_eq!(a.meta().id, b.meta().id);
    }
}

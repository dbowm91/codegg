//! Generic job terminal notification recovery (async-background-jobs M001/M003).
//!
//! Mirrors the verified Tool Program M017 recovery loop without mutating its
//! tables: Pending / Claimed notices are driven to Delivered by appending
//! the parent-session event, marking injected, and acknowledging.
//!
//! M003 adds typed completion dispositions on top of the M001 durable
//! notice:
//!
//! - `notify_only` (default): persist a redacted attention event; the next
//!   model turn may inspect and consume it. No model tokens are spent by
//!   delivery itself. An ended turn does not retain a stale sender.
//! - `resume_owner`: the owning task explicitly authorized one bounded
//!   continuation for this wait. Delivery consumes a single-use lease
//!   (exactly one continuation per logical terminal) at the next safe,
//!   user-authorized turn boundary. Loss of the in-memory sender is not
//!   loss of authority; event append, ack, and lease consumption are
//!   reconciled as separate durable boundaries.
//! - `suppress`: cancelled/archived owner; audit/inspect only.
//!
//! Crash-safe for three interleavings:
//! - Crash after the parent-session event is appended but before
//!   `mark_injected`: the next loop confirms the event semantically,
//!   marks injected, consumes the lease if authorized, and acknowledges.
//! - Crash after `mark_injected` but before lease consumption: the loop
//!   observes `injected_event_id` and consumes the pending lease without
//!   re-injecting the message (exactly-once continuation).
//! - Crash after lease consumption but before `acknowledge`: the loop
//!   observes both markers and acknowledges directly.

use codegg_core::session::events::{EventMeta, JobTerminalNotificationEvent};
use codegg_core::session::EventStore;

use crate::scheduler::job_notifications::{
    format_job_notice_content, injection_event_id_for, JobCompletionDisposition,
    JobNotificationService, JobTerminalNotice,
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
    /// Notices delivered with an authorized single-use resume continuation.
    pub resumed: usize,
    /// Resume leases suppressed (cancel/expiry/owner mismatch/revocation).
    pub resume_suppressed: usize,
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

/// Whether a terminal status may carry an authorized continuation.
/// User cancellation, expiry, and suppression never resume: the cancel or
/// lease-expiry wins before any model invocation.
fn status_allows_resume(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "timed_out")
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
        // Exact-owner routing: only the owning turn's session receives the
        // notice. No session-wide opportunistic injection into a different
        // live turn; an ended turn retains no stale sender (delivery waits
        // for the next user-authorized turn boundary, which is this call).
        if notice.session_id != session_id {
            report.skipped += 1;
            continue;
        }
        let expected = expected_job_event(&notice, session_id);
        let expected_id = expected.meta().id.clone();
        // Already-injected path: durable mark exists. Consume a pending
        // resume lease without re-injecting (crash after mark, before
        // lease-consume), then acknowledge. The notice may still be Pending
        // (crash after mark but before claim, or a confirm-path recovery
        // that marked without claiming), so claim first — tolerating an
        // already-claimed state — before the Claimed→Delivered ack.
        if service.is_injected(&notice.notice_id).await {
            let _ = service.claim_as(&notice.notice_id, "job-recovery").await;
            settle_lease_for_delivered(service, &notice, session_id, &mut report).await;
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
        // Confirm an existing durable event (crash after append). Claim
        // first so the subsequent mark→ack chain starts from a known
        // state; an already-claimed notice (live owner elsewhere) is left
        // for lease expiry rather than force-transitioned.
        match store.confirm_existing(&expected).await {
            Ok(codegg_core::session::ConfirmExistingEvent::SemanticMatch) => {
                // If another live owner holds the claim, do not steal it;
                // the confirmed event means delivery is already durable, so
                // count the recovery without re-injecting.
                let claimed_here = match service.claim_as(&notice.notice_id, "job-recovery").await {
                    Ok(v) => v,
                    Err(e) => {
                        report.store_errors += 1;
                        report
                            .errors
                            .push(format!("claim recovered {} failed: {e}", notice.notice_id));
                        continue;
                    }
                };
                if let Err(e) = service.mark_injected(&notice.notice_id, &expected_id).await {
                    // A mark race means another worker already recorded the
                    // injection. Fall through to settle + ack: if the mark
                    // is now durable we still complete the recovery without
                    // re-injecting; otherwise the ack below leases it.
                    tracing::debug!(
                        notice_id = %notice.notice_id,
                        error = %e,
                        "job mark_injected race during confirmed recovery; settling anyway"
                    );
                    let _ = claimed_here;
                }
                settle_lease_for_delivered(service, &notice, session_id, &mut report).await;
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
        // Fresh path: claim (Pending->Claimed), append, mark, settle lease,
        // ack. If already Claimed by a live owner, leave for lease expiry.
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
        crate::test_failpoint::hit("job_notification_after_append_before_mark");
        if let Err(e) = service.mark_injected(&notice.notice_id, &expected_id).await {
            report.store_errors += 1;
            report
                .errors
                .push(format!("mark_injected {} failed: {e}", notice.notice_id));
            continue;
        }
        // Deliver to the live turn at this safe boundary. The resume marker
        // is appended only when a single-use lease was just consumed;
        // otherwise the plain authenticated content is delivered and no
        // model turn is synthesized or reopened.
        let mut text =
            if let codegg_core::session::SessionEvent::JobTerminalNotification(evt) = &expected {
                evt.content.clone()
            } else {
                format_job_notice_content(&notice)
            };
        if settle_lease_for_delivered(service, &notice, session_id, &mut report).await {
            text.push_str(
                " [resume authorized for this background wait: one bounded continuation]",
            );
            report.resumed += 1;
        }
        on_message(text);
        report.injected += 1;
        report.inject_confirmed += 1;
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

/// Settle the continuation lease for a notice whose session event is
/// durably committed. Returns `true` when a resume continuation was just
/// authorized (exactly once per logical terminal).
///
/// Rules (all fail toward explicit attention, never hidden retry):
/// - No subscription or disposition != resume_owner → no lease; `false`.
/// - Terminal cancelled/expired, or lease expired/owner-mismatched →
///   suppress the lease, count `resume_suppressed`; `false`.
/// - Valid pending lease + resumable status + owner match → CAS consume;
///   exactly one winner under duplicates/frontends; `true` iff consumed here
///   (a lease already consumed by a prior pass returns `false` so the
///   resume marker is not duplicated).
async fn settle_lease_for_delivered(
    service: &JobNotificationService,
    notice: &JobTerminalNotice,
    session_id: &str,
    report: &mut InjectJobNotificationsReport,
) -> bool {
    let sub = match service.get_subscription(&notice.job_id).await {
        Ok(s) => s,
        Err(e) => {
            report.store_errors += 1;
            report.errors.push(format!(
                "subscription load for {} failed: {e}",
                notice.notice_id
            ));
            return false;
        }
    };
    let Some(sub) = sub else {
        return false;
    };
    if sub.disposition != JobCompletionDisposition::ResumeOwner {
        return false;
    }
    // User cancellation, expiry, or suppression wins before invocation.
    if !status_allows_resume(&notice.status) {
        match service.suppress_lease(&notice.notice_id).await {
            Ok(_) => report.resume_suppressed += 1,
            Err(e) => {
                report.store_errors += 1;
                report
                    .errors
                    .push(format!("suppress lease {} failed: {e}", notice.notice_id));
            }
        }
        return false;
    }
    match service
        .consume_resume_lease(&notice.notice_id, session_id)
        .await
    {
        Ok(true) => true,
        Ok(false) => {
            // Distinguish expiry/mismatch (suppress + attention) from an
            // already-consumed lease (exactly-once: a prior pass won).
            let lease = service.get_lease(&notice.notice_id).await.ok().flatten();
            match lease.map(|l| l.status) {
                Some(crate::scheduler::job_notifications::JobContinuationStatus::Pending) => {
                    // Owner mismatch or budget: leave for explicit attention.
                    report.leased += 1;
                }
                Some(crate::scheduler::job_notifications::JobContinuationStatus::Expired) => {
                    report.resume_suppressed += 1;
                }
                Some(crate::scheduler::job_notifications::JobContinuationStatus::Suppressed) => {
                    report.resume_suppressed += 1;
                }
                // Consumed (by this or a prior pass) or absent: no marker.
                _ => {}
            }
            false
        }
        Err(e) => {
            report.store_errors += 1;
            report
                .errors
                .push(format!("consume lease {} failed: {e}", notice.notice_id));
            false
        }
    }
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
                sandbox_profile: None,
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

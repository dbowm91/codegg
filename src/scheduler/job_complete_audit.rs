//! Scheduler terminal `job_complete` live audit hook (identity/audit M003).
//!
//! Canonical ownership: the durable scheduler terminal attempt transition.
//! Emission happens after the terminal state is durably accepted, never from
//! TUI polling, projection/event observers, retry requests, or completion
//! consumers.
//!
//! - One terminal transition emits at most one structural event via a
//!   deterministic event id (`decision | job_complete | correlation |
//!   attempt:outcome`). Replayed completions of an already-terminal attempt
//!   fail at the store (`InvalidTransition`) before any emit, and recovery
//!   replays reuse the stored row via `ON CONFLICT DO NOTHING`.
//! - Attribution comes from the durable `OriginAttribution` row for scope
//!   `job` (first-write-wins at the admission boundary). Legacy rows without
//!   attribution fall back to the explicit `legacy-local`/`LocalOwner`
//!   rule; a current human principal is never manufactured.
//! - Metadata is limited to ids, bounded outcome/state labels, and the
//!   decision outcome. No job payload, tool output, command text, or secret
//!   material enters audit metadata.
//! - Audit failure is bounded best-effort (shared `ExecutionAuditEmitter`
//!   policy): timeouts/failures increment counters with a warn and never
//!   fail the owning scheduler terminalization.

use codegg_core::audit::AuditAction;
use codegg_core::audit_instrumentation::{
    deterministic_event_id, job_complete_event, ExecutionAuditEmitter, TrustedExecutionAuditContext,
};
use codegg_core::jobs::{AttemptId, JobId, JobRecord};

use crate::scheduler::executor::{ExecutorCompletion, ExecutorStatus};

/// Scheduler-owned client marker for reconstructed terminal principals.
///
/// The async terminal has no live connection; only the stored
/// principal/kind/method/transport are restored from durable attribution.
/// The marker makes the reconstruction explicit in stored rows.
pub const SCHEDULER_TERMINAL_CLIENT: &str = "scheduler-terminal";

/// Bounded `job.outcome` label for one executor terminal status.
///
/// Truthful and bounded: success/failure/cancelled/timed_out/interrupted.
/// Never invents success for a dispatch that never occurred; validation
/// rejections map to `failure` at the `mark_unschedulable` owner.
pub fn job_outcome_label(status: ExecutorStatus) -> &'static str {
    match status {
        ExecutorStatus::Completed => "success",
        ExecutorStatus::Failed => "failure",
        ExecutorStatus::Cancelled => "cancelled",
        ExecutorStatus::TimedOut => "timed_out",
        ExecutorStatus::Interrupted => "interrupted",
    }
}

/// Bounded `job.outcome` label for one persisted attempt state string.
///
/// Used by the `mark_unschedulable` owner, which terminalizes without an
/// executor status. Only terminal labels are produced; non-terminal input
/// degrades to `failure` rather than a fabricated success.
pub fn job_outcome_for_attempt_state(state: &str) -> &'static str {
    match state {
        "completed" => "success",
        "failed" => "failure",
        "cancelled" => "cancelled",
        "timed_out" => "timed_out",
        "interrupted" => "interrupted",
        _ => "failure",
    }
}

/// Deterministic idempotency scope for one terminal attempt transition.
///
/// Each attempt carries a distinct id, so the failed prior attempt and its
/// retry successor scope separately (two truthful rows). Replays of the
/// same attempt/outcome reuse the id and dedupe in the store.
pub fn terminal_scope(attempt_id: &AttemptId, job_outcome: &str) -> String {
    format!("{}:{job_outcome}", attempt_id.as_str())
}

/// Deterministic scope for a terminal cancel without an attempt.
///
/// Queued `request_cancel` terminalizes the job without creating an
/// attempt; the job id scopes the single terminal event. Replayed cancels
/// (`AlreadyTerminal`) never reach emission.
pub fn terminal_scope_for_job(job_id: &JobId, job_outcome: &str) -> String {
    format!("{}:{job_outcome}", job_id.as_str())
}

/// Build the causal chain for one terminal job from durable locators.
///
/// Project is omitted: jobs are workspace-scoped in the scheduler and the
/// durable `OriginAttribution` row carries no project. Session/turn/run/job
/// locators are included where the persisted record provides them.
pub fn chain_for_terminal_job(
    job: &JobRecord,
    run_id: Option<&str>,
) -> codegg_core::audit_instrumentation::AuditChainContext {
    let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
    chain.session_id = job.session_id.clone();
    chain.turn_id = job.turn_id.clone();
    chain.job_id = Some(job.job_id.as_str().to_owned());
    if let Some(run) = run_id {
        if !run.is_empty() {
            chain.run_id = Some(run.to_owned());
        }
    }
    chain
}

/// Resolve the trusted terminal context for one job.
///
/// Reads the durable `OriginAttribution` row for scope `job` from the
/// emitter's pool when available and rebuilds the bound principal plus the
/// gate-copied decision linkage losslessly. Falls back to the explicit
/// `legacy_local` rule when the pool is absent, the row is absent, or the
/// lookup fails (lookup failure warns and never fails terminalization).
pub async fn trusted_context_for_terminal_job(
    emitter: &ExecutionAuditEmitter,
    job: &JobRecord,
    run_id: Option<&str>,
) -> TrustedExecutionAuditContext {
    let chain = chain_for_terminal_job(job, run_id);
    let Some(pool) = emitter.pool_snapshot() else {
        return TrustedExecutionAuditContext::legacy_local(job.job_id.as_str());
    };
    let store = codegg_core::authorization::OriginAttributionStore::new(pool);
    let attribution = match store.get("job", job.job_id.as_str()).await {
        Ok(row) => row,
        Err(error) => {
            tracing::warn!(
                error = %error,
                job_id = %job.job_id.as_str(),
                "scheduler terminal audit attribution lookup failed; using legacy-local"
            );
            None
        }
    };
    let Some(attribution) = attribution else {
        return TrustedExecutionAuditContext::legacy_local(job.job_id.as_str());
    };
    // Legacy rows predate attribution: keep the explicit marker rather than
    // inventing a team identity.
    if attribution.is_legacy() {
        return TrustedExecutionAuditContext::legacy_local(job.job_id.as_str());
    }
    let principal = codegg_core::transport_auth::AuthenticatedPrincipal::reconstructed(
        attribution.origin_principal.clone(),
        attribution.origin_kind,
        attribution.auth_method,
        attribution.transport_class,
        SCHEDULER_TERMINAL_CLIENT,
    );
    let provenance = codegg_core::audit::AuditDecisionProvenance::new(
        attribution.decision_id.clone(),
        attribution.correlation_id.clone(),
        attribution.policy.as_str().to_owned(),
        None,
    );
    let mut chain = chain;
    if chain.correlation_id.as_ref().is_none_or(|c| c.is_empty()) {
        chain.correlation_id = Some(attribution.correlation_id.clone());
    }
    TrustedExecutionAuditContext::new(&principal, &provenance, chain)
}

/// Emit one structural `job_complete` event for a terminal attempt.
///
/// `scope` must come from [`terminal_scope`]. Best-effort through the
/// shared emitter: store failure/timeout surfaces counters plus a warn and
/// never fails the owning scheduler transition.
pub async fn emit_job_complete(
    emitter: &ExecutionAuditEmitter,
    ctx: &TrustedExecutionAuditContext,
    job_id: &JobId,
    job_outcome: &str,
    scope: &str,
) {
    let correlation = ctx
        .chain()
        .correlation_id
        .as_deref()
        .filter(|correlation| !correlation.is_empty())
        .unwrap_or_else(|| ctx.provenance().correlation_id());
    let event_id = deterministic_event_id(
        ctx.provenance().decision_id(),
        &AuditAction::JobComplete,
        correlation,
        scope,
    );
    emitter
        .emit_with(ctx, |principal, provenance, chain| {
            job_complete_event(
                principal,
                provenance,
                chain,
                job_id.as_str(),
                job_outcome,
                "allow",
            )
            .with_event_id(event_id.clone())
        })
        .await;
}

/// Emit the terminal event for one durably-accepted executor completion.
///
/// Resolves attribution (persisted row or legacy fallback), builds the
/// deterministic scope from the terminal attempt id plus the truthful
/// outcome label, and emits best-effort. Never fails the caller.
pub async fn emit_terminal_completion(
    emitter: &ExecutionAuditEmitter,
    job: &JobRecord,
    attempt_id: &AttemptId,
    completion: &ExecutorCompletion,
) {
    let job_outcome = job_outcome_label(completion.status);
    let run_id = completion.run_id.as_ref().map(|id| id.as_str());
    let ctx = trusted_context_for_terminal_job(emitter, job, run_id).await;
    let scope = terminal_scope(attempt_id, job_outcome);
    emit_job_complete(emitter, &ctx, &job.job_id, job_outcome, &scope).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_labels_stay_bounded_and_truthful() {
        assert_eq!(job_outcome_label(ExecutorStatus::Completed), "success");
        assert_eq!(job_outcome_label(ExecutorStatus::Failed), "failure");
        assert_eq!(job_outcome_label(ExecutorStatus::Cancelled), "cancelled");
        assert_eq!(job_outcome_label(ExecutorStatus::TimedOut), "timed_out");
        assert_eq!(
            job_outcome_label(ExecutorStatus::Interrupted),
            "interrupted"
        );
    }

    #[test]
    fn terminal_scopes_separate_attempts_and_outcomes() {
        let attempt = AttemptId::new_unchecked("attempt-1");
        let retry_scope = terminal_scope(&attempt, "success");
        assert_eq!(retry_scope, terminal_scope(&attempt, "success"));
        assert_ne!(retry_scope, terminal_scope(&attempt, "failure"));
        let other = AttemptId::new_unchecked("attempt-2");
        assert_ne!(retry_scope, terminal_scope(&other, "success"));
        let job = JobId::new_unchecked("job-1");
        assert_eq!(
            terminal_scope_for_job(&job, "cancelled"),
            terminal_scope_for_job(&job, "cancelled")
        );
        assert_ne!(
            terminal_scope_for_job(&job, "cancelled"),
            terminal_scope(&attempt, "cancelled")
        );
    }

    #[test]
    fn attempt_state_labels_never_invent_success() {
        assert_eq!(job_outcome_for_attempt_state("completed"), "success");
        assert_eq!(job_outcome_for_attempt_state("failed"), "failure");
        assert_eq!(job_outcome_for_attempt_state("created"), "failure");
        assert_eq!(job_outcome_for_attempt_state("running"), "failure");
    }
}

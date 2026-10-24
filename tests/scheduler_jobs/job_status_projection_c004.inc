//! C004 — the model-facing job status is a projection, not a terminality
//! predicate.
//!
//! The status a model reads for a job used to be "running" for every state in
//! which the job had not started yet — `Scheduled`, `Queued`, `Running` and
//! `Blocked` alike — so a caller asking "why has my job not started?" was
//! told "running" whichever of the four it was in. Each of the four now has
//! its own value, and the values a consumer already parses are preserved.

use codegg::scheduler::job_notifications::job_status_projection;
use codegg_core::jobs::JobState;

#[test]
fn a_scheduled_job_is_reported_as_scheduled() {
    assert_eq!(job_status_projection(&JobState::Scheduled), "scheduled");
    assert_ne!(
        job_status_projection(&JobState::Scheduled),
        job_status_projection(&JobState::Running),
        "a scheduled job must not read as a running one"
    );
}

#[test]
fn a_queued_job_is_reported_as_queued() {
    assert_eq!(job_status_projection(&JobState::Queued), "queued");
    assert_ne!(
        job_status_projection(&JobState::Queued),
        job_status_projection(&JobState::Running),
        "a queued job must not read as a running one"
    );
}

#[test]
fn a_running_job_is_reported_as_running() {
    assert_eq!(job_status_projection(&JobState::Running), "running");
}

#[test]
fn a_blocked_job_is_reported_as_blocked() {
    assert_eq!(job_status_projection(&JobState::Blocked), "blocked");
    assert_ne!(
        job_status_projection(&JobState::Blocked),
        job_status_projection(&JobState::Running),
        "a blocked job must not read as a running one"
    );
}

#[test]
fn an_interrupted_job_is_not_reported_as_a_terminal_one() {
    assert_eq!(job_status_projection(&JobState::Interrupted), "interrupted");
    assert_ne!(
        job_status_projection(&JobState::Interrupted),
        job_status_projection(&JobState::Completed),
        "an interrupted job must not read as a finished one"
    );
}

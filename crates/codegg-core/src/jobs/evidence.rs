//! One answer to "does this job state count as evidence that the work
//! SUCCEEDED?".
//!
//! This is a **different question** from "is the job finished?", which
//! [`super::JobState::is_terminal`] answers. The two are compatible, not in
//! conflict: a job that is not finished is also not evidence of success, and
//! a job that finished without succeeding is not evidence of success either.
//!
//! The evidence verdict is deliberately **fail-closed**. `Completed` passes;
//! every other terminal state — `Failed`, `Cancelled`, `TimedOut`, `Expired` —
//! and `Interrupted` fail closed; the non-terminal states are in progress and
//! are evidence of nothing yet. `Interrupted` fails closed even though the
//! domain does not call it terminal, because an interrupted attempt carries no
//! evidence that the work succeeded.
//!
//! This module is the **one** authority for that question. The three host
//! evidence assemblers — the goal verifier, the goal continuation assessor
//! and the work-plan assessor — project a job state onto their own
//! `HostEvidenceStatus` vocabulary through it, instead of each hand-writing a
//! match over the same enum. Three hand-written matches over one enum drift
//! the moment a state is added; one does not.

use super::JobState;

/// The verdict vocabulary a job state is projected onto.
///
/// Implemented by each host-evidence vocabulary in this crate, so the
/// projection itself stays a single function rather than one copy per
/// vocabulary.
pub trait EvidenceVerdict: Copy {
    /// The verdict for evidence that the work succeeded.
    fn passed() -> Self;
    /// The verdict for evidence that the work did not succeed.
    fn failed() -> Self;
    /// The verdict for work that has not finished yet.
    fn in_progress() -> Self;
}

/// The evidence verdict for one job state.
///
/// The question this answers is whether the job counts as **evidence that the
/// work succeeded** — not whether the job is finished. For that, see
/// [`JobState::is_terminal`].
///
/// The verdicts are unchanged from the three hand-written matches this
/// function replaces: `Completed` passes; `Failed`, `Cancelled`, `TimedOut`,
/// `Expired` and `Interrupted` fail closed; `Scheduled`, `Queued`, `Running`
/// and `Blocked` are in progress.
pub fn job_success_evidence<V: EvidenceVerdict>(state: JobState) -> V {
    match state {
        JobState::Completed => V::passed(),
        JobState::Failed
        | JobState::Cancelled
        | JobState::TimedOut
        | JobState::Interrupted
        | JobState::Expired => V::failed(),
        JobState::Scheduled | JobState::Queued | JobState::Running | JobState::Blocked => {
            V::in_progress()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::HostEvidenceStatus;

    /// The verdict one job state projects onto.
    fn verdict(state: JobState) -> HostEvidenceStatus {
        job_success_evidence(state)
    }

    #[test]
    fn the_evidence_projection_is_fail_closed_for_every_variant() {
        for state in JobState::ALL {
            let expected = match state {
                JobState::Completed => HostEvidenceStatus::Passed,
                JobState::Failed
                | JobState::Cancelled
                | JobState::TimedOut
                | JobState::Interrupted
                | JobState::Expired => HostEvidenceStatus::Failed,
                JobState::Scheduled | JobState::Queued | JobState::Running | JobState::Blocked => {
                    HostEvidenceStatus::InProgress
                }
            };
            assert_eq!(
                verdict(*state),
                expected,
                "{state:?} projects onto the wrong evidence verdict"
            );
        }
    }

    #[test]
    fn only_a_completed_job_is_positive_evidence() {
        for state in JobState::ALL {
            assert_eq!(
                verdict(*state) == HostEvidenceStatus::Passed,
                *state == JobState::Completed,
                "{state:?} must not be positive evidence of success"
            );
        }
    }

    #[test]
    fn an_interrupted_job_fails_closed_even_though_it_is_not_terminal() {
        assert!(
            !JobState::Interrupted.is_terminal(),
            "the domain does not call an interrupted job terminal"
        );
        assert_eq!(
            verdict(JobState::Interrupted),
            HostEvidenceStatus::Failed,
            "an interrupted job carries no evidence that the work succeeded"
        );
    }
}

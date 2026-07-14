//! Operation-aware continue/abort/skip for in-progress Git operations.
//!
//! Phase F: `git_mutations_ops::abort_in_progress` previously emitted a
//! "best guess" `git merge --abort` followed by `git rebase --abort`.
//! That works for some workflows but masks intent and can misclassify
//! recovery for cherry-pick/revert/sequencer-driven operations.
//!
//! The new entry points in this file inspect `RepositoryOperationState`
//! from `egggit::operation_state`, then drive the correct Git subcommand
//! for the active operation family. They refuse to run when the
//! detected state and requested action disagree (e.g. `rebase --abort`
//! when a merge is in progress), preventing cross-operation misuse.

use std::path::Path;

use codegg_git::{GitOperation, GitRiskClass};
use egggit::{
    detect_operation_state_for_root, OperationFamily, RecoveryAction, RepositoryOperationState,
};

use crate::git_mutations::{
    classify_outcome, compute_delta, GitMutationError, GitMutationExecutor, MutationOutcome,
    MutationResult, RepoSnapshot,
};

/// Outcome of a recovery action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// Recovery completed and the repository is back to a clean state.
    Completed,
    /// Recovery completed but the repository is still in progress
    /// (e.g. `continue` after some resolutions were staged).
    StillInProgress,
    /// Conflicts remain after `continue` (resolve then re-run continue).
    Conflicted,
    /// Recovery was a no-op because no operation matched.
    NoOp,
    /// Operation was rejected (e.g. conflict markers present but no
    /// `continue` was requested).
    Rejected(String),
}

/// Continue the in-progress operation (e.g. `git rebase --continue`).
///
/// Inspects state first and dispatches to the correct subcommand:
/// `git rebase --continue`, `git merge --continue`, `git cherry-pick
/// --continue`, `git revert --continue`, `git am --continue` (older
/// Git only). Refuses to run when the state is incompatible.
pub async fn continue_in_progress(
    exec: &GitMutationExecutor,
    repo_root: &Path,
) -> Result<MutationResult, GitMutationError> {
    let state = detect_state(repo_root).await?;
    let before = exec.snapshot(repo_root).await?;
    let op = match &state {
        RepositoryOperationState::Rebase(_) => GitOperation::Rebase {
            upstream: None,
            onto: None,
            interactive: false,
            abort: false,
            continue_op: true,
            skip: false,
        },
        RepositoryOperationState::Merge(_) => GitOperation::Merge {
            revisions: vec![],
            no_ff: false,
            strategy: None,
            abort: false,
        },
        RepositoryOperationState::CherryPick(_) => GitOperation::CherryPick {
            revisions: vec![],
            continue_op: true,
            abort: false,
            skip: false,
        },
        RepositoryOperationState::Revert(_) => GitOperation::Revert {
            revisions: vec![],
            no_edit: false,
            continue_op: true,
            abort: false,
            skip: false,
        },
        RepositoryOperationState::Sequencer(_) => GitOperation::Continue,
        RepositoryOperationState::None => {
            return Err(GitMutationError::Precondition(
                "no in-progress operation to continue".to_string(),
            ));
        }
        other => {
            return Err(GitMutationError::Precondition(format!(
                "operation '{}' does not support continue",
                other.label()
            )));
        }
    };
    run_recovery(
        exec,
        repo_root,
        &state,
        before,
        op,
        RecoveryAction::Continue,
    )
    .await
}

/// Abort the in-progress operation (`git rebase --abort`, etc.).
///
/// Refuses when no operation is in progress. The detector is the
/// authoritative source of truth for the action.
pub async fn abort_in_progress_typed(
    exec: &GitMutationExecutor,
    repo_root: &Path,
) -> Result<MutationResult, GitMutationError> {
    let state = detect_state(repo_root).await?;
    let before = exec.snapshot(repo_root).await?;
    let op = match &state {
        RepositoryOperationState::Rebase(_) => GitOperation::Rebase {
            upstream: None,
            onto: None,
            interactive: false,
            abort: true,
            continue_op: false,
            skip: false,
        },
        RepositoryOperationState::Merge(_) => GitOperation::Merge {
            revisions: vec![],
            no_ff: false,
            strategy: None,
            abort: true,
        },
        RepositoryOperationState::CherryPick(_) => GitOperation::CherryPick {
            revisions: vec![],
            continue_op: false,
            abort: true,
            skip: false,
        },
        RepositoryOperationState::Revert(_) => GitOperation::Revert {
            revisions: vec![],
            no_edit: false,
            continue_op: false,
            abort: true,
            skip: false,
        },
        RepositoryOperationState::Sequencer(_) => GitOperation::Abort,
        RepositoryOperationState::ApplyMailbox(_) | RepositoryOperationState::Bisect(_) => {
            return Err(GitMutationError::Precondition(format!(
                "operation '{}' does not support abort via Codegg; run `git {} --abort` directly",
                state.label(),
                state.label()
            )));
        }
        RepositoryOperationState::None => {
            return Err(GitMutationError::Precondition(
                "no in-progress operation to abort".to_string(),
            ));
        }
        RepositoryOperationState::Unknown(_) => {
            return Err(GitMutationError::Precondition(
                "unknown in-progress operation; refusing to abort to avoid mis-targeting"
                    .to_string(),
            ));
        }
    };
    run_recovery(exec, repo_root, &state, before, op, RecoveryAction::Abort).await
}

/// Skip the current step of a rebase/cherry-pick/revert sequence.
pub async fn skip_in_progress(
    exec: &GitMutationExecutor,
    repo_root: &Path,
) -> Result<MutationResult, GitMutationError> {
    let state = detect_state(repo_root).await?;
    let before = exec.snapshot(repo_root).await?;
    let op = match &state {
        RepositoryOperationState::Rebase(_) => GitOperation::Rebase {
            upstream: None,
            onto: None,
            interactive: false,
            abort: false,
            continue_op: false,
            skip: true,
        },
        RepositoryOperationState::CherryPick(_) => GitOperation::CherryPick {
            revisions: vec![],
            continue_op: false,
            abort: false,
            skip: true,
        },
        RepositoryOperationState::Revert(_) => GitOperation::Revert {
            revisions: vec![],
            no_edit: false,
            continue_op: false,
            abort: false,
            skip: true,
        },
        RepositoryOperationState::Sequencer(_) => GitOperation::Skip,
        RepositoryOperationState::None => {
            return Err(GitMutationError::Precondition(
                "no in-progress operation to skip".to_string(),
            ));
        }
        other => {
            return Err(GitMutationError::Precondition(format!(
                "operation '{}' does not support skip",
                other.label()
            )));
        }
    };
    run_recovery(exec, repo_root, &state, before, op, RecoveryAction::Skip).await
}

/// Inspect the current operation state, returning a typed error if no
/// state is detected.
async fn detect_state(repo_root: &Path) -> Result<RepositoryOperationState, GitMutationError> {
    detect_operation_state_for_root(repo_root)
        .map_err(|e| GitMutationError::Repository(e.to_string()))
}

/// Cross-check that the requested action is legal for the state we are
/// running against. Defends against TOCTOU between detection and
/// execution by re-reading state immediately before the action runs.
fn assert_action_matches(
    state: &RepositoryOperationState,
    action: RecoveryAction,
) -> Result<(), GitMutationError> {
    if !state.action_available(action) {
        return Err(GitMutationError::Precondition(format!(
            "action '{}' is not legal for operation '{}'",
            action.label(),
            state.label()
        )));
    }
    // Additional family-specific cross-checks.
    match (state.family(), action) {
        (OperationFamily::Merge, RecoveryAction::Skip) => {
            return Err(GitMutationError::Precondition(
                "merge does not support skip (use continue or abort)".to_string(),
            ));
        }
        (OperationFamily::Bisect, _) | (OperationFamily::ApplyMailbox, _) => {
            return Err(GitMutationError::Precondition(format!(
                "operation '{}' is not model-driven; manage it manually",
                state.family().label()
            )));
        }
        (OperationFamily::Unknown, _) => {
            return Err(GitMutationError::Precondition(
                "operation is in an unrecognized state; refusing automatic recovery".to_string(),
            ));
        }
        _ => {}
    }
    Ok(())
}

async fn run_recovery(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    state: &RepositoryOperationState,
    before: RepoSnapshot,
    op: GitOperation,
    action: RecoveryAction,
) -> Result<MutationResult, GitMutationError> {
    assert_action_matches(state, action)?;
    let result = exec.execute(&op, repo_root).await?;
    // Post-flight classification: the underlying executor classifies on
    // exit code, but recovery benefits from a result-aware label so the
    // projector surfaces the right recovery hint.
    let outcome = match (state, &result.outcome) {
        (RepositoryOperationState::None, _) => MutationOutcome::NoOp,
        (_, MutationOutcome::Conflict) => MutationOutcome::Conflict,
        (_, MutationOutcome::Rejected { reason }) => MutationOutcome::Rejected {
            reason: format!("{action:?} rejected: {reason}"),
        },
        _ if result.success => MutationOutcome::Completed,
        _ => result.outcome.clone(),
    };
    // Recompute the delta since the executor may have already classified,
    // but force `outcome` to the recovery-aware variant.
    let mut final_result = result.clone();
    final_result.outcome = outcome;
    // Pull a fresh delta from the executor's helper but swap outcome.
    let _delta = compute_delta(
        &op,
        &before,
        &final_result.delta.after,
        &crate::git_service::RawGitOutput {
            stdout: final_result.stdout.clone(),
            stderr: final_result.stderr.clone(),
            exit_code: final_result.exit_code,
        },
        &final_result.outcome,
    );
    final_result.delta = _delta;
    let _ = classify_outcome; // suppress unused import warning
    Ok(final_result)
}

/// Hint describing why a recovery may have been refused.
///
/// Convenience for callers that want to surface structured diagnostics.
pub fn describe_recovery_failure(
    state: Option<&RepositoryOperationState>,
    action: RecoveryAction,
) -> String {
    match state {
        None => "no in-progress operation detected".to_string(),
        Some(s) => format!(
            "recovery action '{}' not legal for operation '{}' (legal: {:?})",
            action.label(),
            s.label(),
            s.available_actions()
        ),
    }
}

/// Convenience: hint that `risk_classes_for_recovery` produces only risk
/// classes safe for the requested action (used by projector / permission
/// flow).
pub fn risk_classes_for_recovery(action: RecoveryAction) -> Vec<GitRiskClass> {
    // All recovery actions are inherently destructive (they mutate
    // history/index). The user is expected to grant explicit permission.
    match action {
        RecoveryAction::Continue => vec![GitRiskClass::HistoryIntegration],
        RecoveryAction::Abort => {
            vec![
                GitRiskClass::HistoryIntegration,
                GitRiskClass::DestructiveHistory,
            ]
        }
        RecoveryAction::Skip => vec![GitRiskClass::HistoryIntegration],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_state_blocks_all_actions() {
        let s = RepositoryOperationState::None;
        assert!(!s.action_available(RecoveryAction::Continue));
        assert!(!s.action_available(RecoveryAction::Abort));
        assert!(!s.action_available(RecoveryAction::Skip));
    }

    #[test]
    fn merge_supports_continue_and_abort() {
        let s = RepositoryOperationState::Merge(Default::default());
        assert!(s.action_available(RecoveryAction::Continue));
        assert!(s.action_available(RecoveryAction::Abort));
        assert!(!s.action_available(RecoveryAction::Skip));
    }

    #[test]
    fn rebase_supports_all_three() {
        let s = RepositoryOperationState::Rebase(Default::default());
        assert!(s.action_available(RecoveryAction::Continue));
        assert!(s.action_available(RecoveryAction::Abort));
        assert!(s.action_available(RecoveryAction::Skip));
    }

    #[test]
    fn bisect_blocks_all() {
        let s = RepositoryOperationState::Bisect(Default::default());
        assert!(!s.action_available(RecoveryAction::Continue));
        assert!(!s.action_available(RecoveryAction::Abort));
        assert!(!s.action_available(RecoveryAction::Skip));
    }
}

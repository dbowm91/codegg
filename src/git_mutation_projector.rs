//! Git mutation projector.
//!
//! The mutation framework returns typed [`MutationResult`] values. The
//! projector formats those values into concise, structured summaries
//! suitable for model context and TUI transcript rows.
//!
//! Unlike the shell-output projectors in `src/shell/projector.rs`, the
//! git mutation projector does not consume a `CommandOutputStore` — it
//! is a pure formatter over the typed `MutationResult` because mutation
//! metadata is captured at execution time, not as stream data.

use std::fmt::Write;

use crate::git_mutations::{MutationOutcome, MutationResult};

/// Format a `MutationResult` into a structured, human-readable summary.
///
/// The summary highlights what the model needs to know:
///
/// * operation performed
/// * before/after HEAD/branch
/// * created commits / refs
/// * affected paths
/// * remaining dirty state
/// * conflicts (when present)
/// * recovery instructions (when needed)
pub fn project_mutation(result: &MutationResult) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "git {} — {}",
        result.subcommand,
        result.outcome.label()
    );
    let _ = writeln!(
        out,
        "  before: HEAD={} branch={} ({} staged, {} unstaged, {} untracked, {} conflicts)",
        short(&result.delta.before.head),
        result.delta.before.branch,
        result.delta.before.staged_count,
        result.delta.before.unstaged_count,
        result.delta.before.untracked_count,
        result.delta.before.conflicted_count
    );
    let _ = writeln!(
        out,
        "  after:  HEAD={} branch={} ({} staged, {} unstaged, {} untracked, {} conflicts)",
        short(&result.delta.after.head),
        result.delta.after.branch,
        result.delta.after.staged_count,
        result.delta.after.unstaged_count,
        result.delta.after.untracked_count,
        result.delta.after.conflicted_count
    );

    if !result.delta.commits_created.is_empty() {
        let _ = writeln!(out, "  commits created:");
        for c in &result.delta.commits_created {
            let _ = writeln!(out, "    - {c}");
        }
    }
    if !result.delta.refs_created.is_empty() {
        let _ = writeln!(out, "  refs created:");
        for r in &result.delta.refs_created {
            let _ = writeln!(out, "    - {r}");
        }
    }
    if !result.delta.refs_deleted.is_empty() {
        let _ = writeln!(out, "  refs deleted:");
        for r in &result.delta.refs_deleted {
            let _ = writeln!(out, "    - {r}");
        }
    }
    if !result.delta.paths_staged.is_empty() {
        let _ = writeln!(
            out,
            "  paths staged: {}",
            result.delta.paths_staged.join(", ")
        );
    }
    if !result.delta.paths_unstaged.is_empty() {
        let _ = writeln!(
            out,
            "  paths unstaged: {}",
            result.delta.paths_unstaged.join(", ")
        );
    }
    if !result.delta.conflicts.is_empty() {
        let _ = writeln!(out, "  conflicts:");
        for c in &result.delta.conflicts {
            let _ = writeln!(out, "    - {c}");
        }
        let _ = writeln!(
            out,
            "  recovery: resolve conflicts, then `git add <path>` and `git {kind} --continue`.",
            kind = result.subcommand
        );
    }

    // Outcome-specific notes.
    match &result.outcome {
        MutationOutcome::FastForward { from, to } => {
            let _ = writeln!(out, "  fast-forwarded {} → {}", short(from), short(to));
        }
        MutationOutcome::NoOp => {
            let _ = writeln!(out, "  no state change");
        }
        MutationOutcome::Rejected { reason } => {
            let _ = writeln!(out, "  rejected: {reason}");
        }
        _ => {}
    }

    if result.exit_code != 0 {
        let _ = writeln!(out, "  exit code: {}", result.exit_code);
    }
    if !result.success && !result.stderr.is_empty() {
        let trimmed = result.stderr.trim();
        if !trimmed.is_empty() {
            let _ = writeln!(out, "  stderr: {}", one_line(trimmed));
        }
    }
    if !result.stdout.is_empty() && result.delta.commits_created.is_empty() {
        let trimmed = result.stdout.trim();
        if !trimmed.is_empty() {
            let _ = writeln!(out, "  stdout: {}", one_line(trimmed));
        }
    }
    let _ = writeln!(out, "  duration: {} ms", result.duration_ms);
    out
}

/// Shorten a hash for display (first 7 chars). Returns input unchanged if
/// it is already ≤ 7 chars.
fn short(s: &str) -> String {
    if s.len() > 7 {
        s[..7].to_string()
    } else {
        s.to_string()
    }
}

/// Collapse multi-line text to a single line for summary output.
fn one_line(s: &str) -> String {
    s.replace('\n', " ").replace('\r', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_mutations::{RepoSnapshot, StateDelta};
    use codegg_git::GitOperation;

    fn fake_result() -> MutationResult {
        MutationResult {
            operation: GitOperation::Commit {
                message: "fix".into(),
                amend: false,
                allow_empty: false,
            },
            subcommand: "commit".to_string(),
            delta: StateDelta {
                before: RepoSnapshot {
                    head: "abc1234".into(),
                    branch: "main".into(),
                    detached: false,
                    staged_count: 2,
                    unstaged_count: 0,
                    untracked_count: 0,
                    conflicted_count: 0,
                    captured_at: chrono::Utc::now(),
                    raw_status: None,
                },
                after: RepoSnapshot {
                    head: "def5678".into(),
                    branch: "main".into(),
                    detached: false,
                    staged_count: 0,
                    unstaged_count: 0,
                    untracked_count: 0,
                    conflicted_count: 0,
                    captured_at: chrono::Utc::now(),
                    raw_status: None,
                },
                commits_created: vec!["def5678".into()],
                refs_created: vec![],
                refs_deleted: vec![],
                paths_staged: vec![],
                paths_unstaged: vec![],
                conflicts: vec![],
            },
            outcome: MutationOutcome::Completed,
            stdout: "[main def5678] fix".into(),
            stderr: String::new(),
            exit_code: 0,
            success: true,
            duration_ms: 42,
        }
    }

    #[test]
    fn projection_includes_outcome_and_head_change() {
        let summary = project_mutation(&fake_result());
        assert!(summary.contains("git commit"));
        assert!(summary.contains("completed"));
        assert!(summary.contains("abc1234"));
        assert!(summary.contains("def5678"));
        assert!(summary.contains("commits created"));
        assert!(summary.contains("duration: 42 ms"));
    }

    #[test]
    fn projection_includes_conflicts_and_recovery_hint() {
        let mut r = fake_result();
        r.delta.conflicts = vec!["src/main.rs".into()];
        r.outcome = MutationOutcome::Conflict;
        r.delta.after.conflicted_count = 1;
        let summary = project_mutation(&r);
        assert!(summary.contains("src/main.rs"));
        assert!(summary.contains("recovery:"));
    }

    #[test]
    fn projection_marks_rejected_outcome() {
        let mut r = fake_result();
        r.outcome = MutationOutcome::Rejected {
            reason: "tests failed".into(),
        };
        r.success = false;
        r.exit_code = 1;
        let summary = project_mutation(&r);
        assert!(summary.contains("rejected"));
        assert!(summary.contains("exit code: 1"));
    }
}

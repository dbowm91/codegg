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

/// Format a network operation summary (fetch/pull/push/remote/config).
///
/// These operations don't create local commits, but they do change refs
/// (fetch/push) or repository config (remote/config). We surface those
/// deltas in a compact form.
pub fn project_network_mutation(result: &MutationResult) -> String {
    let mut out = project_mutation(result);
    if !result.stdout.trim().is_empty() {
        // Pull/fetch summary lines (e.g. "Updating abc..def\nFast-forward").
        let trimmed = result.stdout.trim();
        if !trimmed.is_empty() && trimmed.lines().count() <= 8 {
            let _ = writeln!(out, "  network output:");
            for line in trimmed.lines() {
                let _ = writeln!(out, "    {line}");
            }
        } else {
            let _ = writeln!(out, "  network output: {} bytes (truncated)", trimmed.len());
        }
    }
    out
}

/// Format a destructive operation summary (reset/clean).
///
/// Highlights the destructive nature, lists removed paths when known,
/// and prints the recovery instruction.
pub fn project_destructive_mutation(result: &MutationResult) -> String {
    let mut out = project_mutation(result);
    let _ = writeln!(
        out,
        "  destructive: this operation rewrote history or removed files; \
         recovery is `git reflog` + `git reset --hard <sha>`."
    );
    out
}

/// Format a recovery operation summary (continue/abort/skip).
///
/// Highlights the operation-aware recovery semantics, the legal
/// recovery actions that remain available after this one completed,
/// and a one-line next-step hint tailored to the outcome.
pub fn project_recovery(result: &MutationResult, action: &str, family: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "git recover ({action}) — operation: {family} — {}",
        result.outcome.label()
    );
    let _ = writeln!(
        out,
        "  before: HEAD={} branch={} (conflicts={})",
        short(&result.delta.before.head),
        result.delta.before.branch,
        result.delta.before.conflicted_count
    );
    let _ = writeln!(
        out,
        "  after:  HEAD={} branch={} (conflicts={})",
        short(&result.delta.after.head),
        result.delta.after.branch,
        result.delta.after.conflicted_count
    );
    let next_label = match &result.outcome {
        MutationOutcome::Completed => match action {
            "abort" => "operation aborted; repository back to clean state",
            "continue" => "operation continued cleanly",
            "skip" => "step skipped; operation advanced",
            _ => "operation completed",
        },
        MutationOutcome::NoOp => "no action taken (state already aligned with request)",
        MutationOutcome::Conflict => {
            "still in progress — resolve conflict markers, \
            stage resolutions with `git add <path>`, then re-run `recover: continue`"
        }
        MutationOutcome::FastForward { .. } => "operation advanced HEAD",
        MutationOutcome::Rejected { reason } => {
            let _ = writeln!(out, "  rejected: {reason}");
            "recovery rejected by policy or git"
        }
    };
    let _ = writeln!(out, "  next: {next_label}");
    if result.exit_code != 0 {
        let _ = writeln!(out, "  exit code: {}", result.exit_code);
    }
    if !result.success && !result.stderr.is_empty() {
        let trimmed = result.stderr.trim();
        if !trimmed.is_empty() {
            let _ = writeln!(out, "  stderr: {}", one_line(trimmed));
        }
    }
    let _ = writeln!(out, "  duration: {} ms", result.duration_ms);
    out
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

    #[test]
    fn network_projector_includes_stdout() {
        let mut r = fake_result();
        r.subcommand = "fetch".to_string();
        r.operation = GitOperation::Fetch {
            remote: None,
            refspecs: vec![],
            all: true,
        };
        r.stdout = "From origin\n   abc1234..def5678  main -> origin/main\n".to_string();
        r.delta.commits_created.clear();
        let summary = project_network_mutation(&r);
        assert!(summary.contains("git fetch"));
        assert!(summary.contains("From origin"));
        assert!(summary.contains("network output:"));
    }

    #[test]
    fn destructive_projector_warns_recovery() {
        let mut r = fake_result();
        r.subcommand = "reset".to_string();
        r.operation = GitOperation::ResetHard {
            rev: Some("HEAD~3".to_string()),
        };
        let summary = project_destructive_mutation(&r);
        assert!(summary.contains("git reset"));
        assert!(summary.contains("destructive:"));
        assert!(summary.contains("git reflog"));
    }

    #[test]
    fn recovery_projector_includes_action_and_family() {
        let r = fake_result();
        let summary = project_recovery(&r, "abort", "merge");
        assert!(summary.contains("git recover (abort)"));
        assert!(summary.contains("operation: merge"));
        assert!(summary.contains("next:"));
    }

    #[test]
    fn recovery_projector_continue_with_conflict_suggests_resolve() {
        let mut r = fake_result();
        r.outcome = MutationOutcome::Conflict;
        r.success = false;
        r.exit_code = 1;
        r.delta.after.conflicted_count = 1;
        let summary = project_recovery(&r, "continue", "merge");
        assert!(summary.contains("conflicts=1"));
        assert!(summary.contains("resolve conflict markers"));
        assert!(summary.contains("recover: continue"));
    }

    #[test]
    fn recovery_projector_abort_completed_message() {
        let r = fake_result();
        let summary = project_recovery(&r, "abort", "rebase");
        assert!(summary.contains("aborted"));
        assert!(summary.contains("clean state"));
    }

    // ── D2: Credential-bearing output redaction in projection ────────
    //
    // The corrective security closure pass requires that no
    // credential-bearing URL survives in projected output. The
    // projector itself does not redact — the redaction happens
    // upstream at `sanitize_truncate_for_result` in
    // `git_mutations::execute`. These tests verify the projector
    // faithfully renders already-redacted input without
    // reintroducing credentials.

    fn credential_result(subcommand: &str, stdout: &str, stderr: &str) -> MutationResult {
        let mut r = fake_result();
        r.subcommand = subcommand.to_string();
        r.operation = GitOperation::Fetch {
            remote: Some(codegg_git::RemoteName::new("origin").expect("valid")),
            refspecs: vec![],
            all: false,
        };
        // Simulate the production boundary: stdout/stderr have
        // already been passed through `redact_url_credentials_in_text`
        // before reaching the projector.
        r.stdout = crate::git_network_policy::redact_url_credentials_in_text(stdout);
        r.stderr = crate::git_network_policy::redact_url_credentials_in_text(stderr);
        r
    }

    #[test]
    fn projection_does_not_reintroduce_credentials_from_redacted_stdout() {
        // Input has credentials; redactor strips them; projector must
        // render the redacted form without re-introducing the
        // credential segment.
        let mut r = credential_result(
            "fetch",
            "From https://user:secret_token@github.com/r.git\n\
             \x20\x20\x20\x20abc1234..def5678  main -> origin/main\n",
            "",
        );
        // For stdout to render in the projector, success must be true
        // AND delta.commits_created must be empty.
        r.success = true;
        r.delta.commits_created.clear();
        assert!(
            !r.stdout.contains("secret_token"),
            "sanitize step did not strip credential: {}",
            r.stdout
        );
        let summary = project_mutation(&r);
        assert!(
            !summary.contains("secret_token"),
            "projected stdout leaked credential: {summary}"
        );
        assert!(
            summary.contains("github.com"),
            "expected host to remain visible: {summary}"
        );
    }

    #[test]
    fn projection_does_not_reintroduce_credentials_from_redacted_stderr() {
        let mut r = credential_result(
            "fetch",
            "",
            "fatal: unable to access 'https://u:pw@host.example.com/r.git/': \
             Authentication failed",
        );
        // For stderr to render in the projector, success must be false.
        r.success = false;
        r.exit_code = 128;
        assert!(
            !r.stderr.contains("u:pw"),
            "sanitize step did not strip credential: {}",
            r.stderr
        );
        let summary = project_mutation(&r);
        assert!(
            !summary.contains("u:pw"),
            "projected stderr leaked credential: {summary}"
        );
        assert!(
            summary.contains("host.example.com"),
            "expected host to remain visible: {summary}"
        );
    }

    #[test]
    fn network_projector_does_not_reintroduce_credentials() {
        let mut r = credential_result(
            "fetch",
            "From https://alice:hunter2@host.example.com/r.git\n\
             \x20\x20\x20\x20abc..def  main -> origin/main\n",
            "",
        );
        r.success = true;
        r.delta.commits_created.clear();
        let summary = project_network_mutation(&r);
        assert!(
            !summary.contains("hunter2"),
            "network projector leaked credential: {summary}"
        );
    }

    #[test]
    fn recovery_projector_does_not_reintroduce_credentials_in_stderr() {
        let mut r = credential_result(
            "recover",
            "",
            "fatal: unable to access 'https://user:secret@host/x.git': DNS failure",
        );
        r.outcome = MutationOutcome::Rejected {
            reason: "git exited with code 128".to_string(),
        };
        r.success = false;
        r.exit_code = 128;
        let summary = project_recovery(&r, "continue", "fetch");
        assert!(
            !summary.contains("user:secret"),
            "recovery projector leaked credential: {summary}"
        );
    }

    // ── Golden fixture tests ──────────────────────────────────────────

    use crate::git_mutations::MutationResult;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct GoldenFixture {
        fixture: GoldenFixtureInfo,
        input_json: MutationResult,
        expect: GoldenExpect,
    }

    #[derive(Deserialize)]
    struct GoldenFixtureInfo {
        name: String,
        projector: String,
        #[serde(default)]
        action: Option<String>,
        #[serde(default)]
        family: Option<String>,
    }

    #[derive(Deserialize)]
    struct GoldenExpect {
        output: String,
    }

    fn fixture_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/git_mutation_projection")
    }

    fn discover_golden_fixtures() -> Vec<(String, std::path::PathBuf)> {
        let dir = fixture_dir();
        let mut fixtures = Vec::new();
        if !dir.is_dir() {
            return fixtures;
        }
        for entry in std::fs::read_dir(&dir).expect("fixture dir readable") {
            let entry = entry.expect("dir entry readable");
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                fixtures.push((name, path));
            }
        }
        fixtures.sort_by(|a, b| a.0.cmp(&b.0));
        fixtures
    }

    fn load_golden_fixture(path: &std::path::Path) -> GoldenFixture {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path.display(), e));
        toml::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse fixture {}: {}", path.display(), e))
    }

    #[test]
    fn golden_fixtures_parse() {
        let fixtures = discover_golden_fixtures();
        assert!(
            !fixtures.is_empty(),
            "No golden fixtures found in {:?}",
            fixture_dir()
        );
        for (name, path) in &fixtures {
            let f = load_golden_fixture(path);
            assert_eq!(f.fixture.name, *name, "Fixture name mismatch in {}", name);
            assert!(
                matches!(
                    f.fixture.projector.as_str(),
                    "mutation" | "network" | "destructive" | "recovery"
                ),
                "Fixture {} has unknown projector '{}'",
                name,
                f.fixture.projector
            );
        }
    }

    #[test]
    fn golden_fixtures_match_projector_output() {
        let fixtures = discover_golden_fixtures();
        assert!(!fixtures.is_empty(), "No golden fixtures found");

        for (name, path) in &fixtures {
            let f = load_golden_fixture(path);
            let actual = match f.fixture.projector.as_str() {
                "mutation" => project_mutation(&f.input_json),
                "network" => project_network_mutation(&f.input_json),
                "destructive" => project_destructive_mutation(&f.input_json),
                "recovery" => {
                    let action = f
                        .fixture
                        .action
                        .as_deref()
                        .expect("recovery fixture must specify action");
                    let family = f
                        .fixture
                        .family
                        .as_deref()
                        .expect("recovery fixture must specify family");
                    project_recovery(&f.input_json, action, family)
                }
                _ => unreachable!(),
            };
            assert_eq!(
                actual, f.expect.output,
                "Golden fixture '{}' output mismatch.\n--- expected ---\n{}\n--- actual ---\n{}",
                name, f.expect.output, actual
            );
        }
    }
}

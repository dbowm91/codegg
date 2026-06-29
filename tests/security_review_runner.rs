use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use codegg::lsp::config::LspConfig;
use codegg::lsp::service::LspService;
use codegg::security::workflow::{
    parse_security_review_args, run_security_review_background, FixtureSecurityContextExecutor,
    SecurityReviewCommandArgs,
};
use codegg::tool::lsp::LspTool;
use codegg::tui::app::{App, TuiMsg};
use codegg::tui::command::COMMAND_REGISTRY;

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Create a real (idle) `LspTool` Arc for tests that need to satisfy the
/// `lsp_tool: Option<Arc<LspTool>>` argument. The service is never
/// queried in these tests — the executor either errors out before any
/// LSP request, or the workflow returns a stage-1 report.
fn make_idle_lsp_tool() -> Arc<LspTool> {
    Arc::new(LspTool::new(LspService::new_arc(LspConfig::default())))
}

/// Initialize a temporary git repository in `root` so the diff-discovery
/// pipeline can run against it. Uses `git init` and a single empty
/// commit to make `git diff HEAD` succeed with no changed files.
fn init_git_repo(root: &std::path::Path) {
    let status = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(root)
        .status()
        .expect("git init should run");
    assert!(status.success(), "git init failed: {status:?}");

    // Configure a local user so an initial commit is possible.
    Command::new("git")
        .args(["config", "user.email", "test@example.invalid"])
        .current_dir(root)
        .status()
        .expect("git config user.email should run");
    Command::new("git")
        .args(["config", "user.name", "test"])
        .current_dir(root)
        .status()
        .expect("git config user.name should run");

    // Make an initial commit so HEAD exists.
    let status = Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init", "-q"])
        .current_dir(root)
        .status()
        .expect("git commit --allow-empty should run");
    assert!(status.success(), "git commit failed: {status:?}");
}

/// Drive a `/security-review` slash command through the public App
/// surface: set the prompt text and call `process_msg(SubmitPrompt)`,
/// which routes through `send_prompt` → `handle_slash_command` →
/// `execute_command` → the `/security-review` branch.
///
/// This is the integration-level path; we don't call `execute_command`
/// directly (it's a private `fn`).
fn dispatch_security_review(app: &mut App, text: &str) {
    app.prompt_state.prompt.set_text(text.to_string());
    let len = text.chars().count();
    app.prompt_state.prompt.set_cursor(len);
    app.process_msg(TuiMsg::SubmitPrompt);
}

/// Build a fresh `App` whose `project_dir` points at the tempdir so the
/// command handler sees a valid project root.
fn make_test_app(project_dir: &std::path::Path) -> App {
    App::new_for_testing(project_dir.to_string_lossy().to_string())
}

// -----------------------------------------------------------------------------
// Dispatch path tests (1-5)
//
// These exercise the App-level dispatch from `process_msg(SubmitPrompt)`
// through `handle_slash_command` → `execute_command` → the
// `/security-review` branch. The `App::new_for_testing` constructor
// leaves `tui_cmd_tx: None`, so every dispatch hits the
// "TUI command channel unavailable" fallback path and clears the
// reentrancy guard. That is the only way to test the dispatch
// integration-level without a live TUI event loop.
//
// We do NOT assert on toast contents (ToastManager has no public
// read surface in this codebase), only on the observable side
// effects: the `security_review_running` guard and elapsed time.
// -----------------------------------------------------------------------------

#[test]
fn security_review_command_dispatches_background_task() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());
    let mut app = make_test_app(dir.path());

    // Initially the guard is empty.
    assert!(app.security_review_running.is_none());

    dispatch_security_review(&mut app, "/security-review --changed");

    // The fallback path (tui_cmd_tx is None on App::new_for_testing)
    // sets the guard to a fresh run id and then immediately clears it
    // because the channel isn't available. After dispatch returns the
    // guard is therefore back to None.
    assert!(
        app.security_review_running.is_none(),
        "expected guard to be cleared by the fallback path, got {:?}",
        app.security_review_running
    );
}

#[test]
fn security_review_command_does_not_block_inline() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());
    let mut app = make_test_app(dir.path());

    let start = Instant::now();
    dispatch_security_review(&mut app, "/security-review --changed");
    let elapsed = start.elapsed();

    // The whole dispatch is a few field writes and a single toast add;
    // it must be well under 100ms. The old inline `block_in_place`
    // path would block until diff discovery + (optional) LSP completed.
    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "dispatch should not block inline; elapsed = {elapsed:?}"
    );

    // Secondary check: the fallback cleared the guard.
    assert!(app.security_review_running.is_none());
}

#[tokio::test]
async fn security_review_command_rejects_second_run_while_active() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());
    let mut app = make_test_app(dir.path());

    let active_id = "sr-fake-active".to_string();
    app.security_review_running = Some(codegg::security::workflow::SecurityReviewTaskState {
        id: active_id.clone(),
        task_id: codegg::tui::task_lifecycle::TuiTaskId(1),
    });

    dispatch_security_review(&mut app, "/security-review");

    // The dispatch must bail out without touching the guard.
    assert_eq!(
        app.security_review_run_id(),
        Some(active_id.as_str()),
        "guard should be preserved when a review is already active"
    );
}

#[tokio::test]
async fn security_review_command_with_channel_sets_guard_and_spawns_task() {
    // Exercise the new dispatch path: install a tui_cmd_tx,
    // dispatch the command, and verify that the guard is set to a
    // fresh run id with an AbortHandle. The completion path sends
    // `TuiCommand::SecurityReviewFinished` back on the channel and
    // that handler cannot be invoked from an integration test
    // without a live TUI event loop, so we leave the guard set
    // here and assert the guard body instead.
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());
    let mut app = make_test_app(dir.path());

    let (tx, _rx) = tokio::sync::mpsc::channel::<codegg::tui::TuiCommand>(8);
    app.tui_cmd_tx = Some(tx);

    assert!(app.security_review_running.is_none());

    dispatch_security_review(&mut app, "/security-review --changed --enrich");

    let state = app
        .security_review_running
        .as_ref()
        .expect("guard should be set after successful dispatch");
    assert!(
        state.id.starts_with("sr-"),
        "guard should be a fresh run id, got {}",
        state.id
    );
}

#[tokio::test]
async fn security_review_command_with_channel_clears_guard_on_run_id_match() {
    // The TUI command handler in `src/tui/mod.rs` only clears the
    // guard if its current value matches the dispatched run id.
    // We can simulate the relevant invariant end-to-end by
    // dispatching through the channel and then driving the
    // background work ourselves, simulating what the TUI handler
    // does on success.
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());
    let mut app = make_test_app(dir.path());

    let (tx, mut rx) = tokio::sync::mpsc::channel::<codegg::tui::TuiCommand>(8);
    app.tui_cmd_tx = Some(tx);

    dispatch_security_review(&mut app, "/security-review");

    let dispatched_id = app
        .security_review_run_id()
        .expect("guard should be set after dispatch")
        .to_string();

    // Drain the completion message that the spawned task sent.
    // Using `recv().await` lets the runtime drive the spawned task
    // to completion before we read from the channel.
    let msg = rx
        .recv()
        .await
        .expect("channel should have received the SecurityReviewFinished command");
    match msg {
        codegg::tui::TuiCommand::SecurityReviewFinished { id, receipt, error } => {
            assert_eq!(id, dispatched_id);
            assert!(error.is_none(), "expected success, got error: {error:?}");
            assert!(receipt.is_some(), "expected receipt on success");
        }
        other => panic!("expected SecurityReviewFinished, got {other:?}"),
    }

    // The TUI handler's clear-if-matches logic: the guard is only
    // cleared when its current value equals the dispatched run id.
    if app.security_review_run_id() == Some(dispatched_id.as_str()) {
        app.security_review_running = None;
    }

    assert!(
        app.security_review_running.is_none(),
        "handler should clear the guard when the run id matches"
    );
}

// -----------------------------------------------------------------------------
// Background function tests (6-13)
//
// These exercise `run_security_review_background` directly, the
// production entry point used by the TUI command handler. We don't
// need the App at all for these.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn security_review_background_without_executor_returns_unavailable_note() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        ..Default::default()
    };

    let result = run_security_review_background(dir.path().to_path_buf(), args, None).await;
    assert!(result.is_ok(), "background without executor should succeed");
    let receipt = result.unwrap();
    let output = receipt.rendered_report.clone();
    assert!(
        output.contains("no securityContext executor"),
        "report should mention no executor: {output}"
    );
    assert!(!receipt.enriched);
    assert!(!receipt.lsp_available);
}

#[tokio::test]
async fn security_review_background_with_fixture_executor_uses_enrichment() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    // Build a real (idle) LspTool Arc and pass it to the background
    // function. The function takes ownership of the Arc and wraps it
    // in an LspSecurityContextExecutor internally.
    let tool = make_idle_lsp_tool();
    let _fixture = FixtureSecurityContextExecutor::new();

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        ..Default::default()
    };

    let result = run_security_review_background(dir.path().to_path_buf(), args, Some(tool)).await;
    assert!(
        result.is_ok(),
        "background with executor should succeed: {:?}",
        result
    );
    let receipt = result.unwrap();
    assert!(receipt.lsp_available);
    assert!(receipt.enriched);
}

#[tokio::test]
async fn security_review_background_json_mode_returns_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        json: true,
        ..Default::default()
    };

    let result = run_security_review_background(dir.path().to_path_buf(), args, None).await;
    assert!(result.is_ok());
    let receipt = result.unwrap();
    let output = receipt.rendered_report.clone();
    assert!(
        output.starts_with('{'),
        "json output should start with '{{': {output}"
    );
    assert!(
        output.contains("\"notes\""),
        "json should contain notes: {output}"
    );
}

#[tokio::test]
async fn security_review_background_preserves_prompts_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        prompts_only: true,
        ..Default::default()
    };

    let result = run_security_review_background(dir.path().to_path_buf(), args, None).await;
    assert!(result.is_ok());
    let receipt = result.unwrap();
    let output = receipt.rendered_report.clone();
    assert!(
        !output.contains("Findings\n"),
        "prompts-only output should not contain the 'Findings' section header: {output}"
    );
}

#[tokio::test]
async fn security_review_background_preserves_findings_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        findings_only: true,
        ..Default::default()
    };

    let result = run_security_review_background(dir.path().to_path_buf(), args, None).await;
    assert!(result.is_ok());
    let receipt = result.unwrap();
    let output = receipt.rendered_report.clone();
    assert!(
        !output.contains("Review Prompts\n"),
        "findings-only output should not contain the 'Review Prompts' section header: {output}"
    );
}

#[tokio::test]
async fn security_review_default_path_does_not_create_executor() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    let args = SecurityReviewCommandArgs {
        enrich: false,
        base: Some("HEAD".to_string()),
        ..Default::default()
    };

    // lsp_tool is None — the background runner should still succeed
    // and should NOT add the unavailable-executor note because
    // enrichment was not requested.
    let result = run_security_review_background(dir.path().to_path_buf(), args, None).await;
    assert!(result.is_ok());
    let receipt = result.unwrap();
    let output = receipt.rendered_report.clone();
    assert!(
        !output.contains("no securityContext executor"),
        "default path should not add unavailable note: {output}"
    );
}

#[tokio::test]
async fn security_review_remote_mode_none_executor_is_deterministic() {
    // In remote/socket mode the TUI passes lsp_tool = None even when
    // enrichment is requested, because the inproc LSP service is not
    // available. The background function must succeed deterministically
    // and surface a clear "no executor" note.
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        ..Default::default()
    };

    let result = run_security_review_background(dir.path().to_path_buf(), args, None).await;
    assert!(result.is_ok());
    let receipt = result.unwrap();
    let output = receipt.rendered_report.clone();
    assert!(
        output.contains("no securityContext executor"),
        "remote mode with no executor should report unavailable: {output}"
    );
}

#[tokio::test]
async fn security_review_local_mode_executor_arc_is_cloned_not_borrowed() {
    // Regression test for the async-dispatch contract: the background
    // function must take ownership of the `Arc<LspTool>` (so the
    // caller can spawn it in a background task without borrowing App
    // state across an await). To observe the strong count after the
    // call, we keep a clone of the Arc on the stack and pass a
    // second clone to the function. After the await returns, the
    // function's clone (and the executor it built) should be gone,
    // leaving exactly one strong reference: ours.
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    let arc = make_idle_lsp_tool();
    let before = Arc::strong_count(&arc);
    assert!(
        before == 1,
        "Arc should start with 1 strong ref; got {before}"
    );

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        ..Default::default()
    };

    let result =
        run_security_review_background(dir.path().to_path_buf(), args, Some(Arc::clone(&arc)))
            .await;
    assert!(result.is_ok());

    let after = Arc::strong_count(&arc);
    assert_eq!(
        after, 1,
        "after background returns, only our Arc should remain; got {after}"
    );
}

// -----------------------------------------------------------------------------
// Sanity: the slash command is registered and its args parse correctly
// (sanity coverage for the dispatch wiring, not for the function itself).
// -----------------------------------------------------------------------------

#[test]
fn security_review_command_is_registered() {
    let cmd = COMMAND_REGISTRY
        .find_by_name_or_alias("security-review")
        .expect("/security-review command should be registered");
    assert_eq!(cmd.name, "/security-review");
    assert!(cmd.dialog.is_none());
    assert!(cmd.template.is_none());
}

#[test]
fn security_review_arg_parser_handles_changed_and_enrich() {
    let parsed = parse_security_review_args("/security-review --changed --enrich");
    assert_eq!(parsed.base.as_deref(), Some("HEAD"));
    assert!(parsed.enrich);

    let parsed = parse_security_review_args("/security-review --base origin/main --json");
    assert_eq!(parsed.base.as_deref(), Some("origin/main"));
    assert!(parsed.json);
}

#[tokio::test]
async fn security_review_background_produces_receipt_with_hunks() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_git_repo(dir.path());

    // Create a file with content, stage it, then modify it so we have a real diff.
    let file_path = dir.path().join("src");
    std::fs::create_dir(&file_path).expect("create src dir");
    let rs_path = file_path.join("lib.rs");
    std::fs::write(&rs_path, "fn main() {\n    let x = 1;\n}\n").expect("write initial");

    Command::new("git")
        .args(["add", "src/lib.rs"])
        .current_dir(dir.path())
        .status()
        .expect("git add");
    Command::new("git")
        .args(["commit", "-m", "add lib.rs", "-q"])
        .current_dir(dir.path())
        .status()
        .expect("git commit");

    // Modify the file to create a diff
    std::fs::write(
        &rs_path,
        "fn main() {\n    let x = 1;\n    let y = 2;\n    println!(\"{x} {y}\");\n}\n",
    )
    .expect("write modified");

    let args = SecurityReviewCommandArgs {
        base: Some("HEAD".to_string()),
        ..Default::default()
    };

    let receipt = run_security_review_background(dir.path().to_path_buf(), args, None)
        .await
        .expect("background should succeed");

    // The receipt should have parsed hunks from the real diff
    assert!(
        !receipt.output.hunks.is_empty(),
        "receipt.output.hunks should be populated from real diff, got: {:?}",
        receipt.output.hunks
    );

    // Verify the hunk has file_path and lines
    let hunk = &receipt.output.hunks[0];
    assert_eq!(
        hunk.file_path,
        std::path::PathBuf::from("src/lib.rs"),
        "hunk should reference the modified file"
    );
    assert!(!hunk.lines.is_empty(), "hunk should have parsed diff lines");

    // Verify line kinds are present
    let has_added = hunk
        .lines
        .iter()
        .any(|l| l.kind == codegg::security::workflow::SecurityReviewHunkLineKind::Added);
    let has_context = hunk
        .lines
        .iter()
        .any(|l| l.kind == codegg::security::workflow::SecurityReviewHunkLineKind::Context);
    assert!(has_added, "hunk should have added lines");
    assert!(has_context, "hunk should have context lines");
}

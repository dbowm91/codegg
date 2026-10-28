//! C001 — a TUI command must not enter the runtime from inside it.
//!
//! The TUI event loop is awaited inside `#[tokio::main(flavor =
//! "multi_thread")]` (`src/main.rs`), so the whole loop runs with the
//! runtime context **entered**. A synchronous command handler that then
//! called `Handle::block_on` panicked with *"Cannot start a runtime from
//! within a runtime"* and took the whole process down.
//!
//! These tests drive the command path the way production does — through
//! `App::on_key`, inside a future that a real `Runtime::block_on` polls —
//! because the defect lives in the *composition* of the entry path and the
//! handler. Entering the runtime with `Runtime::enter()` alone would not
//! reproduce it: `enter()` sets the current *handle* but not the entered
//! flag, so a nested `block_on` succeeds there and panics in production.
//! That is why the driving future below is polled by `block_on` and not by
//! `enter()`.
//!
//! Commands that perform async work are asserted to enqueue their typed
//! effect on the same bounded channel consumed by the production event loop.

use std::sync::Arc;

use codegg::config::schema::LspConfig;
use codegg::lsp::config_lsp_to_egglsp;
use codegg::lsp::service::LspService;
use codegg::tool::lsp::LspTool;
use codegg::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn sample_lsp_tool() -> Arc<LspTool> {
    let service = LspService::new_arc(config_lsp_to_egglsp(LspConfig::default()));
    Arc::new(LspTool::new(service))
}

/// Type `text` one key at a time, the way a terminal delivers it.
fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
}

fn press_enter(app: &mut App) {
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

fn toast_messages(app: &App) -> Vec<String> {
    app.messages_state
        .toasts
        .iter()
        .map(|toast| toast.message.clone())
        .collect()
}

fn assert_lsp_servers_request_queued(app: &mut App) {
    let request = app
        .tui_cmd_rx
        .as_mut()
        .expect("test should install the production TUI command channel")
        .try_recv()
        .expect("LSP servers request should be queued");
    assert!(matches!(
        request,
        codegg::tui::app::TuiCommand::LspServersRequested
    ));
    assert!(
        toast_messages(app)
            .iter()
            .all(|message| message != "Internal error: command channel unavailable"),
        "the command should not report that its production channel is missing"
    );
}

/// `/lsp-servers` schedules its LSP request from inside the event loop's
/// runtime without falling into the test constructor's absent-channel path.
#[test]
fn lsp_servers_command_queues_inside_the_event_loop_runtime() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.lsp_tool = Some(sample_lsp_tool());
        app.ensure_tui_cmd_channel();

        type_text(&mut app, "/lsp-servers");
        press_enter(&mut app);
        assert_lsp_servers_request_queued(&mut app);

        assert!(!app.ui_state.command_mode);
    });
}

/// The same command with an argument: the argument is read from the command
/// palette's query, so the whole dispatch path is exercised.
///
/// The argument deliberately does **not** start with `/`. The completion
/// popup's filter is the text from the last `/`, so an absolute argument
/// would retarget the filter at the path and the popup would resolve the
/// command from its own best fuzzy row — a different command. An argument
/// with a space in it keeps the filter on the whole query, which no command
/// name can match, so Enter falls back to the leading token, exactly as
/// `handle_command_key` documents.
#[test]
fn lsp_doctor_command_with_an_argument_completes_inside_the_event_loop_runtime() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.lsp_tool = Some(sample_lsp_tool());

        type_text(&mut app, "/lsp-doctor src/main.rs");
        press_enter(&mut app);

        let messages = toast_messages(&app);
        assert!(
            messages.iter().any(|m| m.contains("LSP Doctor")),
            "/lsp-doctor must report the doctor report, got: {messages:?}"
        );
    });
}

/// `/worktree` is the other command that waited on a runtime handle. Its
/// result depends on the workspace, so the assertion is on the surface: the
/// command reports a message and the session is still usable afterwards.
#[test]
fn worktree_command_completes_inside_the_event_loop_runtime() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let workspace = std::env::temp_dir().join("codegg-c001-worktree-probe");
        let mut app = App::new_for_testing(workspace.to_string_lossy().to_string());

        type_text(&mut app, "/worktree");
        press_enter(&mut app);

        let messages = toast_messages(&app);
        assert!(
            messages.iter().any(|m| m == "No worktrees found"),
            "/worktree must report the string it always reported for a workspace outside a repository, got: {messages:?}"
        );
        assert!(
            !app.ui_state.command_mode,
            "the command must leave command mode, got: {messages:?}"
        );
    });
}

/// An async command must leave the session usable: the composer still accepts
/// text and the buffer is intact after the request is queued.
#[test]
fn lsp_command_queueing_leaves_the_session_usable() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut app = App::new_for_testing("/tmp".to_string());
        app.lsp_tool = Some(sample_lsp_tool());
        app.ensure_tui_cmd_channel();

        type_text(&mut app, "/lsp-servers");
        press_enter(&mut app);
        assert_lsp_servers_request_queued(&mut app);

        type_text(&mut app, "still typing");
        assert_eq!(
            app.prompt_state.prompt.get_text(),
            "still typing",
            "the composer must still accept input after a command that waited on the runtime"
        );
        assert!(
            app.ui_state.running,
            "the session must still be running after a command that waited on the runtime"
        );
    });
}

//! # Terminal User Interface (TUI)
//!
//! This module provides the terminal-based UI for codegg, built with [ratatui].
//!
//! ## Architecture Overview
//!
//! The TUI is structured around several key components:
//!
//! - [`App`]: Main application state and event handling
//! - [`components`]: Reusable UI widgets (messages, prompt, sidebar, etc.)
//! - [`input`]: Keyboard event handling and keybindings
//! - [`layout`]: Layout management and area calculations
//! - [`theme`]: Color themes and styling
//! - [`route`]: Route/state machine management
//! - [`command`]: Slash command registry
//!
//! ## State Management
//!
//! The [`App`] struct is organized into several state domains:
//!
//! - [`UiState`](app::UiState): UI state (theme, layout, dialogs, routes)
//! - [`SessionState`](app::SessionState): Session management
//! - [`PromptState`](app::PromptState): Prompt input state
//! - [`MessagesState`](app::MessagesState): Message history and display
//! - [`DialogState`](app::DialogState): Dialog visibility and data
//! - [`AgentState`](app::AgentState): Agent and model configuration
//!
//! ### State Flow
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                      App                                │
//! │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐  │
//! │  │ UiState  │ │ Session  │ │ Prompt   │ │ Dialog    │  │
//! │  │ - theme  │ │ - session│ │ - prompt │ │ - dialogs │  │
//! │  │ - layout │ │ - store  │ │ - compl. │ │ - state   │  │
//! │  └──────────┘ └──────────┘ └──────────┘ └───────────┘  │
//! └─────────────────────────────────────────────────────────┘
//!                         │
//!                         ▼
//!              ┌─────────────────────┐
//!              │  Event Handling     │
//!              │  on_key()           │
//!              │  on_mouse()         │
//!              └─────────────────────┘
//!                         │
//!                         ▼
//!              ┌─────────────────────┐
//!              │      Render         │
//!              │  render()          │
//!              │  render_dialog()    │
//!              └─────────────────────┘
//! ```
//!
//! ## Rendering
//!
//! Rendering uses ratatui's widget model. The main render loop in
//! [`run_event_loop`] handles:
//! - Panic recovery via [`catch_unwind`](std::panic::catch_unwind)
//! - Error boundary display via [`render_error`](app::App::render_error)
//! - Terminal resize handling
//!
//! ### Render Order
//!
//! 1. **Header**: Agent name, model, session info
//! 2. **Viewport**: Messages (Home or Session view)
//! 3. **Prompt**: Input area with status indicator
//! 4. **Footer**: Token counts, session status
//! 5. **Sidebar**: Optional session/agent info panel
//! 6. **Dialog**: Modal overlay (if open)
//! 7. **Completions**: Slash/file completion popup (if active)
//! 8. **Toasts**: Notification messages
//!
//! ## Event Flow
//!
//! ```text
//! Terminal Input ──► EventStream ──► on_key() / on_mouse()
//!                                       │
//!                                       ▼
//!                              ┌─────────────────┐
//!                              │ Route to State  │
//!                              │ - dialog_key    │
//!                              │ - prompt_key    │
//!                              │ - binding_action│
//!                              └─────────────────┘
//!                                       │
//!                                       ▼
//!                              ┌─────────────────┐
//!                              │ State Mutation  │
//!                              │ - update state  │
//!                              │ - open/close    │
//!                              └─────────────────┘
//! ```
//!
//! 1. Terminal events are captured via crossterm's [`EventStream`]
//! 2. Events are routed to [`App::on_key`](app::App::on_key) or [`App::on_mouse`](app::App::on_mouse)
//! 3. Key events are matched against bindings, routed to dialog/prompt handlers
//! 4. State changes trigger re-renders via [`App::render`](app::App::render)
//!
//! ## Error Handling
//!
//! Render errors are caught and displayed gracefully without crashing the application.
//! The event loop uses `catch_unwind` to recover from rendering panics.

pub mod app;
pub mod async_cmd;
pub mod command;
pub mod commands;
pub mod components;
pub mod file_diff;
pub mod input;
pub mod layout;
pub mod route;
pub(crate) mod runtime;
pub mod task_lifecycle;
pub mod terminal;
pub mod theme;

pub use app::{App, Dialog, SessionMutationOp, TuiCommand};
pub use input::InputAction;
pub use route::Route;
pub use terminal::{create_terminal, AppTerminal};
pub use theme::Theme;

pub use runtime::event_loop::run_event_loop;

#[cfg(test)]
mod shell_dispatch_tests {
    use crate::shell::types::{
        ShellCapturePolicy, ShellCommandId, ShellEnvPolicy, ShellOrigin, ShellRequest,
    };
    use crate::tui::app::App;
    use crate::tui::commands::shell::{
        handle_shell_ask, handle_shell_include, handle_shell_kill, handle_shell_list,
        handle_shell_show,
    };
    use crate::tui::components::messages::MessageRole;
    use std::time::Duration;

    fn make_test_app() -> App {
        App::new_for_testing("/tmp".into())
    }

    fn insert_completed_entry(
        app: &mut App,
        id: u64,
        command: &str,
        stdout: &[u8],
        stderr: &[u8],
        exit_code: Option<i32>,
    ) {
        let cmd_id = ShellCommandId(id);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: command.to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);
        app.shell_store.append_stdout(cmd_id, stdout);
        app.shell_store.append_stderr(cmd_id, stderr);
        let exit = exit_code.unwrap_or(0);
        app.shell_store
            .mark_exited(cmd_id, Some(exit), Duration::from_secs(1));
    }

    fn get_toasts(app: &App) -> Vec<String> {
        app.messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect()
    }

    fn get_user_messages(app: &App) -> Vec<String> {
        app.messages_state
            .messages
            .messages
            .iter()
            .filter(|m| m.role == MessageRole::User)
            .map(|m| m.text_content())
            .collect()
    }

    #[test]
    fn shell_list_empty_shows_toast() {
        let mut app = make_test_app();
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("No shell commands")),
            "should show empty message, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_list_with_entries_shows_recent() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "echo hello", b"hello\n", b"", Some(0));
        insert_completed_entry(&mut app, 2, "cargo test", b"", b"fail\n", Some(1));
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        let text = toasts.join("\n");
        assert!(
            text.contains("echo hello"),
            "should list command, got: {text}"
        );
        assert!(
            text.contains("cargo test"),
            "should list command, got: {text}"
        );
    }

    #[test]
    fn shell_include_unknown_id_shows_warning() {
        let mut app = make_test_app();
        handle_shell_include(&mut app, 999, "all".to_string(), None);
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("not found")),
            "should show not-found warning, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_include_full_mode_promotes_output() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "echo hello", b"hello\n", b"", Some(0));
        handle_shell_include(&mut app, 1, "all".to_string(), None);
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("echo hello")),
            "should include command in promoted message, got: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("hello")),
            "should include stdout in promoted message, got: {msgs:?}"
        );
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("included")),
            "should show success toast, got: {toasts:?}"
        );
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert!(entry.promoted, "entry should be marked as promoted");
    }

    #[test]
    fn shell_include_stdout_mode() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo check",
            b"checking...\n",
            b"warning: unused\n",
            Some(0),
        );
        handle_shell_include(&mut app, 1, "stdout".to_string(), None);
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("checking...")),
            "should include stdout, got: {msgs:?}"
        );
    }

    #[test]
    fn shell_include_stderr_mode() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo check",
            b"checking...\n",
            b"error[E0308]: mismatched\n",
            Some(1),
        );
        handle_shell_include(&mut app, 1, "stderr".to_string(), None);
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("error[E0308]")),
            "should include stderr, got: {msgs:?}"
        );
    }

    #[test]
    fn shell_include_tail_mode() {
        let mut app = make_test_app();
        let big_stderr = (0..500).map(|i| format!("line {i}\n")).collect::<String>();
        insert_completed_entry(
            &mut app,
            1,
            "big output",
            b"",
            big_stderr.as_bytes(),
            Some(1),
        );
        handle_shell_include(&mut app, 1, "tail 5".to_string(), None);
        let msgs = get_user_messages(&app);
        let included = msgs.iter().find(|m| m.contains("tail 5")).unwrap();
        assert!(
            included.contains("line 499"),
            "tail should include last lines, got: {included}"
        );
        assert!(
            !included.contains("line 0"),
            "tail should not include first lines"
        );
    }

    #[test]
    fn shell_ask_unknown_id_shows_warning() {
        let mut app = make_test_app();
        handle_shell_ask(&mut app, 999, "why did this fail?".to_string());
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("not found")),
            "should show not-found warning, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_ask_includes_question_and_output() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo test",
            b"",
            b"test result: FAILED\n",
            Some(101),
        );
        handle_shell_ask(&mut app, 1, "why did this fail?".to_string());
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("why did this fail?")),
            "should include question, got: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("cargo test")),
            "should include command, got: {msgs:?}"
        );
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert!(entry.promoted, "entry should be marked as promoted");
    }

    #[test]
    fn shell_kill_nonexistent_shows_error() {
        let mut app = make_test_app();
        handle_shell_kill(&mut app, 999);
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("No running")),
            "should show error for unknown id, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_kill_running_command() {
        let mut app = make_test_app();
        let cmd_id = ShellCommandId(1);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: "sleep 999".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let abort_handle = rt.block_on(async { tokio::spawn(async {}).abort_handle() });
        let handle = crate::shell::runtime::ShellHandle::new_for_test(cmd_id, abort_handle);
        app.shell_handles.insert(1, handle);

        handle_shell_kill(&mut app, 1);
        assert!(
            !app.shell_handles.contains_key(&1),
            "handle should be removed"
        );
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("Killed")),
            "should show kill confirmation, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_include_promotes_only_once() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "echo test", b"test\n", b"", Some(0));
        handle_shell_include(&mut app, 1, "all".to_string(), None);
        handle_shell_include(&mut app, 1, "all".to_string(), None);
        let msgs = get_user_messages(&app);
        let include_count = msgs.iter().filter(|m| m.contains("echo test")).count();
        assert_eq!(
            include_count, 2,
            "each /shell-include creates a new message"
        );
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert!(entry.promoted, "entry should remain promoted");
    }

    #[test]
    fn shell_list_shows_status_labels() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "cmd1", b"", b"", Some(0));
        let cmd_id = ShellCommandId(2);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: "cmd2".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        let text = toasts.join("\n");
        assert!(
            text.contains("done"),
            "should show done status, got: {text}"
        );
        assert!(
            text.contains("running"),
            "should show running status, got: {text}"
        );
    }

    #[test]
    fn shell_list_shows_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "passing", b"", b"", Some(0));
        insert_completed_entry(&mut app, 2, "failing", b"", b"err\n", Some(101));
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        let text = toasts.join("\n");
        assert!(
            text.contains("exit=0"),
            "should show exit=0 for passing cmd, got: {text}"
        );
        assert!(
            text.contains("exit=101"),
            "should show exit=101 for failing cmd, got: {text}"
        );
    }

    #[test]
    fn shell_include_preserves_nonzero_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo test",
            b"",
            b"test result: FAILED\n",
            Some(101),
        );
        handle_shell_include(&mut app, 1, "summary".to_string(), None);
        let msgs = get_user_messages(&app);
        let included = msgs.iter().find(|m| m.contains("cargo test")).unwrap();
        assert!(
            included.contains("Exit code: 101"),
            "should show actual exit code 101, got: {included}"
        );
    }

    #[test]
    fn shell_ask_preserves_nonzero_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo check",
            b"",
            b"error[E0308]: mismatched\n",
            Some(1),
        );
        handle_shell_ask(&mut app, 1, "fix this error".to_string());
        let msgs = get_user_messages(&app);
        let included = msgs.iter().find(|m| m.contains("fix this error")).unwrap();
        assert!(
            included.contains("Exit code: 1"),
            "should show actual exit code 1, got: {included}"
        );
    }

    #[test]
    fn shell_kill_marks_entry_exited() {
        let mut app = make_test_app();
        let cmd_id = ShellCommandId(1);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: "sleep 999".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let abort_handle = rt.block_on(async { tokio::spawn(async {}).abort_handle() });
        let handle = crate::shell::runtime::ShellHandle::new_for_test(cmd_id, abort_handle);
        app.shell_handles.insert(1, handle);

        handle_shell_kill(&mut app, 1);
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert_eq!(
            entry.status,
            crate::shell::types::ShellStatus::Killed,
            "killed entry should be marked as killed"
        );
        assert_eq!(
            entry.exit_code, None,
            "killed entry should have no exit code"
        );
    }

    #[test]
    fn shell_show_unknown_id_shows_warning() {
        let mut app = make_test_app();
        handle_shell_show(&mut app, 999);
        let toasts = get_toasts(&app);
        assert!(
            toasts
                .iter()
                .any(|t| t.contains("No shell command with id 999")),
            "should show not-found warning, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_show_opens_dialog_with_metadata() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo test",
            b"running 1 test\nok\n",
            b"warning: unused\n",
            Some(0),
        );
        handle_shell_show(&mut app, 1);
        assert_eq!(
            app.ui_state.dialog,
            crate::tui::Dialog::ShellShow,
            "dialog should be set to ShellShow"
        );
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let content = dialog.content_lines();
        let text = content.join("\n");
        assert!(
            text.contains("cargo test"),
            "should show command, got: {text}"
        );
        assert!(
            text.contains("Exit:     0"),
            "should show exit code, got: {text}"
        );
        assert!(text.contains("exited"), "should show status, got: {text}");
        assert!(
            text.contains("running 1 test"),
            "should show stdout, got: {text}"
        );
        assert!(
            text.contains("warning: unused"),
            "should show stderr, got: {text}"
        );
    }

    #[test]
    fn shell_show_nonzero_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 2, "cargo check", b"", b"error[E0308]\n", Some(1));
        handle_shell_show(&mut app, 2);
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let text = dialog.content_lines().join("\n");
        assert!(
            text.contains("Exit:     1"),
            "should show exit code 1, got: {text}"
        );
    }

    #[test]
    fn shell_show_running_command() {
        let mut app = make_test_app();
        let cmd_id = ShellCommandId(3);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: "sleep 999".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);
        handle_shell_show(&mut app, 3);
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let text = dialog.content_lines().join("\n");
        assert!(
            text.contains("running"),
            "should show running status, got: {text}"
        );
        assert!(
            text.contains("sleep 999"),
            "should show command, got: {text}"
        );
    }

    #[test]
    fn shell_show_empty_output() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 4, "true", b"", b"", Some(0));
        handle_shell_show(&mut app, 4);
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let text = dialog.content_lines().join("\n");
        assert!(
            text.contains("no output captured"),
            "should show no-output message, got: {text}"
        );
    }
}

#[cfg(test)]
mod async_cmd_tests {
    use super::*;
    use crate::tui::app::App;
    use crate::tui::commands::diagnostics::apply_doctor_result;
    use crate::tui::commands::import::apply_import_preview_loaded;
    use crate::tui::commands::memory::apply_memory_result;
    use crate::tui::commands::research::apply_research_run_loaded;
    use crate::tui::commands::sessions::{
        apply_session_messages_loaded, apply_session_mutation_finished, apply_sessions_reloaded,
        apply_template_session_created,
    };
    use crate::tui::commands::tasks::{
        apply_task_operation_finished, apply_tasks_listed, apply_worktree_listed,
    };
    use std::collections::HashMap;

    fn make_test_app() -> App {
        App::new_for_testing("/tmp".into())
    }

    fn test_session() -> crate::session::Session {
        crate::session::Session {
            id: "test-session-1".into(),
            project_id: "/tmp".into(),
            workspace_id: None,
            parent_id: None,
            slug: "test".into(),
            directory: "/tmp".into(),
            title: "Test Session".into(),
            version: "1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: Vec::new(),
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
            time_deleted: None,
        }
    }

    #[test]
    fn apply_sessions_reloaded_with_error_shows_toast() {
        let mut app = make_test_app();
        let request_id = app.dialog_state.session_reload_request.begin();
        apply_sessions_reloaded(
            &mut app,
            request_id,
            Vec::new(),
            HashMap::new(),
            Some("test error".into()),
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("test error")),
            "should show error toast, got: {toasts:?}"
        );
    }

    #[test]
    fn apply_sessions_reloaded_clears_loading() {
        let mut app = make_test_app();
        let request_id = app.dialog_state.session_reload_request.begin();
        apply_sessions_reloaded(&mut app, request_id, Vec::new(), HashMap::new(), None);
        assert!(!app.dialog_state.session_reload_request.is_loading());
    }

    #[test]
    fn apply_session_messages_loaded_with_error_preserves_old_messages() {
        let mut app = make_test_app();
        let request_id = app.dialog_state.session_messages_request.begin();
        app.messages_state
            .messages
            .add_user_message("old message".to_string(), None);
        apply_session_messages_loaded(
            &mut app,
            request_id,
            "session-1".into(),
            Vec::new(),
            Some("load failed".into()),
        );
        assert_eq!(
            app.messages_state.messages.message_count(),
            1,
            "old messages should be preserved on error"
        );
    }

    #[test]
    fn apply_memory_result_shows_info_toast() {
        let mut app = make_test_app();
        apply_memory_result(&mut app, "operation succeeded".to_string(), false);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t.contains("operation succeeded")));
    }

    #[test]
    fn apply_memory_result_error_shows_error_toast() {
        let mut app = make_test_app();
        apply_memory_result(&mut app, "something failed".to_string(), true);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t.contains("something failed")));
    }

    #[test]
    fn apply_doctor_result_shows_summary() {
        let mut app = make_test_app();
        apply_doctor_result(&mut app, "doctor: OK (mcp, provider)".to_string(), false);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t.contains("doctor: OK")));
    }

    #[test]
    fn import_stale_preview_is_ignored() {
        let mut app = make_test_app();
        app.dialog_state.import_dialog =
            Some(crate::tui::components::dialogs::import::ImportDialog::default());

        // Start preview A
        let id_a = app.dialog_state.import_request.begin();
        // Start preview B (supersedes A)
        let id_b = app.dialog_state.import_request.begin();

        // Apply A's result -- should be ignored (stale)
        apply_import_preview_loaded(&mut app, id_a, Some(test_session()), 10, None);
        // Import dialog's preview should still be None (A was ignored, B hasn't arrived)
        let import = app.dialog_state.import_dialog.as_ref().unwrap();
        assert!(
            import.preview_session.is_none(),
            "preview A should be ignored, preview_session should still be None"
        );

        // Apply B's result -- should succeed
        apply_import_preview_loaded(&mut app, id_b, Some(test_session()), 5, None);
        let import = app.dialog_state.import_dialog.as_ref().unwrap();
        assert!(
            import.preview_session.is_some(),
            "preview B should be applied"
        );
    }

    #[test]
    fn import_cancelled_result_is_ignored() {
        let mut app = make_test_app();
        app.dialog_state.import_dialog =
            Some(crate::tui::components::dialogs::import::ImportDialog::default());

        let id = app.dialog_state.import_request.begin();
        app.dialog_state.import_request.cancel();

        // Apply result after cancel -- should be ignored
        apply_import_preview_loaded(&mut app, id, Some(test_session()), 5, None);
        let import = app.dialog_state.import_dialog.as_ref().unwrap();
        assert!(
            import.preview_session.is_none(),
            "result after cancel should be ignored"
        );
    }

    #[test]
    fn research_stale_run_is_ignored() {
        let mut app = make_test_app();
        app.dialog_state.research_browser = Some(
            crate::tui::components::dialogs::research::ResearchBrowserDialog::new(
                std::sync::Arc::new(Theme::dark()),
            ),
        );

        // Start load run A
        let id_a = app.dialog_state.research_request.begin();
        // Simulate A setting browser.loading = true
        if let Some(ref mut b) = app.dialog_state.research_browser {
            b.loading = true;
        }
        // Start load run B (supersedes A)
        let id_b = app.dialog_state.research_request.begin();

        // Apply A -- stale, should be ignored (loading stays true from B's perspective)
        apply_research_run_loaded(&mut app, id_a, "run-a".into(), None, None);
        assert!(
            app.dialog_state.research_browser.as_ref().unwrap().loading,
            "research should still be loading (A was stale)"
        );

        // Apply B -- should succeed and clear loading
        apply_research_run_loaded(&mut app, id_b, "run-b".into(), None, None);
        assert!(
            !app.dialog_state.research_browser.as_ref().unwrap().loading,
            "research should not be loading after B applied"
        );
    }

    #[test]
    fn close_dialog_cancels_import_request() {
        let mut app = make_test_app();
        app.dialog_state.import_dialog =
            Some(crate::tui::components::dialogs::import::ImportDialog::default());
        app.ui_state.dialog = Dialog::Import;

        let id = app.dialog_state.import_request.begin();
        assert!(app.dialog_state.import_request.is_loading());

        app.close_dialog();

        assert!(!app.dialog_state.import_request.is_loading());
        assert!(app.dialog_state.import_request.is_cancelled());
        // Old request ID should be stale
        assert!(!app.dialog_state.import_request.is_current(id));
    }

    #[test]
    fn close_dialog_cancels_research_request() {
        let mut app = make_test_app();
        app.dialog_state.research_browser = Some(
            crate::tui::components::dialogs::research::ResearchBrowserDialog::new(
                std::sync::Arc::new(Theme::dark()),
            ),
        );
        app.ui_state.dialog = Dialog::ResearchBrowser;

        let id = app.dialog_state.research_request.begin();
        assert!(app.dialog_state.research_request.is_loading());

        app.close_dialog();

        assert!(!app.dialog_state.research_request.is_loading());
        assert!(app.dialog_state.research_request.is_cancelled());
        assert!(!app.dialog_state.research_request.is_current(id));
    }

    #[test]
    fn session_messages_stale_result_ignored() {
        let mut app = make_test_app();
        // Begin a request for session A
        let id_a = app.dialog_state.session_messages_request.begin();

        // Simulate user switching to session B (supersedes request A)
        let id_b = app.dialog_state.session_messages_request.begin();
        app.session_state.session = Some(crate::session::Session {
            id: "session-b".into(),
            project_id: String::new(),
            workspace_id: None,
            parent_id: None,
            slug: String::new(),
            directory: String::new(),
            title: String::new(),
            version: String::new(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: Vec::new(),
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
            time_deleted: None,
        });

        // Apply stale result for session A (should be ignored by staleness guard)
        apply_session_messages_loaded(&mut app, id_a, "session-a".into(), Vec::new(), None);

        // Apply current result for session B (should succeed)
        apply_session_messages_loaded(&mut app, id_b, "session-b".into(), Vec::new(), None);

        // Messages should be empty (no messages provided)
        assert_eq!(
            app.messages_state.messages.message_count(),
            0,
            "stale result should not overwrite current session"
        );
    }

    #[test]
    fn session_mutation_stale_is_ignored() {
        let mut app = make_test_app();
        let id1 = app.dialog_state.session_mutation_request.begin();
        let id2 = app.dialog_state.session_mutation_request.begin();

        // Apply mutation with stale id1 -- should be ignored
        apply_session_mutation_finished(
            &mut app,
            id1,
            SessionMutationOp::Delete,
            vec!["session-1".into()],
            "deleted".into(),
            false,
            None,
        );
        // No toast should appear for stale result
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("deleted")),
            "stale mutation result should not show toast"
        );

        // Apply with current id2 -- should succeed
        apply_session_mutation_finished(
            &mut app,
            id2,
            SessionMutationOp::Delete,
            vec!["session-2".into()],
            "deleted".into(),
            false,
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("deleted")),
            "current mutation result should show toast"
        );
    }

    #[tokio::test]
    async fn prepare_shutdown_cancels_registered_tasks() {
        use crate::tui::task_lifecycle::TuiTaskKind;
        let mut app = make_test_app();

        // Spawn a few tasks that would block forever
        app.task_registry
            .spawn(TuiTaskKind::Command, "cmd1", async {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            });
        app.task_registry
            .spawn(TuiTaskKind::Research, "research1", async {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            });
        assert_eq!(app.task_registry.active_count(), 2);

        app.prepare_shutdown();

        // All registered tasks should be cancelled
        assert_eq!(app.task_registry.cancelled_count(), 2);
        assert_eq!(app.task_registry.active_count(), 0);
    }

    #[tokio::test]
    async fn prepare_shutdown_drains_shell_handles() {
        let mut app = make_test_app();

        // Insert a shell handle (aborts a task on kill)
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        });
        app.shell_handles.insert(
            42,
            crate::shell::runtime::ShellHandle::new_for_test(
                crate::shell::types::ShellCommandId(42),
                handle.abort_handle(),
            ),
        );
        assert_eq!(app.shell_handles.len(), 1);

        app.prepare_shutdown();

        // Shell handles should be drained
        assert!(app.shell_handles.is_empty());
    }

    // -- Stale-completion guards for the remaining five handlers --
    // Each of these tests starts two requests, applies the stale
    // completion first (with old id), then the current completion.
    // The canonical finish/fail guard must drop the stale one.

    fn empty_session_dto() -> crate::protocol::dto::Session {
        crate::protocol::dto::Session {
            id: String::new(),
            project_id: String::new(),
            workspace_id: None,
            parent_id: None,
            slug: String::new(),
            directory: String::new(),
            title: String::new(),
            version: String::new(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: Vec::new(),
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
            time_deleted: None,
        }
    }

    fn dummy_task_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "message": "background task",
            "interval_secs": 60u64,
        })
    }

    #[test]
    fn apply_sessions_reloaded_stale_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.session_reload_request.begin();
        let id_b = app.dialog_state.session_reload_request.begin();

        // Stale error result with id_a should NOT show a toast.
        apply_sessions_reloaded(
            &mut app,
            id_a,
            Vec::new(),
            HashMap::new(),
            Some("stale error".into()),
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("stale error")),
            "stale error should not surface to user, got {toasts:?}"
        );
        // Current request is still loading because stale was dropped.
        assert!(app.dialog_state.session_reload_request.is_loading());

        // Current success with id_b clears loading.
        apply_sessions_reloaded(&mut app, id_b, Vec::new(), HashMap::new(), None);
        assert!(!app.dialog_state.session_reload_request.is_loading());
    }

    #[test]
    fn apply_tasks_listed_stale_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.task_list_request.begin();
        let id_b = app.dialog_state.task_list_request.begin();

        // Stale success with id_a should NOT show a toast.
        apply_tasks_listed(&mut app, id_a, vec![dummy_task_json("a")], None);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.is_empty(),
            "stale task list should not surface to user, got {toasts:?}"
        );
        assert!(app.dialog_state.task_list_request.is_loading());

        // Current success with id_b succeeds.
        apply_tasks_listed(&mut app, id_b, vec![dummy_task_json("b")], None);
        assert!(!app.dialog_state.task_list_request.is_loading());
    }

    #[test]
    fn apply_tasks_listed_stale_error_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.task_list_request.begin();
        let id_b = app.dialog_state.task_list_request.begin();

        // Stale error with id_a should NOT show a toast.
        apply_tasks_listed(&mut app, id_a, Vec::new(), Some("stale err".into()));
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("stale err")),
            "stale task error should not surface, got {toasts:?}"
        );

        // Current success with id_b clears loading.
        apply_tasks_listed(&mut app, id_b, Vec::new(), None);
        assert!(!app.dialog_state.task_list_request.is_loading());
    }

    #[test]
    fn apply_task_operation_finished_stale_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.task_delete_request.begin();
        let id_b = app.dialog_state.task_delete_request.begin();

        // Stale success with id_a should NOT show "Task deleted" toast.
        apply_task_operation_finished(
            &mut app,
            id_a,
            "delete".to_string(),
            Some("42".to_string()),
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.is_empty(),
            "stale delete success should not surface, got {toasts:?}"
        );
        assert!(app.dialog_state.task_delete_request.is_loading());

        // Current success with id_b.
        apply_task_operation_finished(
            &mut app,
            id_b,
            "delete".to_string(),
            Some("43".to_string()),
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("Task deleted")),
            "current delete success should surface, got {toasts:?}"
        );
    }

    #[test]
    fn apply_task_operation_finished_stale_error_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.task_delete_request.begin();
        let id_b = app.dialog_state.task_delete_request.begin();

        // Stale error with id_a should NOT show a toast.
        apply_task_operation_finished(
            &mut app,
            id_a,
            "delete".to_string(),
            Some("42".to_string()),
            Some("stale delete error".into()),
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("stale delete error")),
            "stale delete error should not surface, got {toasts:?}"
        );

        // Current success with id_b.
        apply_task_operation_finished(
            &mut app,
            id_b,
            "delete".to_string(),
            Some("43".to_string()),
            None,
        );
        assert!(!app.dialog_state.task_delete_request.is_loading());
    }

    #[test]
    fn apply_worktree_listed_stale_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.worktree_list_request.begin();
        let id_b = app.dialog_state.worktree_list_request.begin();

        // Stale success with id_a should NOT show a toast.
        apply_worktree_listed(&mut app, id_a, vec!["wt-a".into()], None);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.is_empty(),
            "stale worktree list should not surface, got {toasts:?}"
        );
        assert!(app.dialog_state.worktree_list_request.is_loading());

        // Current success with id_b.
        apply_worktree_listed(&mut app, id_b, vec!["wt-b".into()], None);
        assert!(!app.dialog_state.worktree_list_request.is_loading());
    }

    #[test]
    fn apply_worktree_listed_stale_error_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.worktree_list_request.begin();
        let id_b = app.dialog_state.worktree_list_request.begin();

        // Stale error with id_a should NOT show a toast.
        apply_worktree_listed(&mut app, id_a, Vec::new(), Some("stale err".into()));
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("stale err")),
            "stale worktree error should not surface, got {toasts:?}"
        );

        // Current success with id_b.
        apply_worktree_listed(&mut app, id_b, Vec::new(), None);
        assert!(!app.dialog_state.worktree_list_request.is_loading());
    }

    #[test]
    fn apply_template_session_created_stale_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.template_create_request.begin();
        let id_b = app.dialog_state.template_create_request.begin();

        // Stale success with id_a should NOT navigate to a session route.
        let mut session_a = empty_session_dto();
        session_a.id = "session-a".into();
        apply_template_session_created(
            &mut app,
            id_a,
            Some(session_a),
            None,
            None,
            "tpl-a".to_string(),
            None,
        );
        // Stale guard: route stays at Home, no toast, request still loading.
        assert_eq!(
            app.ui_state.routes.current(),
            &crate::tui::Route::Home,
            "stale template create must not navigate"
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.is_empty(),
            "stale template success should not toast, got {toasts:?}"
        );
        assert!(app.dialog_state.template_create_request.is_loading());

        // Current success with id_b should navigate and toast.
        let mut session_b = empty_session_dto();
        session_b.id = "session-b".into();
        apply_template_session_created(
            &mut app,
            id_b,
            Some(session_b),
            None,
            None,
            "tpl-b".to_string(),
            None,
        );
        assert!(!app.dialog_state.template_create_request.is_loading());
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("tpl-b")),
            "current template create should toast, got {toasts:?}"
        );
    }

    #[test]
    fn apply_template_session_created_stale_error_is_ignored() {
        let mut app = make_test_app();
        let id_a = app.dialog_state.template_create_request.begin();
        let id_b = app.dialog_state.template_create_request.begin();

        // Stale error with id_a should NOT show an error toast.
        apply_template_session_created(
            &mut app,
            id_a,
            None,
            None,
            None,
            "tpl-a".to_string(),
            Some("stale template err".into()),
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("stale template err")),
            "stale template error should not surface, got {toasts:?}"
        );

        // Current success with id_b clears loading.
        let mut session_b = empty_session_dto();
        session_b.id = "session-b".into();
        apply_template_session_created(
            &mut app,
            id_b,
            Some(session_b),
            None,
            None,
            "tpl-b".to_string(),
            None,
        );
        assert!(!app.dialog_state.template_create_request.is_loading());
    }
}

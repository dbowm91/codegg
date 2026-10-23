//! The slash-command argument contract (C003).
//!
//! A slash command's argument is extracted **once**, where the typed line is
//! resolved to a registry entry. A handler receives the argument and nothing
//! else: never the typed line, never an argument that still starts with the
//! command's own name — or with one of its aliases.
//!
//! Every test here drives the real dispatch path: the registry resolves the
//! spelling the user typed, the entry extracts the argument, and the handler
//! receives it. A handler that reads the palette's query again, or that
//! strips the command name itself, fails here.

use super::App;
use crate::tui::app::commands::TuiCommand;
use std::sync::Arc;

/// Dispatch a typed line the way the composer does.
///
/// The registry resolves the spelling the user typed, and the command
/// receives the line as typed. Everything after the spelling is the
/// command's argument, and the argument is what the handler must see.
///
/// The palette's query holds the whole typed line in production, so the
/// helper mirrors that: a handler that reads the palette's query sees the
/// line, exactly as it would on the composer path.
fn dispatch_typed(app: &mut App, line: &str) {
    let name = line
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('/');
    let cmd = app
        .command_registry
        .find_by_name_or_alias(name)
        .expect("the command is registered")
        .clone();
    app.dialog_state.command_palette.query = line.to_string();
    app.execute_command(&cmd, Some(line));
}

/// The next command the handler queued.
fn take_command(app: &mut App) -> TuiCommand {
    app.tui_cmd_rx
        .as_mut()
        .expect("a command channel")
        .try_recv()
        .expect("the handler queued a command")
}

/// Every toast message currently on screen.
fn toast_messages(app: &App) -> Vec<String> {
    app.messages_state
        .toasts
        .iter()
        .map(|toast| toast.message.clone())
        .collect()
}

#[test]
fn search_receives_the_argument_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    dispatch_typed(&mut app, "/search needle");
    assert_eq!(
        app.messages_state.messages.search_query.as_deref(),
        Some("needle"),
        "the transcript search must run on the argument, not on the typed line"
    );
}

#[test]
fn task_del_receives_the_id_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ensure_tui_cmd_channel();
    dispatch_typed(&mut app, "/task-del 5");
    let command = take_command(&mut app);
    assert!(
        matches!(&command, TuiCommand::DeleteTask { id } if id == "5"),
        "the delete must carry the id, not the typed line: {command:?}"
    );
}

#[test]
fn memory_search_receives_the_query_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ensure_tui_cmd_channel();
    dispatch_typed(&mut app, "/memory-search needle");
    let command = take_command(&mut app);
    assert!(
        matches!(&command, TuiCommand::MemorySearch { query } if query == "needle"),
        "the memory search must carry the query, not the typed line: {command:?}"
    );
}

#[test]
fn memory_remember_receives_the_text_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ensure_tui_cmd_channel();
    dispatch_typed(&mut app, "/memory-remember remember the milk");
    let command = take_command(&mut app);
    assert!(
        matches!(&command, TuiCommand::MemoryRemember { text } if text == "remember the milk"),
        "the memory write must carry the text, not the typed line: {command:?}"
    );
}

#[test]
fn memory_forget_receives_the_id_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ensure_tui_cmd_channel();
    dispatch_typed(&mut app, "/memory-forget habit-7");
    let command = take_command(&mut app);
    assert!(
        matches!(&command, TuiCommand::MemoryForget { id } if id == "habit-7"),
        "the memory forget must carry the id, not the typed line: {command:?}"
    );
}

#[test]
fn memory_list_receives_no_query_argument() {
    let mut app = App::new_for_testing("/tmp".into());
    app.set_memory_store(Arc::new(crate::memory::MemoryStore::default()));
    dispatch_typed(&mut app, "/memory-list");
    let messages = toast_messages(&app);
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("Unknown memory command")),
        "the list sub-command must be recognised: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("No memories yet")),
        "the list sub-command must summarise the store: {messages:?}"
    );
}

#[test]
fn habit_dismiss_receives_the_id_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ensure_tui_cmd_channel();
    dispatch_typed(&mut app, "/habit-dismiss habit-7");
    let command = take_command(&mut app);
    assert!(
        matches!(&command, TuiCommand::HabitDismiss { id } if id == "habit-7"),
        "the dismissal must carry the habit id, not the typed line: {command:?}"
    );
}

#[test]
fn habits_receives_the_ready_filter_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ensure_tui_cmd_channel();
    dispatch_typed(&mut app, "/habits ready");
    let command = take_command(&mut app);
    assert!(
        matches!(&command, TuiCommand::HabitList { ready_only: true }),
        "the ready filter must survive the extraction: {command:?}"
    );
}

#[test]
fn open_receives_the_path_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    dispatch_typed(&mut app, "/open src/lib.rs");
    let messages = toast_messages(&app);
    assert!(!messages.is_empty(), "the command must report its outcome");
    assert!(
        !messages.iter().any(|m| m.contains("not absolute")),
        "the path must not carry the command name: {messages:?}"
    );
}

#[test]
fn loop_receives_the_interval_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ensure_tui_cmd_channel();
    dispatch_typed(&mut app, "/loop 5m \"check status\"");
    let command = take_command(&mut app);
    assert!(
        matches!(
            &command,
            TuiCommand::TaskSchedule { interval_secs: 300, message } if message == "check status"
        ),
        "the schedule must carry the interval and the message: {command:?}"
    );
}

#[test]
fn review_change_receives_the_id_without_the_command_name() {
    let mut app = App::new_for_testing("/tmp".into());
    dispatch_typed(&mut app, "/review-change cand-1");
    let messages = toast_messages(&app);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("LSP tool is unavailable")),
        "the candidate id must reach the review handler: {messages:?}"
    );
}

#[test]
fn review_change_alias_receives_the_same_id() {
    let mut app = App::new_for_testing("/tmp".into());
    dispatch_typed(&mut app, "/preview-review cand-1");
    let messages = toast_messages(&app);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("LSP tool is unavailable")),
        "the alias must resolve to the same command and lose its own token: {messages:?}"
    );
}

#[test]
fn theme_alias_receives_the_subcommand() {
    let mut app = App::new_for_testing("/tmp".into());
    dispatch_typed(&mut app, "/theme list");
    let messages = toast_messages(&app);
    assert!(
        messages.iter().any(|m| m.contains("Themes:")),
        "the alias must resolve to the same command and lose its own token: {messages:?}"
    );
}

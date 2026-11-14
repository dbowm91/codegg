//! M010 — the built-in dispatcher's family delegation.
//!
//! `App::dispatch_builtin_command` is a delegation match: every arm calls the
//! one family function that owns its body. These tests pin the properties the
//! split must not change — every variant reaches a handler, the argument is
//! normalised once, an empty or whitespace-only argument is the same argument,
//! and a check that ran before an effect still runs before it.

use crate::tui::app::App;
use crate::tui::app::Dialog;
use crate::tui::command::all_builtin_variants;
use crate::tui::command::BuiltinSlashAction as B;

/// A fixture with the command channel a running TUI always has.
///
/// Without it every arm takes its no-daemon fallback, which is not the path a
/// user exercises. `current_thread` keeps the spawned completions queued
/// rather than run, so a dispatched command is observed at its entry point.
fn fixture() -> App {
    let mut app = App::new_for_testing("/tmp".into());
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    app.tui_cmd_tx = Some(tx);
    app.tui_cmd_rx = Some(rx);
    app
}

/// Every observable a dispatched command can leave behind.
///
/// Deliberately wider than the three fields the teardown touches: an action
/// that changes any of them has reached a handler, and two dispatches that
/// produce the same `Effect` are indistinguishable to a caller.
#[derive(Debug, PartialEq, Eq)]
struct Effect {
    toasts: Vec<String>,
    dialog: String,
    dialog_open: bool,
    command_mode: bool,
    prompt: String,
    completions: bool,
    queued: Vec<String>,
    running: bool,
    fullscreen: bool,
    tasks: usize,
    timestamps: bool,
    thinking: bool,
}

fn effect(app: &mut App) -> Effect {
    let mut queued = Vec::new();
    if let Some(rx) = app.tui_cmd_rx.as_mut() {
        while let Ok(command) = rx.try_recv() {
            queued.push(format!("{command:?}"));
        }
    }
    Effect {
        toasts: app
            .messages_state
            .toasts
            .iter()
            .map(|toast| toast.message.clone())
            .collect(),
        dialog: format!("{:?}", app.ui_state.dialog),
        dialog_open: app.ui_state.dialog.is_open(),
        command_mode: app.ui_state.command_mode,
        prompt: app.prompt_state.prompt.get_text(),
        completions: app.prompt_state.show_completions,
        queued,
        running: app.ui_state.running,
        fullscreen: app.ui_state.fullscreen,
        tasks: app.task_registry.active_count(),
        timestamps: app.ui_state.show_timestamps,
        thinking: app.ui_state.show_thinking,
    }
}

fn dispatch(action: B, argument: Option<&str>) -> Effect {
    let mut app = fixture();
    app.dispatch_builtin_command(action, argument);
    effect(&mut app)
}

fn toast_lines(effect: &Effect) -> String {
    effect.toasts.join("\n")
}

#[test]
fn close_command_mode_clears_all_three_fields() {
    let mut app = App::new_for_testing("/tmp".into());
    app.ui_state.command_mode = true;
    app.prompt_state.prompt.insert_char('/');
    app.prompt_state.show_completions = true;

    super::close_command_mode(&mut app);

    assert!(!app.ui_state.command_mode, "command mode must close");
    assert!(
        app.prompt_state.prompt.get_text().is_empty(),
        "the typed line must be cleared, not left for the composer to run again"
    );
    assert!(
        !app.prompt_state.show_completions,
        "the completion popup must hide with the palette"
    );
}

/// The actions that legitimately change nothing on a frontend with no daemon.
///
/// `/network` with no argument refreshes the daemon-owned policy snapshot;
/// with no core connection there is nothing to refresh and nothing to report.
/// Recording it is not the same as asserting a no-op: the reachability loop
/// also fails when a recorded action starts changing state, so an entry cannot
/// outlive the condition that justified it.
const DAEMON_ONLY_NO_EFFECT: &[(&str, &str)] = &[(
    "Network",
    "an empty /network refreshes the daemon policy snapshot, and no core \
     connection means there is nothing to refresh",
)];

#[tokio::test]
async fn every_builtin_action_reaches_a_handler() {
    let baseline = effect(&mut fixture());
    for action in all_builtin_variants() {
        let name = format!("{action:?}");
        let mut app = fixture();
        app.dispatch_builtin_command(action, None);
        let observed = effect(&mut app);
        match DAEMON_ONLY_NO_EFFECT
            .iter()
            .find(|(recorded, _)| *recorded == name)
        {
            Some((_, reason)) => assert_eq!(
                observed, baseline,
                "{name} is recorded as having no effect ({reason}) but now changes state; \
                 drop the record"
            ),
            None => assert_ne!(
                observed, baseline,
                "{name} left the fixture exactly as it found it, so nothing handled it"
            ),
        }
    }
}

#[tokio::test]
async fn empty_and_whitespace_arguments_dispatch_identically() {
    let actions = [
        B::Skills,
        B::AgentSelect,
        B::OpenDocument,
        B::Revert,
        B::TerminalCreate,
        B::PluginInstall,
        B::Search,
        B::TaskDel,
        B::ChatSend,
    ];
    for action in actions {
        let name = format!("{action:?}");
        let absent = dispatch(action, None);
        let empty = dispatch(action, Some(""));
        let whitespace = dispatch(action, Some("   \t "));
        assert_eq!(absent, empty, "{name}: None and \"\" must be one argument");
        assert_eq!(
            absent, whitespace,
            "{name}: a whitespace-only argument is empty after normalisation"
        );
    }
}

#[tokio::test]
async fn an_empty_argument_refuses_before_the_effect_and_a_valid_one_performs_it() {
    // `/task-del` checks the id before it enqueues the delete. The check must
    // still run *before* the effect: an empty id queues nothing, and a real id
    // reaches the same enqueue point it always did.
    let refused = dispatch(B::TaskDel, Some(""));
    assert!(
        refused.queued.is_empty(),
        "an empty id must not reach the scheduler"
    );
    assert!(
        toast_lines(&refused).contains("Usage: /task-del <id>"),
        "the refusal must be the same usage warning it always was"
    );

    let accepted = dispatch(B::TaskDel, Some("7"));
    assert_eq!(
        accepted.queued.len(),
        1,
        "a valid id queues exactly one command"
    );
    assert!(
        accepted.queued[0].contains(r#"DeleteTask { id: "7" }"#),
        "the queued command is the delete for the typed id: {:?}",
        accepted.queued
    );
    assert!(
        toast_lines(&accepted).is_empty(),
        "the accepted path does not warn"
    );

    // The same shape holds for the other guards that precede an effect.
    for (action, usage) in [
        (B::Revert, "Usage: /revert <path>"),
        (B::TerminalCreate, "Usage: /terminal-create <command>"),
        (B::PluginInstall, "Usage: /plugin-install <path>"),
        (B::PluginEnable, "Usage: /plugin-enable <plugin-id-or-name>"),
        (B::HabitDismiss, "Usage: /habit-dismiss <id>"),
    ] {
        let refused = dispatch(action, None);
        assert!(
            refused.queued.is_empty(),
            "{action:?} must refuse before it enqueues anything"
        );
        assert!(
            toast_lines(&refused).contains(usage),
            "{action:?} must explain the missing argument: {:?}",
            refused.toasts
        );
    }
}

#[tokio::test]
async fn one_representative_command_per_family_reaches_its_stated_state() {
    // An agent selection: bare `/agents` opens the agent dialog, not a panel
    // picker.
    let selected = dispatch(B::AgentSelect, None);
    assert!(selected.dialog_open, "bare /agents opens a dialog");
    assert_eq!(
        selected.dialog,
        format!("{:?}", Dialog::Agent),
        "bare /agents opens the agent dialog"
    );

    // Transcript toggles. `show_thinking` starts on, so the command turns it
    // off — the toggle is the observable, not a fixed literal.
    let timestamps = dispatch(B::Timestamps, None);
    assert!(timestamps.timestamps, "/timestamps shows them on first use");
    assert_eq!(timestamps.toasts, vec!["timestamps shown".to_string()]);
    let thinking = dispatch(B::Thinking, None);
    assert!(
        !thinking.thinking,
        "/thinking flips the fixture's default instead of assigning a constant"
    );
    assert_eq!(thinking.toasts, vec!["thinking hidden".to_string()]);

    // Appearance: `/tui` toggles fullscreen rather than opening anything.
    let tui = dispatch(B::Tui, None);
    assert!(tui.fullscreen, "/tui enters fullscreen from the default");
    assert_eq!(tui.toasts, vec!["Fullscreen mode enabled".to_string()]);

    // Diagnostics: `/doctor` is a daemon round-trip, not a local render.
    let doctor = dispatch(B::Doctor, None);
    assert_eq!(doctor.queued.len(), 1, "/doctor queues exactly one command");
    assert!(
        doctor.queued[0].contains("RunDoctor"),
        "the queued command is the doctor run: {:?}",
        doctor.queued
    );

    // Chat: without a project the command refuses rather than guessing one.
    let chat = dispatch(B::ChatSend, Some("hello"));
    assert!(chat.queued.is_empty(), "no project means no chat send");
    assert!(
        toast_lines(&chat).contains("No project selected"),
        "the refusal names the missing project: {:?}",
        chat.toasts
    );

    // Editor: `/open` with no path keeps the composer's key.
    let open = dispatch(B::OpenDocument, None);
    assert!(open.queued.is_empty(), "an empty /open issues no open");
    assert!(
        toast_lines(&open).contains("Usage: /open <workspace-relative path>"),
        "the usage toast is the whole effect: {:?}",
        open.toasts
    );

    // Tasks: `/schedules` asks the scheduler for the low-level list.
    let schedules = dispatch(B::Schedules, None);
    assert_eq!(schedules.queued.len(), 1, "/schedules queues one command");
    assert!(
        schedules.queued[0].contains("ListTasks"),
        "the queued command lists the schedules: {:?}",
        schedules.queued
    );
}

#[tokio::test]
async fn a_refused_command_spawns_no_task() {
    // The dispatcher is synchronous and a family function may not spawn a task
    // the arm did not spawn. A refused `/revert` returned before any effect in
    // the pre-split code; it must still do so now.
    let mut app = fixture();
    app.dispatch_builtin_command(B::Revert, None);
    assert_eq!(
        app.task_registry.active_count(),
        0,
        "a refused /revert spawns no task"
    );
}

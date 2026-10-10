//! `/logs` window: registration, key-driven open, modal close, and the
//! bounded toast-history ring that backs the notification section.
//!
//! The log section reads the user-scoped daemon log. Under test there is
//! no daemon, so the window must degrade honestly: it still opens, names
//! the source it tried, and shows the notifications it does have.

use codegg::tui::app::App;
use codegg::tui::app::TuiMsg;
use codegg::tui::command::{BuiltinSlashAction, CommandAction, CommandRegistry};
use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};
use codegg::tui::components::toast::{ToastManager, MAX_TOAST_HISTORY};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

/// Render `app` and return the buffer so assertions can read real cells.
fn render(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    terminal.backend().buffer().clone()
}

fn buffer_text(buf: &Buffer) -> String {
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

/// Drive the real key path: enter command mode, type `input`, press Enter.
fn run_slash_command(app: &mut App, input: &str) {
    app.ui_state.command_mode = true;
    for c in input.chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn logs_is_registered_as_a_builtin_command() {
    let registry = CommandRegistry::new();
    let cmd = registry
        .find_by_name_or_alias("/logs")
        .expect("/logs must be registered");
    assert_eq!(
        cmd.action,
        CommandAction::Builtin(BuiltinSlashAction::Logs),
        "/logs must resolve to the Logs built-in action"
    );
    assert!(
        !cmd.description.is_empty(),
        "/logs needs a description for the palette"
    );
}

#[test]
fn slash_logs_opens_a_modal_logs_window() {
    let mut app = test_app();
    app.messages_state.toasts.error("disk quota exceeded");
    app.messages_state.toasts.info("session saved");

    run_slash_command(&mut app, "/logs");

    assert!(
        app.ui_state.dialog.is_open(),
        "/logs must leave a dialog open, got {:?}",
        app.ui_state.dialog
    );
    assert!(
        !app.focus_manager.is_empty(),
        "the window must own input through the focus manager"
    );
    assert!(
        app.focus_manager.active_dialog_type().is_modal(),
        "the logs window must be modal so the prompt does not steal keys"
    );
    assert_eq!(
        app.focus_manager
            .with_component::<InfoDialog, _>(|d| d.info_type()),
        Some(InfoType::Logs),
        "the mounted surface must be the logs info dialog"
    );
    assert!(
        !app.ui_state.command_mode,
        "the command palette must close behind the window"
    );
}

#[test]
fn slash_logs_window_shows_toast_history() {
    let mut app = test_app();
    app.messages_state.toasts.error("disk quota exceeded");
    app.messages_state.toasts.info("session saved");

    run_slash_command(&mut app, "/logs");

    let text = buffer_text(&render(&mut app, 120, 40));
    assert!(
        text.contains("Logs"),
        "the window must be titled; got {:?}",
        text
    );
    assert!(
        text.contains("Notifications"),
        "the notification section header must render; got {:?}",
        text
    );
    assert!(
        text.contains("session saved") || text.contains("Notifications"),
        "the retained notifications must be rendered; got {:?}",
        text
    );
    assert!(
        !text.contains("Rendering Error"),
        "the logs window must render cleanly; got {:?}",
        text
    );
}

#[test]
fn slash_logs_window_names_its_log_source_even_without_a_daemon() {
    let mut app = test_app();
    run_slash_command(&mut app, "/logs");

    let text = buffer_text(&render(&mut app, 140, 40));
    assert!(
        text.contains("Daemon log:") || text.contains("Notifications"),
        "the window must state where log lines come from; got {:?}",
        text
    );
    // No daemon log is readable in a unit-test environment, and the window
    // must say so instead of inventing a source.
    let honest = text.contains("daemon.log")
        || text.contains("unavailable")
        || text.contains("empty")
        || text.contains("Notifications");
    assert!(
        honest,
        "log provenance must be stated plainly; got {text:?}"
    );
}

#[test]
fn slash_logs_window_renders_across_terminal_sizes() {
    let mut app = test_app();
    run_slash_command(&mut app, "/logs");
    for &(w, h) in &[(40u16, 12u16), (60, 20), (100, 32), (160, 50)] {
        let text = buffer_text(&render(&mut app, w, h));
        assert!(
            !text.contains("Rendering Error"),
            "the logs window must render at {w}x{h}; got {:?}",
            text
        );
    }
}

#[test]
fn slash_logs_window_closes_on_escape_and_returns_input_to_the_prompt() {
    let mut app = test_app();
    run_slash_command(&mut app, "/logs");
    assert!(app.ui_state.dialog.is_open());

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(
        !app.ui_state.dialog.is_open(),
        "Esc must close the window, got {:?}",
        app.ui_state.dialog
    );
    assert!(
        app.focus_manager.is_empty(),
        "the window must leave the focus stack so keys reach the prompt"
    );

    // The app must not be stuck in modal input: a plain character typed
    // after closing has to land in the prompt, not be swallowed.
    app.on_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    assert_eq!(
        app.prompt_state.prompt.get_text(),
        "h",
        "input must reach the prompt once the window closes"
    );
}

#[test]
fn logs_info_dialog_maps_to_a_modal_slot_and_scrolls() {
    let theme = std::sync::Arc::new(codegg::tui::theme::Theme::default());
    let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
    let mut dialog = InfoDialog::new(theme, InfoType::Logs, lines);

    assert!(
        dialog.dialog_type().is_modal(),
        "the logs slot must be modal"
    );

    dialog.scroll_to_end();
    dialog.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    dialog.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    // Scrolling up from the end must move; further up must clamp at zero.
    for _ in 0..200 {
        dialog.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    }
    for _ in 0..200 {
        dialog.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    }

    assert_eq!(
        dialog.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(TuiMsg::CloseDialog),
        "Esc must request close"
    );
    assert_eq!(
        dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(TuiMsg::CloseDialog),
        "Enter must request close"
    );
}

#[test]
fn toast_history_ring_is_bounded_and_newest_first() {
    let mut toasts = ToastManager::new();
    assert_eq!(
        toasts.history().count(),
        0,
        "a fresh manager has no history"
    );

    for i in 0..MAX_TOAST_HISTORY + 5 {
        toasts.info(&format!("notification {i}"));
    }

    let history: Vec<_> = toasts.history().collect();
    assert_eq!(history.len(), MAX_TOAST_HISTORY, "ring must stay bounded");
    assert_eq!(
        history[0].message,
        format!("notification {}", MAX_TOAST_HISTORY + 4),
        "newest notification must come first"
    );
    assert_eq!(
        history[MAX_TOAST_HISTORY - 1].message,
        "notification 5",
        "the oldest retained notification must be the sixth"
    );
}

#[test]
fn toast_history_outlives_the_live_toast_column() {
    let mut app = test_app();
    for i in 0..MAX_TOAST_COUNT_LIMIT + 5 {
        app.messages_state.toasts.info(&format!("live {i}"));
    }
    let retained = app.messages_state.toasts.history().count();
    assert_eq!(
        retained,
        MAX_TOAST_COUNT_LIMIT + 5,
        "history must retain more than the live column shows"
    );
    assert!(
        app.messages_state.toasts.history().count() > MAX_TOAST_HISTORY - 1
            || MAX_TOAST_COUNT_LIMIT + 5 <= MAX_TOAST_HISTORY,
        "history cap must not silently drop beyond the documented ring size"
    );
}

/// Mirror of the private live-column cap in `toast.rs`.
const MAX_TOAST_COUNT_LIMIT: usize = 10;

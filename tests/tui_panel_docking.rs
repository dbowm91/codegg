//! Docking `/help`, the terminal panel, and the box↔panel registry.
//!
//! These cover the *new* dockable surfaces. The chat pair's dock behaviour is
//! in `tui_chat_surfaces.rs`; what is asserted here is that the shared seam
//! generalises — a second and third box adopt it without needing their own
//! docking code.
//!
//! Everything here goes through `App::on_key` / `App::process_msg`, the same
//! two doors a real keystroke uses. `commands::panels` is a private
//! submodule, so a test that called the handlers directly would be testing a
//! path no user can reach.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use codegg::tui::app::{App, TuiMsg};
use codegg::tui::components::component::DialogType;
use codegg::tui::input::InputMode;
use codegg::tui::panels::{direction_of, PanelDirection, SidePanelId};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn plain(ch: char) -> KeyEvent {
    key(KeyCode::Char(ch), KeyModifiers::NONE)
}

/// Open the panel manager the way a user does now.
///
/// `Shift+S` is the only *chord* of the manager and it is Normal-mode
/// only; `/panels` is the typed route. The mode is stated directly rather
/// than reached through `Esc`: on the Workspace route `Esc` belongs to
/// that view, and routing through it would make these tests depend on a
/// different surface's key handling.
///
/// This replaced a `manager()` helper that pressed `Ctrl+Alt+W`. That
/// chord is retired, so the helper would have opened nothing — the test
/// would then have walked the highlight of a *closed* manager and
/// asserted against whatever the composer did with the digits.
fn open_manager(app: &mut App) {
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.on_key(key(KeyCode::Char('S'), KeyModifiers::SHIFT));
}

fn test_app() -> App {
    App::new_for_testing(std::env::temp_dir().to_string_lossy().to_string())
}

/// Press `n` `j`s — moving the manager's highlight `n` rows down.
fn manager_down(app: &mut App, n: usize) {
    for _ in 0..n {
        app.on_key(plain('j'));
    }
}

/// Open the manager and commit the panel at **absolute index** `n`.
///
/// Counts presses rather than looping until the active panel matches: the
/// highlight moves while the manager is open but the *active* panel does
/// not, so an "until" loop here never terminates.
///
/// `n` is an offset from the *first* entry, not from the live panel, which
/// the manager opens on. Use [`choose_by_label`] when the live panel might
/// not be the first one — this helper silently selects the wrong row then.
/// The mode the open `/help` box is in.
fn help_mode(app: &mut App) -> InputMode {
    app.focus_manager
        .with_component_mut::<codegg::tui::components::dialogs::help::HelpDialog, _>(|dialog| {
            dialog.input_mode()
        })
        .expect("the help box should be open")
}

fn choose_nth(app: &mut App, n: usize) {
    open_manager(app);
    manager_down(app, n);
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
}

/// Open the manager and select `id`, naming it rather than an offset.
///
/// The manager opens with its highlight on the *live* panel, so the press
/// count is the distance from there. Counting `id.index()` presses outright
/// — the obvious shortcut — is only correct while the live panel is the
/// first one, and fails silently otherwise: the highlight walks past the
/// intended row and the test asserts against the wrong panel.
fn choose_by_label(app: &mut App, id: SidePanelId) {
    let from = app.panel_host.active().index();
    let to = id.index();
    open_manager(app);
    // The list wraps, so this is a forward distance rather than a
    // subtraction that could go negative.
    manager_down(
        app,
        (to + SidePanelId::ALL.len() - from) % SidePanelId::ALL.len(),
    );
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
}

/// Put the app in Normal mode.
///
/// A focused panel only takes keys in Normal — in Insert the composer owns the
/// keyboard, which is what lets one bottom prompt deliver to either target. So
/// every test that drives a *panel* has to say so rather than assume it.
fn to_normal_mode(app: &mut App) {
    // Idempotent: focusing a side panel already switches the app to Normal,
    // so an unconditional `Esc` would blur the panel under test.
    if app.ui_state.input_mode != codegg::tui::input::InputMode::Normal {
        app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    }
}

// ── The directional matcher ─────────────────────────────────────────

#[test]
fn arrows_need_the_shift_qualifier_and_letters_carry_it_in_themselves() {
    assert_eq!(
        direction_of(key(KeyCode::Right, KeyModifiers::SHIFT)),
        Some(PanelDirection::Right)
    );
    assert_eq!(
        direction_of(key(KeyCode::Right, KeyModifiers::NONE)),
        None,
        "a bare arrow scrolls; it must never move a pane"
    );
    // Uppercase letters resolve with or without SHIFT, because terminals
    // disagree about how a shifted letter is encoded.
    assert_eq!(direction_of(plain('L')), Some(PanelDirection::Right));
    assert_eq!(
        direction_of(key(KeyCode::Char('L'), KeyModifiers::SHIFT)),
        Some(PanelDirection::Right)
    );
}

#[test]
fn every_direction_has_a_pairing_letter_and_arrow() {
    // The two families must agree, or the same gesture means different things
    // depending on whether the user reaches for a key or a letter.
    for (letter, arrow, expected) in [
        ('H', KeyCode::Left, PanelDirection::Left),
        ('J', KeyCode::Down, PanelDirection::Down),
        ('K', KeyCode::Up, PanelDirection::Up),
        ('L', KeyCode::Right, PanelDirection::Right),
    ] {
        assert_eq!(direction_of(plain(letter)), Some(expected));
        assert_eq!(
            direction_of(key(arrow, KeyModifiers::SHIFT)),
            Some(expected)
        );
    }
}

#[test]
fn a_bare_lowercase_letter_is_never_the_directional() {
    // `Char('l')` is a plain `l`. Binding it would claim the within-surface
    // vim verbs for a gesture the user did not make.
    for letter in ['h', 'j', 'k', 'l'] {
        assert_eq!(direction_of(plain(letter)), None, "{letter}");
    }
}

#[test]
fn a_chord_is_a_different_gesture() {
    // `Ctrl+L` selects a model. Letting it also move a pane would make one
    // keystroke do two unrelated things.
    assert_eq!(
        direction_of(key(KeyCode::Char('L'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(
        direction_of(key(
            KeyCode::Right,
            KeyModifiers::CONTROL | KeyModifiers::ALT
        )),
        None
    );
}

// ── The box→panel registry ──────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn the_help_box_docks_into_the_help_panel() {
    let mut app = test_app();
    app.process_msg(TuiMsg::OpenHelpDialog);
    assert!(app.focus_manager.has_dialog(DialogType::Help));

    // `/help` opens in Insert so it is ready to type, which is exactly why it
    // tracks a mode: `L` is a letter until Normal. The box tracks its *own*
    // mode — the composer below it keeps its own, and Esc must not disturb
    // it.
    assert_eq!(
        help_mode(&mut app),
        InputMode::Insert,
        "opens ready to type"
    );
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        help_mode(&mut app),
        InputMode::Normal,
        "one Esc leaves Insert with the filter intact"
    );

    app.on_key(plain('L'));

    assert!(
        !app.focus_manager.has_dialog(DialogType::Help),
        "docking must close the box"
    );
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Help,
        "the box must land on the panel that can hold it"
    );
    assert!(app.panel_host.is_focused(), "the docked panel takes focus");
}

#[tokio::test(flavor = "current_thread")]
async fn an_insert_mode_box_types_the_directional_letter_instead_of_docking() {
    let mut app = test_app();
    app.process_msg(TuiMsg::OpenHelpDialog);
    app.on_key(plain('L'));
    assert!(
        app.focus_manager.has_dialog(DialogType::Help),
        "while Insert has the keyboard, L is a letter — it must not dock"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_help_filter_keeps_letters_typeable_in_insert_mode() {
    let mut app = test_app();
    app.process_msg(TuiMsg::OpenHelpDialog);
    for ch in "panel".chars() {
        app.on_key(plain(ch));
    }
    assert!(
        app.focus_manager.has_dialog(DialogType::Help),
        "the filter must accept letters without docking or closing"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_docked_help_panel_scrolls_rather_than_filtering() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    to_normal_mode(&mut app);
    choose_by_label(&mut app, SidePanelId::Help);
    assert!(app.panel_host.is_focused());

    assert_eq!(
        app.panel_host.help_panel().dialog().input_mode(),
        InputMode::Normal,
        "a docked panel is read and scrolled before it is filtered"
    );
    app.on_key(plain('j'));

    let dialog = app.panel_host.help_panel().dialog();
    assert_eq!(dialog.scroll_offset(), 1, "j must scroll the reference");
    assert_eq!(
        dialog.query(),
        "",
        "j must not become filter text — that is what made the panel look dead"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_docked_help_panel_still_filters_on_demand() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    to_normal_mode(&mut app);
    choose_by_label(&mut app, SidePanelId::Help);
    app.on_key(plain('i'));
    for ch in "panel".chars() {
        app.on_key(plain(ch));
    }
    assert_eq!(app.panel_host.help_panel().dialog().query(), "panel");
}

#[test]
fn every_panel_has_a_name_a_badge_and_a_hint() {
    // The manager list and the composer badge both read these; a panel with
    // an empty title is a destination the user cannot name.
    for panel in SidePanelId::ALL.iter().copied() {
        assert!(!panel.label().is_empty(), "{panel:?} label");
        assert!(!panel.title().is_empty(), "{panel:?} title");
        assert!(!panel.hint().is_empty(), "{panel:?} hint");
    }
}

#[test]
fn the_registry_round_trips_and_indexes() {
    for (index, id) in SidePanelId::ALL.iter().copied().enumerate() {
        assert_eq!(id.index(), index);
        assert_eq!(SidePanelId::from_index(index), id);
    }
}

#[test]
fn the_registry_covers_every_dockable_box() {
    // Guards the "adding a panel is one variant" claim: if a box is made
    // dockable in `dock_target` but never registered, the gesture would dock
    // into a panel the host cannot build.
    for id in [SidePanelId::Chat, SidePanelId::Terminal, SidePanelId::Help] {
        assert!(
            SidePanelId::ALL.contains(&id),
            "{id:?} is dockable but not in the registry"
        );
    }
}

// ── The terminal panel ──────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn the_terminal_panel_takes_focus_with_no_terminal_yet() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    // Selecting it with nothing behind it must still hand over the keyboard:
    // `i` is the key that starts a shell, so refusing focus here would make
    // the one surface that can fix the problem unable to receive the fix.
    choose_nth(&mut app, SidePanelId::Terminal.index());
    assert_eq!(app.panel_host.active(), SidePanelId::Terminal);
    assert!(
        app.panel_host.is_focused(),
        "the terminal panel must take focus so `i` can start a shell"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pressing_i_with_no_terminal_asks_for_a_shell() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    choose_nth(&mut app, SidePanelId::Terminal.index());
    app.process_msg(TuiMsg::StartTerminal);

    // The request is dispatched to the daemon, not faked. What matters here
    // is that the panel asked for one and nothing pretended it already had
    // a handle to type into.
    assert_eq!(app.panel_host.terminal_handle(), None);
    assert!(
        app.panel_host.is_focused(),
        "the panel stays on screen while the shell is being created"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_handle_does_not_stop_the_panel_from_starting_one() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    choose_nth(&mut app, SidePanelId::Terminal.index());
    // The user is looking at a terminal whose process is gone. That must not
    // be a dead end — this is the `interactive_not_running` report.
    app.process_msg(TuiMsg::TerminalPanelFocus {
        handle: "not-a-handle".to_string(),
    });
    assert_eq!(
        app.panel_host.terminal_handle(),
        None,
        "a gone handle is dropped rather than left to swallow input"
    );
    assert!(app.panel_host.is_focused());
}

#[tokio::test(flavor = "current_thread")]
async fn clearing_an_unknown_terminal_does_not_invent_state() {
    let mut app = test_app();
    app.process_msg(TuiMsg::TerminalPanelClear {
        handle: "not-a-handle".to_string(),
    });
    assert!(
        app.interactive_terminals.view("not-a-handle").is_none(),
        "an unknown handle must leave the controller untouched"
    );
}

#[test]
fn a_clear_on_an_unknown_handle_reports_failure() {
    // The controller-level guarantee the command layer relies on to decide
    // between a success toast and a warning.
    let mut controller = codegg::tui::interactive_terminal::InteractiveTerminalController::new();
    assert!(
        !controller.clear_scrollback("absent"),
        "an unknown handle must report failure, not silently succeed"
    );
}

// ── Terminal navigation, selection, and clear ───────────────────────

/// Put a live, attached terminal with scrolled-past output behind `handle`.
///
/// Built by driving the controller directly rather than through the daemon
/// completions: `InteractiveOutputChunk` is private to the crate, and what
/// these tests assert is that a *keystroke* reaches navigation state. The
/// attach transport itself is `interactive_terminal_tui.rs`'s job.
///
/// Put a live, attached terminal behind `handle` with a screen exactly
/// `screen_rows` tall.
///
/// The explicit screen height is the point: passing `rows.len()` would make the
/// output fit exactly and leave *no* scrollback, so the tests that need history
/// to scroll back into would have none. Lines are joined CRLF with no trailing
/// newline, so screen row `n` is `rows[n]` until the screen overflows.
fn attach_terminal_sized(app: &mut App, handle: &str, rows: &[&str], screen_rows: u16) {
    use codegg::protocol::interactive_process::InteractiveOutputChunk;

    let rendered = rows.join("\r\n");
    let terminals = &mut app.interactive_terminals;
    assert!(terminals.apply_created(
        handle.to_string(),
        "ws-1".to_string(),
        "cat".to_string(),
        40,
        screen_rows.max(1)
    ));
    let chunk = InteractiveOutputChunk {
        handle: handle.to_string(),
        from_seq: 0,
        next_seq: rendered.len() as u64,
        gap: false,
        data_b64: B64.encode(rendered.as_bytes()),
    };
    assert!(terminals
        .apply_attached(handle, "att-1".to_string(), &chunk, None)
        .expect("attach")
        .is_none());
}

/// Screen sized to fit `rows` exactly — no trailing blank row, no scrollback.
fn attach_terminal(app: &mut App, handle: &str, rows: &[&str]) {
    attach_terminal_sized(app, handle, rows, u16::try_from(rows.len()).unwrap_or(1));
}

#[tokio::test(flavor = "current_thread")]
async fn d_deletes_the_selection_and_c_is_what_clears() {
    // `d` is `OpenDiff` globally and a focused panel is consulted before
    // Normal-mode actions, so the panel must claim `d` explicitly. It claims
    // it as the *delete operator*; `C` took over the clear it used to do.
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(&mut app, "t-nav", &["alpha", "bravo", "charlie", "delta"]);
    focus_terminal_panel(&mut app);

    // `x` takes the cursor's whole line, then `d` removes it.
    app.on_key(plain('x'));
    app.on_key(plain('d'));
    assert_eq!(
        app.interactive_terminals.redaction_count("t-nav"),
        1,
        "d must delete the held selection"
    );
    assert!(
        app.interactive_terminals.selection("t-nav").is_none(),
        "d is an operator: it consumes what it acted on"
    );

    app.on_key(plain('d'));
    assert_eq!(
        app.ui_state.dialog,
        codegg::tui::Dialog::None,
        "a bare d is a no-op, never the diff box"
    );

    // `C` is the clear.
    app.on_key(plain('C'));
    let remaining = app
        .interactive_terminals
        .render_lines("t-nav", 200)
        .expect("render")
        .join("\n")
        .trim()
        .to_string();
    assert!(
        remaining.is_empty(),
        "C must clear the screen, not leave the old output (got {remaining:?})"
    );
    assert_eq!(
        app.ui_state.dialog,
        codegg::tui::Dialog::None,
        "C must not open the diff box"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn x_selects_whole_lines_and_movement_alone_does_not_add_them() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(&mut app, "t-x", &["one", "two", "three", "four"]);
    focus_terminal_panel(&mut app);
    // Park the cursor on "two". The panel cursor starts on the last row, and
    // growth is capped at the last *visible* row, so starting there would test
    // the cap rather than the gesture.
    app.on_key(plain('k'));
    app.on_key(plain('k'));

    app.on_key(plain('x'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-x").as_deref(),
        Some("two")
    );

    // `j` is a movement. In line mode it must relocate the cursor and leave the
    // selection exactly one line wide — otherwise "press x again for the next
    // line" and "press x on the line I moved to" become the same key.
    app.on_key(plain('j'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-x").as_deref(),
        Some("two"),
        "a bare movement must not widen a line selection"
    );
    assert_eq!(
        app.interactive_terminals.cursor("t-x").map(|(_, col)| col),
        Some(0),
        "the cursor did move to the next line"
    );

    app.on_key(plain('x'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-x").as_deref(),
        Some("three"),
        "x after a move re-anchors on the cursor's line"
    );
    app.on_key(plain('x'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-x").as_deref(),
        Some("three\nfour")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn v_is_a_mode_and_x_then_v_extends_by_movement() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(
        &mut app,
        "t-v",
        &["cargo build --release", "second", "third"],
    );
    focus_terminal_panel(&mut app);
    // The panel cursor starts on the last row; these verbs are about the line
    // the reader is actually on, so park it on the first one.
    app.on_key(plain('k'));
    app.on_key(plain('k'));
    assert_eq!(
        app.interactive_terminals.cursor("t-v").map(|(row, _)| row),
        Some(0)
    );

    app.on_key(plain('v'));
    app.on_key(plain('l'));
    app.on_key(plain('l'));
    app.on_key(plain('l'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-v").as_deref(),
        Some("carg"),
        "in character mode movement extends the selection"
    );

    // `x` re-anchors as a whole line, whatever was selected before.
    app.on_key(plain('x'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-v").as_deref(),
        Some("cargo build --release"),
        "x ignores the cursor column and takes the whole line"
    );

    // `x` then `v`: movement now extends, because `v` re-armed character mode.
    // `v` re-anchors *at the cursor*, so walk the cursor to the line start
    // first — the whole line `x` took is not what `v` refines.
    app.on_key(plain('h'));
    app.on_key(plain('h'));
    app.on_key(plain('h'));
    assert_eq!(
        app.interactive_terminals.cursor("t-v").map(|(_, col)| col),
        Some(0)
    );
    app.on_key(plain('v'));
    app.on_key(plain('e'));
    app.on_key(plain('e'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-v").as_deref(),
        Some("cargo build"),
        "`e` after `x` then `v` extends the re-anchored selection by word"
    );

    // `v` again releases the mode; movement stops widening.
    app.on_key(plain('v'));
    assert!(app.interactive_terminals.selection("t-v").is_none());
    app.on_key(plain('b'));
    assert!(
        app.interactive_terminals.selection("t-v").is_none(),
        "movement after v was released must not re-arm it"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn word_motion_walks_the_cursor_without_widening_a_line_selection() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(
        &mut app,
        "t-w",
        &["cargo build --release", "second", "third"],
    );
    focus_terminal_panel(&mut app);
    app.on_key(plain('k'));
    app.on_key(plain('k'));

    app.on_key(plain('x'));
    let before = app.interactive_terminals.selected_text("t-w");

    app.on_key(plain('e'));
    app.on_key(plain('b'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-w"),
        before,
        "e/b are movements, and movements do not add to a line selection"
    );

    // In character mode the same keys do extend.
    app.on_key(plain('v'));
    app.on_key(plain('e'));
    app.on_key(plain('e'));
    assert_eq!(
        app.interactive_terminals.selected_text("t-w").as_deref(),
        Some("cargo build")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn scrolling_back_reaches_output_that_left_the_screen() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    let rows: Vec<String> = (0..40).map(|n| format!("page-{n:02}")).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    // A 10-row screen over 40 lines: 30 rows of history to scroll back into.
    attach_terminal_sized(&mut app, "t-scroll", &refs, 10);
    focus_terminal_panel(&mut app);

    assert!(
        visible(&mut app, "t-scroll").contains("page-39"),
        "the newest output is on screen while following"
    );
    assert!(
        !visible(&mut app, "t-scroll").contains("page-00"),
        "output that scrolled off is not on the live screen"
    );

    // `k` walks the cursor up one row at a time and the viewport follows it,
    // so enough presses to cross the whole retained history must reveal the
    // oldest output still in the buffer.
    for _ in 0..40 {
        app.on_key(plain('k'));
    }

    assert!(
        app.interactive_terminals
            .viewport_offset("t-scroll")
            .is_some_and(|offset| offset > 0),
        "k must move the viewport into the retained history"
    );
    assert!(
        visible(&mut app, "t-scroll").contains("page-00"),
        "the retained scrollback is what makes old output selectable"
    );
}

/// Select the Terminal panel and put the keyboard on it in Normal mode.
///
/// `choose_nth` alone is not enough for the navigation tests: it selects the
/// panel, but a panel is only consulted for keys when it is both focused *and*
/// in Normal mode — the contract that keeps a composing `q` typeable.
fn focus_terminal_panel(app: &mut App) {
    choose_nth(app, SidePanelId::Terminal.index());
    app.ui_state.input_mode = InputMode::Normal;
    assert!(
        app.panel_host.is_focused(),
        "the terminal panel must hold the keyboard for these keys to reach it"
    );
}

/// Everything the *viewport* currently shows for `handle`, as one string.
///
/// Reads through the selection-aware render path rather than `render_lines`:
/// only that path parks the scrollback offset on the emulator first, so this is
/// what actually tells the reader whether old output is reachable.
fn visible(app: &mut App, handle: &str) -> String {
    // One newline per *line*, not per span. A row's spans are split wherever
    // the style changes — and the cursor's REVERSED modifier splits the row
    // under it — so joining spans inserts a newline mid-row and makes a row
    // like "page-39" unreadable. That is a rendering artifact of this helper,
    // not what the user sees.
    app.interactive_terminals
        .render_view(handle, 200, ratatui::style::Style::default())
        .expect("render")
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "current_thread")]
async fn a_selection_survives_leaving_and_refocusing_the_panel() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(
        &mut app,
        "t-sel",
        &["first", "second", "third", "fourth", "fifth"],
    );
    focus_terminal_panel(&mut app);

    // The screen holds exactly the five lines given, so row 4 is "fifth" and
    // `x` on it must take that line whole — cursor column is irrelevant.
    app.on_key(plain('x'));
    let one = app
        .interactive_terminals
        .selected_text("t-sel")
        .expect("a line is selected");
    assert_eq!(one, "fifth");

    // The cursor is already on the last screen row, so `x` cannot grow further
    // and the selection correctly stays put rather than inventing an empty
    // line below the visible screen.
    app.on_key(plain('x'));
    assert_eq!(
        app.interactive_terminals
            .selected_text("t-sel")
            .expect("the selection grew"),
        "fifth",
        "growing at the last visible row must not select past the screen"
    );

    // Walk the cursor up two lines, then re-anchor with `x`.
    app.on_key(plain('k'));
    app.on_key(plain('k'));
    app.on_key(plain('x'));
    assert_eq!(
        app.interactive_terminals
            .selected_text("t-sel")
            .expect("a line is selected"),
        "third",
        "x selects the cursor's line, whatever the cursor column"
    );

    // Pressing `x` again grows the selection one line from the anchor — the
    // affordance behind "hit x repeatedly to take the next line".
    app.on_key(plain('x'));
    assert_eq!(
        app.interactive_terminals
            .selected_text("t-sel")
            .expect("the selection grew"),
        "third\nfourth"
    );

    // `v` re-anchors at character granularity from the current cursor, which is
    // how a reader switches granularity after `x` grabbed too much.
    app.on_key(plain('v'));
    let cell = app
        .interactive_terminals
        .selected_text("t-sel")
        .expect("a character selection");
    assert_eq!(
        cell.chars().count(),
        1,
        "v selects the single cell under the cursor, got {cell:?}"
    );
    assert!(
        "fourth".contains(cell.as_str()),
        "the cell must come from the cursor's row, got {cell:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn v_selects_characters_and_space_y_arms_only_a_prefix() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(&mut app, "t-char", &["abcdef", "ghijkl", "mnopqr"]);
    focus_terminal_panel(&mut app);

    // Cursor starts on the bottom row; `k` walks it up to "ghijkl", `h` takes
    // it to column 0, `v` anchors there, and two `l`s extend the selection.
    app.on_key(plain('k'));
    app.on_key(key(KeyCode::Char('h'), KeyModifiers::NONE));
    app.on_key(plain('v'));
    app.on_key(key(KeyCode::Char('l'), KeyModifiers::NONE));
    app.on_key(plain('l'));

    let selected = app
        .interactive_terminals
        .selected_text("t-char")
        .expect("a character selection");
    assert_eq!(
        selected, "ghi",
        "character selection stays inside the cursor's row"
    );

    // `space` alone arms nothing visible; the following `y` consumes the prefix.
    app.on_key(plain(' '));
    assert!(app.interactive_terminals.prefix_pending("t-char"));
    app.on_key(plain('y'));
    assert!(
        !app.interactive_terminals.prefix_pending("t-char"),
        "the yank must disarm the prefix"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn esc_drops_a_selection_before_it_leaves_the_panel() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(&mut app, "t-esc", &["one", "two", "three", "four"]);
    focus_terminal_panel(&mut app);

    app.on_key(plain('x'));
    assert!(app.interactive_terminals.selection("t-esc").is_some());

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));

    // The host claims Esc as the panel exit, so the selection must survive it:
    // the state lives on the controller, so stepping back out and returning
    // finds the text the reader was about to copy still selected.
    assert!(
        !app.panel_host.is_focused(),
        "Esc is the host's exit from a focused panel"
    );
    assert!(
        app.interactive_terminals.selection("t-esc").is_some(),
        "leaving the panel must not discard the selection"
    );

    focus_terminal_panel(&mut app);
    assert!(app.interactive_terminals.selection("t-esc").is_some());
}

// ── Panel hotkeys ───────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn a_digit_saved_in_the_manager_is_recalled_by_ctrl_digit() {
    let mut app = test_app();
    choose_nth(&mut app, 1); // Session -> Chat
    open_manager(&mut app); // reopen on Chat
    app.on_key(plain('1')); // bind
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.panel_hotkeys.get('1'), Some(SidePanelId::Chat));

    app.on_key(key(KeyCode::Char('1'), KeyModifiers::CONTROL));

    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Chat,
        "the saved key must swap the panel"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn reassigning_a_digit_rebinds_rather_than_failing() {
    let mut app = test_app();
    // Bind slot 3 to Chat, then rebind the same slot to Help. The manager
    // opens on the live panel each time, so walk from there.
    // Terminal, not Chat: an unbound chat panel refuses focus and would
    // leave the keyboard in the composer, which is a different test entirely.
    choose_by_label(&mut app, SidePanelId::Chat);
    // `Enter` above committed the panel and closed the manager; the digit
    // only binds while the manager is up.
    open_manager(&mut app);
    app.on_key(plain('3'));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.panel_hotkeys.get('3'), Some(SidePanelId::Chat));

    // The manager opens on the *live* panel, which is Chat by now — so
    // this cannot be an absolute index from the first entry.
    choose_by_label(&mut app, SidePanelId::Help);
    open_manager(&mut app);
    app.on_key(plain('3'));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.panel_hotkeys.get('3'),
        Some(SidePanelId::Help),
        "reusing a slot must replace it"
    );
    assert_eq!(
        app.panel_hotkeys.digit_for(SidePanelId::Chat),
        None,
        "the old binding must be gone"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_unbound_digit_is_consumed_so_it_cannot_reach_the_composer() {
    let mut app = test_app();
    app.on_key(key(KeyCode::Char('5'), KeyModifiers::CONTROL));
    assert!(
        app.prompt_state.prompt.get_text().is_empty(),
        "Ctrl+5 with nothing bound must not type or submit"
    );
}

#[test]
fn zero_is_a_valid_tenth_slot() {
    use codegg::tui::app::state::panel_hotkeys::{digit_slot, PanelHotkeys};
    let mut hotkeys = PanelHotkeys::default();
    hotkeys.set('0', SidePanelId::Terminal);
    assert_eq!(hotkeys.get('0'), Some(SidePanelId::Terminal));
    assert_eq!(
        digit_slot(key(KeyCode::Char('0'), KeyModifiers::CONTROL)),
        Some('0')
    );
}

#[test]
fn ctrl_digit_shaped_keys_are_recognised_and_other_keys_are_not() {
    use codegg::tui::app::state::panel_hotkeys::digit_slot;
    for digit in "1234567890".chars() {
        assert_eq!(
            digit_slot(key(KeyCode::Char(digit), KeyModifiers::CONTROL)),
            Some(digit)
        );
    }
    assert_eq!(
        digit_slot(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(digit_slot(plain('1')), None, "a bare digit is just text");
    assert_eq!(digit_slot(key(KeyCode::Char('1'), KeyModifiers::ALT)), None);
}

// ── Focus/mode coherence ────────────────────────────────────────────

/// Open a dockable box the way a user does, with the *composer* left in
/// Insert — the state the regression below needs.
///
/// The box's own `Esc` is sent first because `/help` opens in its Insert
/// mode, where the directional gesture is deliberately inert so `Shift+H`
/// stays a typeable letter. That `Esc` is consumed by the box, which is
/// exactly why the app-level mode is still Insert afterwards: the composer
/// never saw it. This is the precise state that used to strand the user.
///
/// The dock gesture that follows is `Shift+L` throughout: a box dock names
/// the side it should land on, and the panel is on the right by default. It
/// used to be `Shift+H` because *any* directional docked, which is exactly
/// what made "open a panel on the left" impossible to ask for.
fn open_help_box_while_composing(app: &mut App) {
    app.ui_state.input_mode = InputMode::Insert;
    for ch in "/help".chars() {
        app.on_key(plain(ch));
    }
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE)); // box: Insert -> Normal
    assert!(
        app.dock_target().is_some(),
        "precondition: a dockable box should be open"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Insert,
        "precondition: the composer is still in Insert behind the box"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn docking_a_box_leaves_a_panel_the_keyboard_can_actually_reach() {
    // Regression, and the reason the directional gesture has to work from
    // inside a panel at all.
    //
    // Docking ran `close_dialog()` — which parks the composer in Insert —
    // and then focused the panel. `App::on_key` consults a focused panel
    // *only* in Normal mode, so the app sat in a state where the badge and
    // the hint line both said the panel had the keyboard while every key,
    // including `Shift+H`, the only way out, went to the composer and was
    // typed as text. Focus and mode now agree by construction.
    let mut app = test_app();
    open_help_box_while_composing(&mut app);

    app.on_key(key(KeyCode::Char('L'), KeyModifiers::SHIFT));
    assert_eq!(
        app.panel_host.active(),
        SidePanelId::Help,
        "the gesture docks the open box into its panel"
    );
    assert!(
        app.panel_host.is_focused(),
        "precondition: the panel holds the keyboard after docking"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Normal,
        "a focused panel is a Normal-mode surface"
    );

    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
    assert!(
        !app.panel_host.is_focused(),
        "Shift+H must return focus to the main buffer"
    );
    assert_eq!(
        app.composer_destination_title(),
        "LLM",
        "the composer must be aimed at the main buffer again"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn leaving_a_panel_restores_the_composer_mode_it_had() {
    // Taking focus parks the composer's mode rather than discarding it, so
    // stepping back out of a panel does not silently change how the next
    // keystroke behaves. Docking is the route that carries a composer in
    // Insert into a focused panel, so that is the round trip under test.
    let mut app = test_app();
    open_help_box_while_composing(&mut app);

    app.on_key(key(KeyCode::Char('L'), KeyModifiers::SHIFT));
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Normal,
        "precondition: the panel is a Normal-mode surface"
    );

    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
    assert!(
        !app.panel_host.is_focused(),
        "the second gesture leaves the panel"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Insert,
        "the composer's own mode must survive the round trip"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_normal_mode_composer_is_not_rewritten_by_a_panel_visit() {
    // The other direction: a user who was reading in Normal — rather than
    // mid-sentence — comes back to Normal, not Insert.
    let mut app = test_app();
    open_help_box_while_composing(&mut app);
    app.ui_state.input_mode = InputMode::Normal;

    app.on_key(key(KeyCode::Char('L'), KeyModifiers::SHIFT));
    assert!(
        app.panel_host.is_focused(),
        "precondition: docked + focused"
    );
    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));

    assert!(
        !app.panel_host.is_focused(),
        "the second gesture leaves the panel"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Normal,
        "Normal must round-trip unchanged too"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn leaving_a_panel_with_escape_also_restores_the_composer_mode() {
    // `Esc` blurs through the host rather than through `blur_panel`, so it
    // owes the composer the same restoration. Without this the user is
    // dropped into Normal whatever they were doing before.
    let mut app = test_app();
    open_help_box_while_composing(&mut app);

    app.on_key(key(KeyCode::Char('L'), KeyModifiers::SHIFT));
    assert!(
        app.panel_host.is_focused(),
        "precondition: docked + focused"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Normal,
        "precondition: the panel is a Normal-mode surface"
    );

    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.panel_host.is_focused(), "Esc leaves the panel");
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Insert,
        "Esc must hand back the composer's own mode too"
    );
}

// ── Hosted panels: the generalisation, not the original three ─────────

/// Every plain viewer box docks into its panel from one shared gesture.
///
/// `/chat`, `/terminal` and `/help` each carry their own handler, so a test
/// over only those proves nothing about the 28 panels added later. These are
/// the boxes that rely on the *central* gate in `handle_dialog_key`: none of
/// them knows the gesture exists, and all of them must still land correctly.
#[tokio::test(flavor = "current_thread")]
async fn every_viewer_box_docks_from_the_one_shared_gesture() {
    // Only boxes with a public door are listed. `/logs`, `/tasks`, `/goal`,
    // `/memory-search`, `/collaborators` and `/doctor` are produced by
    // `open_info_dialog` / dedicated helpers rather than by `open_dialog`,
    // so a test cannot mount them without reaching into `dialog_state`.
    // `Theme`/`Mcp`/`Keybind` are here alongside the `InfoDialog`-backed ones
    // precisely so the gate is proven for a *focused* dialog too, not just
    // the one widget that covers eleven panels.
    let cases = [
        (codegg::tui::Dialog::Cost, SidePanelId::Cost),
        (codegg::tui::Dialog::Usage, SidePanelId::Usage),
        (codegg::tui::Dialog::Context, SidePanelId::Context),
        (codegg::tui::Dialog::Theme, SidePanelId::Themes),
        (codegg::tui::Dialog::Mcp, SidePanelId::Mcp),
        (codegg::tui::Dialog::Keybind, SidePanelId::Keybinds),
        // `/files` is here for the same reason as the rest: it reports
        // `docks_as_viewer`, so `Shift+L` must move it into the column rather
        // than reaching the widget as a stray `L`.
        (codegg::tui::Dialog::Files, SidePanelId::Files),
    ];

    for (dialog, expected) in cases {
        let mut app = test_app();
        app.open_dialog(dialog.clone());
        assert!(
            app.focus_manager.active_dialog_type() != DialogType::None,
            "{dialog:?} must actually have mounted a box"
        );

        app.on_key(plain('L'));

        assert_eq!(
            app.focus_manager.active_dialog_type(),
            DialogType::None,
            "{dialog:?} must be closed after docking"
        );
        assert_eq!(
            app.panel_host.active(),
            expected,
            "{dialog:?} must land on {expected:?}"
        );
        assert!(
            app.panel_host.is_focused(),
            "{dialog:?} must hand focus to the panel that took it"
        );
    }
}

/// The docked panel keeps the box it was given rather than restarting empty.
///
/// This is the whole point of adopting the unmounted component: the column
/// starts showing the same surface the box was showing. Without it, "docking"
/// would be a re-open that happens to look similar — the exact failure the
/// chat box/panel pair is documented as having had.
#[tokio::test(flavor = "current_thread")]
async fn a_docked_panel_adopts_the_box_it_was_given() {
    let mut app = test_app();
    app.open_dialog(codegg::tui::Dialog::Cost);
    assert!(app.focus_manager.has_dialog(DialogType::Cost));

    app.on_key(plain('L'));

    let active = app.panel_host.active();
    assert_eq!(active, SidePanelId::Cost);
    assert!(
        app.panel_host
            .hosted_panel_has_content(active)
            .expect("Cost is a hosted panel"),
        "docking must install the box, not an empty placeholder"
    );
}

/// A box with no panel counterpart must still refuse the gesture.
///
/// The negative half of `dock_target`. If a box with no column to move into
/// quietly selected a panel, `L` in some unrelated box would appear to do
/// something the user never asked for.
#[tokio::test(flavor = "current_thread")]
async fn a_box_with_no_panel_refuses_the_gesture_instead_of_guessing() {
    let mut app = test_app();
    // `/share` is a one-shot action with no side-panel counterpart.
    app.open_dialog(codegg::tui::Dialog::Share);
    let before = app.panel_host.active();
    app.on_key(plain('L'));
    assert_eq!(
        app.panel_host.active(),
        before,
        "a box with no panel must not move anything"
    );
}

// ── Single-step selection: the manager loads the panel ───────────────

/// Picking an empty hosted panel from the manager fills it in one step.
///
/// Before the single-step loader, `Shift+S` + `Enter` on an empty panel
/// landed on a placeholder that refused focus — the panel "did not work"
/// while Chat/Help (bespoke, always live) did. Now the selection opens
/// the panel's box through the same seam as its `/` command and docks it.
#[tokio::test(flavor = "current_thread")]
async fn selecting_an_empty_sync_panel_materializes_and_focuses_it() {
    for id in [
        SidePanelId::Tree,
        SidePanelId::Context,
        SidePanelId::Logs,
        SidePanelId::Review,
    ] {
        let mut app = test_app();
        app.ui_state.sidebar_visible = true;
        to_normal_mode(&mut app);
        assert_eq!(
            app.panel_host.hosted_panel_has_content(id),
            Some(false),
            "{id:?} precondition: nothing adopted yet"
        );
        choose_by_label(&mut app, id);
        assert_eq!(app.panel_host.active(), id, "{id:?} must be loaded");
        assert!(
            app.panel_host
                .hosted_panel_has_content(id)
                .expect("hosted panel"),
            "{id:?} must hold the box it opened, not a placeholder"
        );
        assert!(
            app.panel_host.is_focused(),
            "{id:?} must take focus like Chat/Help do"
        );
    }
}

/// Panels with genuinely nothing to show stay on their placeholder.
///
/// A plan panel with no pending confirmation and a security panel with no
/// receipt have no box to open. Selecting them must not invent one, and
/// must not strand the keyboard: focus stays in the composer.
#[tokio::test(flavor = "current_thread")]
async fn panels_with_no_content_keep_their_placeholder_and_the_composer() {
    for id in [SidePanelId::Plan, SidePanelId::Security] {
        let mut app = test_app();
        app.ui_state.sidebar_visible = true;
        to_normal_mode(&mut app);
        choose_by_label(&mut app, id);
        assert_eq!(app.panel_host.active(), id);
        assert_eq!(
            app.panel_host.hosted_panel_has_content(id),
            Some(false),
            "{id:?} must stay empty rather than invent a box"
        );
        assert!(
            !app.panel_host.is_focused(),
            "{id:?} must refuse focus so keys stay with the composer"
        );
    }
}

/// A live-loading box stays open instead of docking frozen.
///
/// Research opens its browser and *then* fetches the run list. Adopting
/// the loading spinner would freeze it there (and cancel the fetch), so
/// the selection leaves the box live and keeps the current panel. `L`
/// docks it once it has something to show.
#[tokio::test(flavor = "current_thread")]
async fn a_live_loading_box_stays_open_instead_of_docking_frozen() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    to_normal_mode(&mut app);
    let before = app.panel_host.active();
    choose_by_label(&mut app, SidePanelId::Research);
    assert!(
        app.focus_manager.has_dialog(DialogType::ResearchBrowser),
        "the research box must be open and live"
    );
    assert_eq!(
        app.panel_host.active(),
        before,
        "the panel must not switch to a box that has not loaded"
    );
    assert_eq!(
        app.panel_host
            .hosted_panel_has_content(SidePanelId::Research),
        Some(false),
        "nothing is adopted until the user docks the loaded box"
    );
}

/// `/tasks` without a project has no box to open, so the panel waits.
///
/// `start_tasks_command` falls back to the async schedule list, which
/// toasts and opens nothing synchronously. The selection must land on
/// the placeholder (naming the loader) rather than on a stray surface.
#[tokio::test(flavor = "current_thread")]
async fn tasks_without_a_project_lands_on_its_placeholder() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    to_normal_mode(&mut app);
    choose_by_label(&mut app, SidePanelId::Tasks);
    assert_eq!(app.panel_host.active(), SidePanelId::Tasks);
    assert_eq!(
        app.panel_host.hosted_panel_has_content(SidePanelId::Tasks),
        Some(false)
    );
    assert!(!app.panel_host.is_focused());
}

// ── Panel resize mode ─────────────────────────────────────────────────

/// Arm resize mode the way a reader does: `Ctrl+N` in Normal mode.
fn arm_resize(app: &mut App) {
    app.on_key(key(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert!(app.panel_host.is_resizing(), "Ctrl+N arms resize mode");
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_n_arms_resize_mode_and_new_session_is_now_a_bare_n() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.ui_state.input_mode = InputMode::Normal;

    // The chord is the resize toggle, not a second spelling of new session.
    arm_resize(&mut app);
    assert!(
        app.panel_host.is_resizing(),
        "Ctrl+N no longer starts a session"
    );

    // And the bare letter still does, so nothing was lost by the remap.
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Normal;
    app.on_key(plain('n'));
    assert!(
        !app.panel_host.is_resizing(),
        "a bare n is NewSession, not the resize toggle"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn resize_mode_stays_armed_until_esc_enter_or_the_chord() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.ui_state.input_mode = InputMode::Normal;

    for exit in [
        key(KeyCode::Esc, KeyModifiers::NONE),
        key(KeyCode::Enter, KeyModifiers::NONE),
        key(KeyCode::Char('n'), KeyModifiers::CONTROL),
    ] {
        arm_resize(&mut app);
        app.on_key(exit);
        assert!(
            !app.panel_host.is_resizing(),
            "{exit:?} must leave resize mode"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn h_on_a_right_panel_widens_it_and_l_is_a_no_op() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.ui_state.input_mode = InputMode::Normal;
    // Terminal, not Chat: an unbound chat panel refuses focus and would
    // leave the keyboard in the composer, which is a different test entirely.
    choose_by_label(&mut app, SidePanelId::Terminal);
    let before = app
        .ui_state
        .layout
        .config
        .column_width(codegg::tui::panels::PanelPlacement::Right);

    arm_resize(&mut app);
    app.on_key(plain('h'));
    assert_eq!(
        app.ui_state
            .layout
            .config
            .column_width(codegg::tui::panels::PanelPlacement::Right),
        before + 1,
        "h drags a right panel's left border out, so the column grows"
    );

    // A right panel has no right border of its own — the frame owns it — so
    // `l` must do nothing at all rather than shrinking the panel.
    app.on_key(plain('l'));
    assert_eq!(
        app.ui_state
            .layout
            .config
            .column_width(codegg::tui::panels::PanelPlacement::Right),
        before + 1,
        "a frame edge is not draggable; resize is outward-only"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn l_from_the_main_buffer_narrows_the_chat_column() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.ui_state.input_mode = InputMode::Normal;
    // Terminal, not Chat: an unbound chat panel refuses focus and would
    // leave the keyboard in the composer, which is a different test entirely.
    choose_by_label(&mut app, SidePanelId::Terminal);
    let before = app
        .ui_state
        .layout
        .config
        .column_width(codegg::tui::panels::PanelPlacement::Right);

    // Hand the keyboard back to the transcript. The chat column and the buffer
    // share one border, and `l` must mean opposite things on each side of it.
    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
    assert!(!app.panel_host.is_focused());
    // Leaving a panel restores whatever mode the composer had before it took
    // focus, so a Normal-mode frame gesture needs Normal mode stated again.
    app.ui_state.input_mode = InputMode::Normal;

    arm_resize(&mut app);
    app.on_key(plain('l'));
    assert_eq!(
        app.ui_state
            .layout
            .config
            .column_width(codegg::tui::panels::PanelPlacement::Right),
        before - 1,
        "growing the buffer pushes the column's left border out from under it"
    );

    // Nothing is docked on the left, so `h` from the buffer has no panel to
    // push and must be inert.
    app.on_key(plain('h'));
    assert_eq!(
        app.ui_state
            .layout
            .config
            .column_width(codegg::tui::panels::PanelPlacement::Left),
        app.ui_state.layout.config.sidebar_width
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hjkl_do_not_navigate_while_resize_mode_is_armed() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(&mut app, "t-resize", &["alpha", "bravo", "charlie"]);
    focus_terminal_panel(&mut app);
    let parked = app
        .interactive_terminals
        .cursor("t-resize")
        .expect("cursor");

    arm_resize(&mut app);
    for ch in ['h', 'j', 'k', 'l'] {
        app.on_key(plain(ch));
    }

    assert_eq!(
        app.interactive_terminals
            .cursor("t-resize")
            .expect("cursor"),
        parked,
        "a focused panel consumes every key it is shown, so resize mode has \
         to be answered ahead of the panel or h/j/k/l would move the cursor"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hjkl_navigate_again_once_resize_mode_is_left() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    attach_terminal(&mut app, "t-resize", &["alpha", "bravo", "charlie"]);
    focus_terminal_panel(&mut app);
    let before = app
        .interactive_terminals
        .cursor("t-resize")
        .expect("cursor");

    arm_resize(&mut app);
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    app.on_key(plain('k'));

    assert_ne!(
        app.interactive_terminals
            .cursor("t-resize")
            .expect("cursor"),
        before,
        "the keys must mean navigation again"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn resize_mode_is_a_normal_mode_gesture() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.ui_state.input_mode = InputMode::Insert;

    // Resize steals `h/j/k/l`, which in Insert are ordinary letters. It is a
    // frame gesture for the same reason the panel manager is Normal-only: a
    // frame-level mode that could be armed mid-sentence would swallow the next
    // four letters of whatever was being typed. `Ctrl+N` arriving here at all
    // is harmless — it is a chord, so it is never text — but it must not arm a
    // mode that then eats the prompt.
    app.on_key(key(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert!(
        !app.panel_host.is_resizing(),
        "resize mode is Normal-only; Insert keeps its letters"
    );
}

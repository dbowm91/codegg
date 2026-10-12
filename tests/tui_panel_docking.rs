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
/// `Shift+S` is the only spelling of the manager and it is Normal-mode
/// only. The mode is stated directly rather than reached through `Esc`: on
/// the Workspace route `Esc` belongs to that view, and routing through it
/// would make these tests depend on a different surface's key handling.
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

/// Open the manager and commit the panel `n` rows below the live one.
///
/// Deliberately counts presses rather than looping until the active panel
/// matches: the highlight moves while the manager is open but the *active*
/// panel does not, so a "until" loop here never terminates.
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
    choose_nth(&mut app, SidePanelId::Help.index());
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
    choose_nth(&mut app, SidePanelId::Help.index());
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
    choose_nth(&mut app, SidePanelId::Chat.index());
    open_manager(&mut app);
    app.on_key(plain('3'));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.panel_hotkeys.get('3'), Some(SidePanelId::Chat));

    choose_nth(&mut app, SidePanelId::Help.index());
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

    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
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

    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
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

    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
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

    app.on_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
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

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

fn manager() -> KeyEvent {
    key(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    )
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
    app.on_key(manager());
    manager_down(app, n);
    app.on_key(key(KeyCode::Enter, KeyModifiers::NONE));
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
async fn the_terminal_panel_refuses_focus_until_it_has_a_handle() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    // Select it the way a user does. No terminal exists yet, so the panel has
    // nothing to show.
    choose_nth(&mut app, SidePanelId::Terminal.index());
    assert_eq!(app.panel_host.active(), SidePanelId::Terminal);
    assert!(
        !app.panel_host.is_focused(),
        "a panel with nothing to show must not take the keyboard, or it would \
         swallow every keystroke"
    );
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
    app.on_key(manager()); // reopen on Chat
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
    app.on_key(manager());
    app.on_key(plain('3'));
    app.on_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.panel_hotkeys.get('3'), Some(SidePanelId::Chat));

    choose_nth(&mut app, SidePanelId::Help.index());
    app.on_key(manager());
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

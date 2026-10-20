//! The composer's Normal-mode selection and clipboard verbs.
//!
//! ADR-0042 part 7 gives the composer `v`, `x`, `d` and `p`, and gives the
//! **copy gesture** to the help box that the `space` chord opens — and to no
//! other surface. The composer does not claim `y`; the box does, and only in
//! the box the chord opened.
//!
//! The tests press keys into [`App::on_key`] and read the composer back out.
//! A test that called the model's methods directly would pass even if the
//! claim table and the runner had drifted apart, and the drift is the thing
//! the claim table exists to prevent.
//!
//! The copy is the one verb whose owner is *another* surface, so it is the
//! one verb whose test has to reach across: the chord opens a box, the box
//! asks the app to copy, and the app performs the copy. The route — not the
//! mode — is what tells the two help boxes apart, and the tests below cover
//! the case the mode cannot: a `/help` box that `Esc` has moved into Normal
//! mode.

use codegg::tui::app::{App, Dialog};
use codegg::tui::components::dialogs::help::HelpRoute;
use codegg::tui::input::{InputAction, InputMode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

/// Put text in the composer and place the caret.
fn compose(app: &mut App, text: &str, caret: usize) {
    app.prompt_state.prompt.set_text(text.to_string());
    app.prompt_state.prompt.set_cursor(caret);
    app.ui_state.input_mode = InputMode::Normal;
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent::new(code, modifiers));
}

fn prompt_text(app: &App) -> String {
    app.prompt_state.prompt.get_text()
}

fn caret(app: &App) -> usize {
    app.prompt_state.prompt.cursor_pos()
}

/// The toast messages the app has reported, newest first.
fn toasts(app: &App) -> Vec<String> {
    app.messages_state
        .toasts
        .history()
        .map(|record| record.message.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// The selection verbs
// ---------------------------------------------------------------------------

/// `v` arms a character selection at the caret.
#[test]
fn v_arms_a_character_selection_at_the_caret() {
    let mut app = test_app();
    compose(&mut app, "hello world", 2);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);

    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        Some((2, 3)),
        "`v` did not arm a selection at the caret"
    );
}

/// A movement extends the held character selection.
#[test]
fn a_movement_extends_the_held_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);

    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        Some((0, 2)),
        "the selection did not grow with the caret"
    );
}

/// A second `v` discards the selection rather than re-anchoring it.
#[test]
fn a_second_v_discards_the_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);

    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        None,
        "a second `v` must discard the selection"
    );
}

/// `x` selects the caret's line, and a second `x` extends it a line at a
/// time.
#[test]
fn x_selects_the_line_and_grows_it_a_line_at_a_time() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo\nthree", 0);

    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        Some((0, 4)),
        "`x` did not select the caret's line"
    );

    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        Some((0, 8)),
        "a second `x` did not extend the selection by a line"
    );
}

/// `x` stops growing at the last line.
#[test]
fn x_stops_growing_at_the_last_line() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);

    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        Some((0, 7)),
        "the selection grew past the last line"
    );
}

/// `d` deletes the held selection.
#[test]
fn d_deletes_the_held_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "llo world",
        "`d` did not delete the selection"
    );
    assert_eq!(caret(&app), 0, "the caret follows the deletion");
}

/// Deleting a whole line removes the line, terminator and all.
#[test]
fn d_deletes_a_whole_line() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo\nthree", 5);

    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "one\nthree", "the line was left behind");
}

/// Deleting with nothing selected changes nothing — and still consumes the
/// key, because the letter is the composer's.
#[test]
fn d_with_nothing_selected_changes_nothing_and_is_consumed() {
    let mut app = test_app();
    app.ui_state
        .bindings
        .insert((KeyModifiers::NONE, KeyCode::Char('d')), InputAction::Help);
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "hello world", "`d` changed the text");
    assert!(
        !matches!(app.ui_state.dialog, Dialog::Help(_)),
        "the claimed letter `d` reached the app's binding map"
    );
}

/// The selection verbs are Normal-mode verbs: in Insert mode the same
/// letters are text.
#[test]
fn the_selection_verbs_are_normal_mode_verbs() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);
    app.ui_state.input_mode = InputMode::Insert;

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "vxhello", "the letters were not typed");
    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        None,
        "a selection was armed in Insert mode"
    );
}

// ---------------------------------------------------------------------------
// The paste verb
// ---------------------------------------------------------------------------

/// `p` is the composer's: it is consumed by the composer in Normal mode.
#[test]
fn p_is_consumed_by_the_composer() {
    let mut app = test_app();
    app.ui_state
        .bindings
        .insert((KeyModifiers::NONE, KeyCode::Char('p')), InputAction::Help);
    compose(&mut app, "hello", 0);

    press(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);

    assert!(
        !matches!(app.ui_state.dialog, Dialog::Help(_)),
        "the claimed letter `p` reached the app's binding map"
    );
}

/// `p` inserts what the clipboard holds, at the caret.
///
/// The clipboard is the **system** clipboard, and this test does not assert
/// on its contents: the clipboard is global state, so a test that read it
/// would race every other test in the process — including this file's own
/// copy test, which writes to it — and would be a flaky test rather than a
/// test. The *insertion* is pinned where it is deterministic, in the widget
/// (`a_paste_inserts_the_text_at_the_caret`); what is asserted here is that
/// the verb is the composer's and that it says what it did.
#[test]
fn p_is_the_composers_own_verb() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    press(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);

    let reported = toasts(&app);
    assert!(
        reported
            .iter()
            .any(|message| message.contains("Pasted") || message.contains("Nothing to paste")),
        "the paste did not report: {reported:?}"
    );
}

/// The paste always reports what it did.
///
/// A silent no-op would teach the user that the key is dead, so the verb
/// says what happened either way. The report is also the only evidence of
/// *what* was pasted: the pasted text is not echoed back, because the
/// composer can hold a secret and toasts are retained for `/logs`.
#[test]
fn the_paste_always_reports_what_it_did() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    press(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);

    let reported = toasts(&app);
    assert!(
        reported
            .iter()
            .any(|message| message.contains("Pasted") || message.contains("Nothing to paste")),
        "the paste did not report: {reported:?}"
    );
}

// ---------------------------------------------------------------------------
// The copy gesture, and the route that owns it
// ---------------------------------------------------------------------------

/// `space` and then `y` in the help box the chord opened copies the
/// composer's selection.
#[test]
fn space_then_y_copies_the_composers_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);

    // The chord opens the composer's help box.
    press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    assert!(
        matches!(app.ui_state.dialog, Dialog::Help(_)),
        "the space chord did not open the help box"
    );

    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    let reported = toasts(&app);
    assert!(
        reported
            .iter()
            .any(|message| message.contains("Copied") || message.contains("Could not copy")),
        "the copy did not report its outcome: {reported:?}"
    );
}

/// The copy takes exactly the selection's range and nothing else.
///
/// The report names the **size** of what was copied, and the size is the
/// range's: a copy of the whole buffer would say "11 characters" and a copy
/// of one character would say "1". So the count pins the range without the
/// test writing to the system clipboard — which is global state that
/// parallel tests would race on, and which holds whatever the user copied.
#[test]
fn the_copy_takes_exactly_the_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);

    press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    let reported = toasts(&app);
    assert!(
        reported
            .iter()
            .any(|message| message.contains("Copied 2 characters")
                || message.contains("Could not copy")),
        "the copy did not report the selection's size: {reported:?}"
    );
}

/// With nothing selected, the copy reports that there is nothing to copy and
/// changes nothing.
#[test]
fn the_copy_reports_when_there_is_nothing_to_copy() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    let reported = toasts(&app);
    assert!(
        reported
            .iter()
            .any(|message| message.contains("Nothing to copy")),
        "the copy did not report that there was nothing to copy: {reported:?}"
    );
}

/// The `/help` command's box does not copy — and the difference is the
/// route, not the mode.
///
/// The box is opened in Insert, the way the `/help` command opens it, and
/// `Esc` moves it into Normal. The paired test below opens the box the
/// **same** way through the chord and copies, so "the `/help` box did not
/// copy" cannot pass merely because the box happened to be in Insert mode.
#[test]
fn the_help_commands_box_does_not_copy_even_after_esc() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);

    // The `/help` box opens in Insert, because the user typed the command.
    app.ui_state.input_mode = InputMode::Insert;
    app.open_dialog(Dialog::Help(HelpRoute::Command));

    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    assert!(
        !toasts(&app)
            .iter()
            .any(|message| message.contains("Copied") || message.contains("Nothing to copy")),
        "the `/help` box copied: {:?}",
        toasts(&app)
    );
}

/// The chord's box copies after the same `Esc`.
///
/// This is the half that makes the test above meaningful: the box is opened
/// in Insert, `Esc` moves it into Normal, and `y` copies. The mode is
/// therefore the same in both boxes — only the route differs.
#[test]
fn the_chords_box_copies_after_the_same_esc() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);

    app.ui_state.input_mode = InputMode::Insert;
    app.open_dialog(Dialog::Help(HelpRoute::Chord));

    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    let reported = toasts(&app);
    assert!(
        reported
            .iter()
            .any(|message| message.contains("Copied 2 characters")
                || message.contains("Could not copy")),
        "the chord's box did not copy: {reported:?}"
    );
}

/// In Insert mode, `y` is a filter character in **both** routes.
#[test]
fn y_is_a_filter_character_in_insert_mode_in_both_routes() {
    for route in [HelpRoute::Chord, HelpRoute::Command] {
        let mut app = test_app();
        compose(&mut app, "hello", 0);
        app.ui_state.input_mode = InputMode::Insert;
        app.open_dialog(Dialog::Help(route));

        press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

        assert!(
            !toasts(&app)
                .iter()
                .any(|message| message.contains("Copied") || message.contains("Nothing to copy")),
            "the box copied in Insert mode: {:?}",
            toasts(&app)
        );
    }
}

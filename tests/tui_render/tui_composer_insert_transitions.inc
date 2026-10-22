//! The composer's insert transitions: `a` and `o`.
//!
//! M006 made `a` (insert after the caret) and `o` (open a line below) live.
//! The caret placement each transition implies lives in the **model**, not in
//! the app — so the tests here go through the key chain (`App::on_key`),
//! because the dispatch is what M006 changed.
//!
//! The two verbs differ in one way that matters: `a` is a **movement** (the
//! buffer's text does not change, so there is nothing to undo), and `o` is an
//! **edit** (the buffer gains a line, so `u` takes it back).

use codegg::tui::app::App;
use codegg::tui::input::InputMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

/// Put text in the composer and place the caret, in Normal mode.
fn compose(app: &mut App, text: &str, caret: usize) {
    app.prompt_state.prompt.set_text(text.to_string());
    app.prompt_state.prompt.set_cursor(caret);
    app.ui_state.input_mode = InputMode::Normal;
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent::new(code, modifiers));
}

fn caret(app: &App) -> usize {
    app.prompt_state.prompt.cursor_pos()
}

fn prompt_text(app: &App) -> String {
    app.prompt_state.prompt.get_text()
}

// ---------------------------------------------------------------------------
// `a` — insert after the caret
// ---------------------------------------------------------------------------

/// `a` moves the caret one character forward and enters Insert mode.
#[test]
fn a_moves_the_caret_after_the_character_and_enters_insert_mode() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);

    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        1,
        "`a` did not move the caret past the character"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Insert,
        "`a` did not enter insert mode"
    );
}

/// `a` at the end of a line stays on that line.
///
/// Opening a line below is `o`'s job. Two verbs that did the same thing would
/// make one of them wrong.
#[test]
fn a_stays_on_its_own_line() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        1,
        "`a` should have moved one character forward"
    );
    assert!(
        prompt_text(&app).starts_with("one"),
        "`a` changed the text: {:?}",
        prompt_text(&app)
    );
}

/// `a` at the end of the last character leaves the caret at the line's end.
#[test]
fn a_at_the_end_of_a_line_leaves_the_caret_at_the_line_end() {
    let mut app = test_app();
    compose(&mut app, "hello", 4);

    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        5,
        "`a` should have left the caret at the line end"
    );
}

/// `a` on the last line of a multi-line buffer does not run into the next
/// line.
#[test]
fn a_does_not_run_into_the_next_line() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 2);

    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        3,
        "`a` should have stopped at the end of the first line"
    );
}

/// `a` is a movement, so it changes no text and records nothing.
#[test]
fn a_changes_no_text_and_records_nothing() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);

    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "hello", "`a` changed the text");

    // Nothing was recorded, so `u` has nothing to undo.
    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(
        prompt_text(&app),
        "hello",
        "`u` undid something `a` never did"
    );
}

/// `a` extends a held character selection.
///
/// The move is a *deliberate* movement, so it goes through the shared
/// machine's movement path — the same path the motion verbs use, and the one
/// that grows a selection. A re-anchored caret would throw the selection
/// away.
#[test]
fn a_extends_a_held_character_selection() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    let range = app
        .prompt_state
        .prompt
        .selection_range()
        .expect("`a` threw the held selection away");
    assert_eq!(
        range,
        (0, 2),
        "`a` did not extend the held character selection"
    );
}

// ---------------------------------------------------------------------------
// `o` — open a line below
// ---------------------------------------------------------------------------

/// `o` opens an empty line below the caret's line and enters Insert mode.
#[test]
fn o_opens_a_line_below_and_enters_insert_mode() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    press(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "one\n\ntwo",
        "`o` did not open a line below"
    );
    assert_eq!(caret(&app), 4, "`o` did not put the caret in the new line");
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Insert,
        "`o` did not enter insert mode"
    );
}

/// `o` on the last line opens a line below it.
#[test]
fn o_on_the_last_line_opens_a_line_below() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 4);

    press(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "one\ntwo\n",
        "`o` did not open a line below"
    );
    assert_eq!(caret(&app), 8, "`o` did not put the caret in the new line");
}

/// `o` keeps the caret's line's text.
///
/// The newline goes at the end of the line's *content*, so the line's own
/// terminator stays where it was and the new empty line is the one below.
#[test]
fn o_keeps_the_carets_line_text() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    press(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);

    let text = prompt_text(&app);
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines[0], "one", "the caret's line lost its text");
    assert_eq!(lines[1], "", "the new line is not empty");
    assert_eq!(lines[2], "two", "the line below moved");
}

/// `o` on a single-line buffer creates the second line.
///
/// `o` carries no multi-line condition: it *creates* the second line, so the
/// condition that gates `j` and `k` does not apply to it.
#[test]
fn o_on_a_single_line_buffer_creates_the_second_line() {
    let mut app = test_app();
    compose(&mut app, "one", 0);

    press(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "one\n", "`o` did not open a line below");
    assert_eq!(caret(&app), 4, "`o` did not put the caret in the new line");
}

/// `o` is an edit, so `u` takes the new line back.
///
/// This is the whole difference between `o` and `a`: `a` moves the caret and
/// changes nothing, `o` changes the buffer and is undoable.
#[test]
fn o_is_undoable() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 0);

    press(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "one\n\ntwo");

    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "one\ntwo",
        "`u` did not take the opened line back"
    );
}

// ---------------------------------------------------------------------------
// The claims still hold
// ---------------------------------------------------------------------------

/// The insert transitions are Normal-mode verbs: in Insert mode the same
/// letters are text.
#[test]
fn the_insert_transitions_are_normal_mode_verbs() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);
    app.ui_state.input_mode = InputMode::Insert;

    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "ahello",
        "in Insert mode `a` must be typed, not run as a transition"
    );
}

/// A claimed letter is consumed: `a` never reaches an app-level action.
///
/// This is the half of the D6 re-spelling that matters at runtime. `a` used
/// to be the vim map's `FocusSidebar`; now the composer owns it, and the
/// action is reachable by its chord instead.
#[test]
fn the_insert_after_verb_does_not_reach_an_app_level_action() {
    let mut app = test_app();
    app.ui_state.bindings.insert(
        (KeyModifiers::NONE, KeyCode::Char('a')),
        codegg::tui::input::InputAction::FocusSidebar,
    );
    compose(&mut app, "hello", 0);

    press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        1,
        "the claimed letter `a` reached the app's binding map"
    );
    assert_eq!(
        app.ui_state.input_mode,
        InputMode::Insert,
        "`a` did not run the composer's transition"
    );
}

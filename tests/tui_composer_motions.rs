//! The composer's Normal-mode motion verbs.
//!
//! ADR-0042 gave the composer `w`, `b`, `e`, `j` and `k`, and M004 made them
//! live. The verbs move the caret; the *point* of the milestone is that they
//! move it the way the shared geometry says, and that they do it through the
//! key chain rather than through a helper the tests call directly.
//!
//! Every test here presses a key into [`App::on_key`] and reads the caret back
//! out. A test that called `ComposerText::move_word_forward` directly would
//! pass even if the claim table and the runner had drifted apart — the
//! dispatch *is* the thing M004 changed, so the dispatch is what the test
//! exercises.
//!
//! The offsets the tests expect are the ones `codegg::tui::text_geometry`
//! computes. The composer is not allowed a second opinion about where a word
//! is: the editor uses the same functions, and a composer that disagreed with
//! it would be two surfaces answering "where is the next word" differently.

use codegg::tui::app::App;
use codegg::tui::input::{InputAction, InputMode};
use codegg::tui::text_geometry;
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

/// The caret's byte offset.
fn caret(app: &App) -> usize {
    app.prompt_state.prompt.cursor_pos()
}

fn prompt_text(app: &App) -> String {
    app.prompt_state.prompt.get_text()
}

// ---------------------------------------------------------------------------
// The word motions
// ---------------------------------------------------------------------------

/// `w` moves the caret to the start of the next word.
#[test]
fn w_moves_the_caret_to_the_start_of_the_next_word() {
    let mut app = test_app();
    compose(&mut app, "one two three", 0);

    press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);

    assert_eq!(caret(&app), 4, "`w` did not reach the next word");
}

/// `b` moves the caret to the start of the previous word.
#[test]
fn b_moves_the_caret_to_the_start_of_the_previous_word() {
    let mut app = test_app();
    compose(&mut app, "one two three", 8);

    press(&mut app, KeyCode::Char('b'), KeyModifiers::NONE);

    assert_eq!(caret(&app), 4, "`b` did not reach the previous word");
}

/// `e` moves the caret to the last character of the word at the caret.
#[test]
fn e_moves_the_caret_to_the_end_of_the_word() {
    let mut app = test_app();
    compose(&mut app, "one two three", 0);

    press(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);

    assert_eq!(caret(&app), 2, "`e` did not reach the end of the word");
}

/// The word motions land on exactly the offset the shared geometry computes.
///
/// The loop is the point: it is the same text and the same offsets the
/// composer and the editor are compared over, so a drift in either direction
/// shows up here.
#[test]
fn the_word_motions_land_on_the_shared_geometrys_offsets() {
    let text = "one two three";
    for offset in 0..=text.len() {
        let mut app = test_app();
        compose(&mut app, text, offset);

        press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);
        assert_eq!(
            caret(&app),
            text_geometry::word_forward(text, offset),
            "`w` disagreed with the shared geometry at offset {offset}"
        );

        let mut app = test_app();
        compose(&mut app, text, offset);
        press(&mut app, KeyCode::Char('b'), KeyModifiers::NONE);
        assert_eq!(
            caret(&app),
            text_geometry::word_backward(text, offset),
            "`b` disagreed with the shared geometry at offset {offset}"
        );

        let mut app = test_app();
        compose(&mut app, text, offset);
        press(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);
        assert_eq!(
            caret(&app),
            text_geometry::word_end(text, offset),
            "`e` disagreed with the shared geometry at offset {offset}"
        );
    }
}

/// The word motions stop at the buffer's edges rather than wrapping.
#[test]
fn the_word_motions_stop_at_the_buffers_edges() {
    let mut app = test_app();
    compose(&mut app, "one two", 7);
    press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);
    assert_eq!(caret(&app), 7, "`w` ran past the end of the buffer");

    let mut app = test_app();
    compose(&mut app, "one two", 0);
    press(&mut app, KeyCode::Char('b'), KeyModifiers::NONE);
    assert_eq!(caret(&app), 0, "`b` ran past the start of the buffer");
}

// ---------------------------------------------------------------------------
// The vertical motions
// ---------------------------------------------------------------------------

/// `j` and `k` move the caret between the buffer's lines.
#[test]
fn j_and_k_move_the_caret_between_lines() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo\nthree", 0);

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(caret(&app), 4, "`j` did not move the caret down a line");

    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(caret(&app), 0, "`k` did not move the caret up a line");
}

/// The vertical motions keep the column where the target line allows.
#[test]
fn the_vertical_motions_keep_the_column() {
    let mut app = test_app();
    compose(&mut app, "one\ntwo", 2);

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);

    assert_eq!(caret(&app), 6, "the column should have been kept");
}

/// The vertical motions land on exactly the offset the shared geometry
/// computes.
#[test]
fn the_vertical_motions_land_on_the_shared_geometrys_offsets() {
    let text = "one\ntwo\nthree";
    for offset in 0..=text.len() {
        let mut app = test_app();
        compose(&mut app, text, offset);

        press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(
            caret(&app),
            text_geometry::move_vertical(text, offset, 1, None),
            "`j` disagreed with the shared geometry at offset {offset}"
        );

        let mut app = test_app();
        compose(&mut app, text, offset);
        press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(
            caret(&app),
            text_geometry::move_vertical(text, offset, -1, None),
            "`k` disagreed with the shared geometry at offset {offset}"
        );
    }
}

/// `j` and `k` are claimed only on a multi-line buffer.
///
/// On a single-line buffer they fall through to transcript scroll, which is
/// what ADR-0042's conditional pair says. The caret staying put is the
/// observable half; the scroll is the other half, and it is checked in the
/// claim table's own tests.
#[test]
fn the_vertical_motions_do_not_apply_to_a_single_line_buffer() {
    let mut app = test_app();
    compose(&mut app, "one two three", 4);

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(
        caret(&app),
        4,
        "`j` moved the caret on a single-line buffer"
    );

    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(
        caret(&app),
        4,
        "`k` moved the caret on a single-line buffer"
    );
}

/// A motion does not change the text.
#[test]
fn a_motion_does_not_change_the_text() {
    let mut app = test_app();
    compose(&mut app, "one two three", 0);

    press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "one two three");
}

// ---------------------------------------------------------------------------
// The claim still holds
// ---------------------------------------------------------------------------

/// A live verb is still the composer's: it does not reach an app-level action.
///
/// The fallthrough rule says a claimed letter is consumed by the composer. A
/// verb becoming live must not weaken that, so the same oracle the claim
/// table's tests use — a bound app-level action — is used here.
#[test]
fn a_motion_does_not_reach_an_app_level_action() {
    let mut app = test_app();
    app.ui_state
        .bindings
        .insert((KeyModifiers::NONE, KeyCode::Char('w')), InputAction::Help);
    compose(&mut app, "one two three", 0);

    press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);

    assert!(
        !matches!(app.ui_state.dialog, codegg::tui::app::Dialog::Help(_)),
        "the claimed letter `w` reached the app's binding map"
    );
}

/// The motions are Normal-mode verbs: in Insert mode the same letters are
/// text.
#[test]
fn the_motions_are_normal_mode_verbs() {
    let mut app = test_app();
    compose(&mut app, "one two three", 0);
    app.ui_state.input_mode = InputMode::Insert;

    press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "wone two three",
        "in Insert mode `w` must be typed, not run as a motion"
    );
    assert_eq!(caret(&app), 1, "the typed character moved the caret");
}

/// A motion never reaches a surface that does not own the keyboard.
///
/// The claim is live only when the composer holds the keyboard, so a focused
/// panel keeps its own keys.
#[test]
fn a_motion_does_not_fire_while_a_panel_owns_the_keyboard() {
    let mut app = test_app();
    compose(&mut app, "one two three", 0);
    app.ui_state
        .bindings
        .insert((KeyModifiers::NONE, KeyCode::Char('w')), InputAction::Help);

    // A mounted modal owns the keyboard outright, so the composer's claim is
    // not consulted at all.
    app.open_dialog(codegg::tui::app::Dialog::Help(
        codegg::tui::components::dialogs::help::HelpRoute::Command,
    ));

    press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        0,
        "the motion ran while a dialog owned the keyboard"
    );
}

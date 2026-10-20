//! The composer's undo and redo verbs.
//!
//! M006 made `u` and `U` live. The verbs reach the composer's text model,
//! which is the only thing that owns the composer's history — so the tests
//! here go through the **key chain** (`App::on_key`), because the dispatch is
//! what M006 changed. A test that called `ComposerText::undo` directly would
//! pass even if the claim table and the runner had drifted apart.
//!
//! The undo scope is the composer's own text (ADR-0042 part 4; the roadmap's
//! D4 row). Two paths replace the whole buffer — a submit and a staged
//! projection — and both drop the history, so an undo can never bring back
//! text the user has already sent.

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

/// The messages the toast manager is currently showing.
fn toast_messages(app: &App) -> Vec<String> {
    app.messages_state
        .toasts
        .iter()
        .map(|toast| toast.message.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// The verbs
// ---------------------------------------------------------------------------

/// `u` undoes the newest edit.
#[test]
fn u_undoes_the_newest_edit() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    // Type a character, so the buffer holds one recorded edit.
    app.ui_state.input_mode = InputMode::Insert;
    press(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "hello!");

    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "hello", "`u` did not undo the edit");
}

/// `U` redoes the newest undone edit.
#[test]
fn u_uppercase_redoes_the_newest_undone_edit() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    app.ui_state.input_mode = InputMode::Insert;
    press(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
    app.ui_state.input_mode = InputMode::Normal;

    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "hello");

    press(&mut app, KeyCode::Char('U'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "hello!", "`U` did not redo the edit");
}

/// The capital claim answers to both encodings.
///
/// Terminals disagree about whether a shifted key carries the SHIFT modifier,
/// so `U` is bound twice. A terminal that reports the unshifted letter has to
/// reach the verb too.
#[test]
fn the_redo_verb_answers_to_both_capital_encodings() {
    for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
        let mut app = test_app();
        compose(&mut app, "hello", 5);

        app.ui_state.input_mode = InputMode::Insert;
        press(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
        app.ui_state.input_mode = InputMode::Normal;

        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('U'), modifiers);

        assert_eq!(
            prompt_text(&app),
            "hello!",
            "`U` under {modifiers:?} did not redo the edit"
        );
    }
}

/// Undo walks back through several edits, and redo walks forward again.
#[test]
fn undo_and_redo_walk_through_several_edits() {
    let mut app = test_app();
    compose(&mut app, "", 0);

    app.ui_state.input_mode = InputMode::Insert;
    for character in ['a', 'b', 'c'] {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    assert_eq!(prompt_text(&app), "abc");

    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "ab");
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "a");

    press(&mut app, KeyCode::Char('U'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "ab");
    press(&mut app, KeyCode::Char('U'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "abc");
}

/// Undo puts the caret back where the edit started.
///
/// The caret is part of the edit: undoing a paste has to leave the caret
/// where the user was before the paste, or the next keystroke lands in the
/// wrong place.
#[test]
fn undo_restores_the_caret() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);

    app.ui_state.input_mode = InputMode::Insert;
    press(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
    assert_eq!(caret(&app), 1);

    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert_eq!(caret(&app), 0, "the caret did not go back to where it was");
}

// ---------------------------------------------------------------------------
// The verbs report in both branches
// ---------------------------------------------------------------------------

/// `u` with nothing to undo says so.
///
/// A silent no-op would teach the user that the key is dead, and "there was
/// nothing to undo" is a different fact from "nothing happened".
#[test]
fn undo_with_nothing_to_undo_reports() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "hello");
    assert!(
        toast_messages(&app)
            .iter()
            .any(|message| message.contains("Nothing to undo")),
        "the undo verb did not report that there was nothing to undo; the toasts \
         were {:?}",
        toast_messages(&app)
    );
}

/// `U` with nothing to redo says so.
#[test]
fn redo_with_nothing_to_redo_reports() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    press(&mut app, KeyCode::Char('U'), KeyModifiers::NONE);

    assert_eq!(prompt_text(&app), "hello");
    assert!(
        toast_messages(&app)
            .iter()
            .any(|message| message.contains("Nothing to redo")),
        "the redo verb did not report that there was nothing to redo; the toasts \
         were {:?}",
        toast_messages(&app)
    );
}

/// A successful undo reports too.
///
/// The rule is that a verb is never silent: the two branches say different
/// things, so neither of them can be mistaken for the other.
#[test]
fn a_successful_undo_reports() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    app.ui_state.input_mode = InputMode::Insert;
    press(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
    app.ui_state.input_mode = InputMode::Normal;

    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    let messages = toast_messages(&app);
    assert!(
        messages.iter().any(|message| message.contains("Undid")),
        "the undo verb did not report the undo; the toasts were {messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.contains("Nothing to undo")),
        "a successful undo also claimed there was nothing to undo"
    );
}

// ---------------------------------------------------------------------------
// The eviction fix
// ---------------------------------------------------------------------------

/// A paste larger than the byte bound is still undoable.
///
/// This is the defect M006 fixed. The history's byte bound evicted the
/// **oldest** entry, and on an oversized paste the oldest entry *was* the
/// paste — it was evicted the moment it was made, so the history was empty
/// and the user's last action could not be undone at all.
///
/// The bound now drops older entries and never the last one, so an edit over
/// the bound stays undoable.
#[test]
fn an_oversized_edit_is_still_undoable() {
    let mut app = test_app();
    compose(&mut app, "", 0);

    app.ui_state.input_mode = InputMode::Insert;
    let big = "x".repeat(128 * 1024);
    app.prompt_state.prompt.paste(big.clone());
    assert_eq!(prompt_text(&app).len(), big.len());

    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "",
        "an oversized paste was not undoable; the byte bound evicted the edit \
         that had just been made"
    );
}

/// The undo scope is the composer's own text, and a wholesale replacement
/// resets the history.
///
/// A submit clears the buffer and a staged projection replaces it; both are
/// replacements rather than edits, so neither is undoable — an undo must not
/// bring back text the user has already sent.
#[test]
fn replacing_the_whole_buffer_resets_the_history() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);

    app.ui_state.input_mode = InputMode::Insert;
    press(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
    assert_eq!(prompt_text(&app), "hello!");

    // A wholesale replacement, the way a staged projection lands.
    app.prompt_state.prompt.set_text("replaced".to_string());
    app.ui_state.input_mode = InputMode::Normal;

    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "replaced",
        "an undo brought back text from before the replacement"
    );
    assert!(
        toast_messages(&app)
            .iter()
            .any(|message| message.contains("Nothing to undo")),
        "the history was not reset by the replacement"
    );
}

// ---------------------------------------------------------------------------
// The selection rule
// ---------------------------------------------------------------------------

/// Undo drops the held selection.
///
/// The selection's byte offsets name positions in the text that the undo just
/// replaced. Carrying them across would make the selection cover different
/// characters — or reach past the end of the buffer.
#[test]
fn undo_drops_the_held_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    // Arm a character selection, then type, which is an edit.
    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
    assert!(
        app.prompt_state.prompt.selection_range().is_some(),
        "the selection was not armed"
    );

    app.ui_state.input_mode = InputMode::Insert;
    press(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
    assert!(
        app.prompt_state.prompt.selection_range().is_none(),
        "the edit did not drop the held selection"
    );

    app.ui_state.input_mode = InputMode::Normal;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert!(
        app.prompt_state.prompt.selection_range().is_none(),
        "the undo kept a selection recorded against the old text"
    );
}

// ---------------------------------------------------------------------------
// The claim still holds
// ---------------------------------------------------------------------------

/// The undo verbs are Normal-mode verbs: in Insert mode the same letters are
/// text.
#[test]
fn the_undo_verbs_are_normal_mode_verbs() {
    let mut app = test_app();
    compose(&mut app, "hello", 5);
    app.ui_state.input_mode = InputMode::Insert;

    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert_eq!(
        prompt_text(&app),
        "hellou",
        "in Insert mode `u` must be typed, not run as an undo"
    );
}

/// A claimed letter is consumed: `u` never reaches an app-level action.
#[test]
fn the_undo_verb_does_not_reach_an_app_level_action() {
    let mut app = test_app();
    app.ui_state.bindings.insert(
        (KeyModifiers::NONE, KeyCode::Char('u')),
        codegg::tui::input::InputAction::Help,
    );
    compose(&mut app, "hello", 5);

    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);

    assert!(
        !matches!(app.ui_state.dialog, codegg::tui::app::Dialog::Help(_)),
        "the claimed letter `u` reached the app's binding map"
    );
}

//! The live dialog dispatch, checked branch by branch against the branches
//! the `#[cfg(any())]` `handle_dialog_key_legacy` body handled.
//!
//! `handle_dialog_key_legacy` is a 615-line body that has never been
//! compiled (`#[cfg(any())]`) and has no caller. It was retained as the
//! record of the dialog key contract that the live `FocusManager`-owned
//! dispatch replaced. This member is the executable part of that record: it
//! drives the live dispatch with the same keys the legacy body handled and
//! asserts that the live dispatch still owns them.
//!
//! Three branches the legacy body handled have **no** live counterpart, and
//! this member deliberately does not assert them — asserting them would lock
//! the gap in place rather than report it. They are recorded in
//! `plans/closure/repository-audit-corrective/005-status.md`:
//!
//! - the Model dialog's `Tab` — the `FocusManager` consumes `Tab` for its
//!   focus ring, so it never reaches the component that owns the tabs;
//! - the `/shell-show` modal's `i`/`a`/`r`/`k`/`e` verbs, which the modal's
//!   own footer still advertises;
//! - the Import dialog's text input (`Char`/`Backspace`): the live
//!   `ImportDialog` component handles only `Esc` and `Enter`, so a typed URL
//!   never reaches the preview, and the legacy body holds the only
//!   implementation of that input.

use codegg::tui::components::component::{DialogType, FocusManager};
use codegg::tui::components::dialogs::model::ModelDialog;
use codegg::tui::components::dialogs::question::{QuestionDialog, QuestionSpec};
use codegg::tui::components::dialogs::session::SessionDialog;
use codegg::tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn theme() -> Arc<Theme> {
    Arc::new(Theme::dark())
}

/// The Model dialog owns its filter keys: typing reaches the filter and
/// `Backspace` pops it.
#[test]
fn the_model_dialog_owns_its_filter_keys() {
    let mut focus = FocusManager::new();
    focus.push(Box::new(ModelDialog::new(theme())));

    focus.handle_key(key(KeyCode::Char('g')));
    focus.handle_key(key(KeyCode::Char('p')));
    let filter = focus
        .with_dialog::<ModelDialog, _>(DialogType::Model, |dialog| dialog.filter.clone())
        .expect("the model dialog is mounted");
    assert_eq!(filter, "gp", "typing must reach the model dialog's filter");

    focus.handle_key(key(KeyCode::Backspace));
    let filter = focus
        .with_dialog::<ModelDialog, _>(DialogType::Model, |dialog| dialog.filter.clone())
        .expect("the model dialog is mounted");
    assert_eq!(filter, "g", "backspace must pop the model dialog's filter");
}

/// The Question dialog owns its answer-editing keys: typing appends,
/// `Left` moves the cursor, `Delete` removes the character under it, and
/// `Backspace` removes the one before it.
#[test]
fn the_question_dialog_owns_its_answer_editing_keys() {
    let mut focus = FocusManager::new();
    focus.push(Box::new(QuestionDialog::new(vec![QuestionSpec {
        question: "Continue?".to_string(),
        options: None,
        initial: None,
    }])));

    focus.handle_key(key(KeyCode::Char('y')));
    focus.handle_key(key(KeyCode::Char('e')));
    let (answers, cursor) = focus
        .with_dialog::<QuestionDialog, _>(DialogType::Question, |dialog| {
            (dialog.answers.clone(), dialog.cursor_pos)
        })
        .expect("the question dialog is mounted");
    assert_eq!(
        answers,
        vec!["ye".to_string()],
        "typing must reach the answer"
    );
    assert_eq!(cursor, 2, "the cursor must sit after the typed answer");

    focus.handle_key(key(KeyCode::Left));
    focus.handle_key(key(KeyCode::Delete));
    let (answers, cursor) = focus
        .with_dialog::<QuestionDialog, _>(DialogType::Question, |dialog| {
            (dialog.answers.clone(), dialog.cursor_pos)
        })
        .expect("the question dialog is mounted");
    assert_eq!(
        answers,
        vec!["y".to_string()],
        "delete must remove the character under the cursor"
    );
    assert_eq!(cursor, 1, "delete must leave the cursor where it was");

    focus.handle_key(key(KeyCode::Backspace));
    let answers = focus
        .with_dialog::<QuestionDialog, _>(DialogType::Question, |dialog| dialog.answers.clone())
        .expect("the question dialog is mounted");
    assert_eq!(
        answers,
        vec![String::new()],
        "backspace must remove the character before the cursor"
    );
}

/// A key goes to the top mounted component only; the dialog below it does
/// not see it.
///
/// `z` is chosen deliberately: the session dialog claims `s` (sort), `h`
/// (archived), `b` (bulk), `m` (search) and several others, so `z` is the
/// one character that reaches its filter.
#[test]
fn a_key_reaches_the_top_mounted_component_only() {
    let mut focus = FocusManager::new();
    focus.push(Box::new(ModelDialog::new(theme())));
    focus.push(Box::new(SessionDialog::new(theme())));

    focus.handle_key(key(KeyCode::Char('z')));

    let model_filter = focus
        .with_dialog::<ModelDialog, _>(DialogType::Model, |dialog| dialog.filter.clone())
        .expect("the model dialog is mounted");
    let session_filter = focus
        .with_dialog::<SessionDialog, _>(DialogType::Session, |dialog| dialog.filter.clone())
        .expect("the session dialog is mounted");
    assert_eq!(session_filter, "z", "the top dialog must receive the key");
    assert_eq!(
        model_filter, "",
        "a dialog below the top must not receive the key"
    );
}

/// A key no dialog handles is dropped: it produces no message and does not
/// fall through to a dialog below.
#[test]
fn an_unhandled_dialog_key_produces_no_message() {
    let mut focus = FocusManager::new();
    focus.push(Box::new(ModelDialog::new(theme())));

    assert!(
        focus.handle_key(key(KeyCode::F(5))).is_none(),
        "an unhandled dialog key must be dropped, not turned into a message"
    );
}

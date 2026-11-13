//! The live dialog dispatch, checked against the branches the `#[cfg(any())]`
//! `handle_dialog_key_legacy` body handled — the record of the dialog key
//! contract the `FocusManager`-owned dispatch replaced.
//!
//! Of its three branches with no live counterpart: Import `Char`/`Backspace`
//! is **closed** (`ImportDialog::handle_key` edits its input in
//! `ImportState::Input`); `/shell-show`'s `i`/`a`/`r`/`k`/`e` is **closed by
//! removing the advertisement** — no arm dispatches those five **verbs**, and
//! `k` reaches the scroll arm rather than a kill, so the row named one working
//! key and four intents that each need a shell-detail id the widget is never
//! handed; the modal therefore paints its own row and the verbs stay
//! unimplemented. Model `Tab` is **retained**, because `FocusManager` owns
//! `Tab` for every dialog (ADR-0042) and this milestone does not amend it.
//! Long form: `docs/tui-render-invariant-baseline.toml`.

use codegg::tui::app::TuiMsg;
use codegg::tui::components::component::{Component, DialogType, FocusManager};
use codegg::tui::components::dialogs::import::ImportDialog;
use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};
use codegg::tui::components::dialogs::model::ModelDialog;
use codegg::tui::components::dialogs::question::{QuestionDialog, QuestionSpec};
use codegg::tui::components::dialogs::session::SessionDialog;
use codegg::tui::components::hint_line::HintLine;
use codegg::tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
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

/// The Import dialog takes a typed URL through the live dispatch, and `Enter`
/// offers its preview.
#[test]
fn the_import_dialog_takes_a_typed_url_and_offers_its_preview() {
    let mut focus = FocusManager::new();
    focus.push(Box::new(ImportDialog::new(theme())));

    for c in "codegg://share/abc".chars() {
        assert!(
            focus.handle_key(key(KeyCode::Char(c))).is_none(),
            "typing edits the input and reports no message"
        );
    }
    let input = focus
        .with_dialog::<ImportDialog, _>(DialogType::Import, |dialog| dialog.input.clone())
        .expect("the import dialog is mounted");
    assert_eq!(
        input, "codegg://share/abc",
        "typing must reach the import dialog's input"
    );

    assert_eq!(
        focus.handle_key(key(KeyCode::Enter)),
        Some(TuiMsg::SubmitImportPreview),
        "Enter must offer the preview of what was typed"
    );
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

/// The modal's verb row, read back from a rendered frame: it is the last of the
/// three stacked sections, so its text is the second-to-last row.
fn painted_footer(dialog: &mut InfoDialog, theme: &Arc<Theme>) -> String {
    let (width, height) = (100u16, 20u16);
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| dialog.render(frame, frame.area(), theme))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..width)
        .map(|x| buffer[(x, height - 2)].symbol().to_string())
        .collect()
}

/// `/shell-show` paints only the verbs it dispatches. The caller still sets the
/// six-verb row, so this reads the **painted** footer rather than the intent.
#[test]
fn the_shell_show_modal_advertises_only_keys_it_handles() {
    let theme = theme();
    let mut shell = InfoDialog::new(
        Arc::clone(&theme),
        InfoType::ShellShow,
        vec!["  $ cargo test".to_string()],
    );
    shell.set_custom_footer(HintLine::from_items(&[
        ("i", "include"),
        ("a", "ask"),
        ("r", "rerun"),
        ("k", "kill"),
        ("e", "expand"),
        ("j/k", "scroll"),
        ("Esc", "close"),
    ]));
    let footer = painted_footer(&mut shell, &theme);

    for unhandled in ["i include", "a ask", "r rerun", "k kill", "e expand"] {
        assert!(
            !footer.contains(unhandled),
            "the /shell-show footer advertises `{unhandled}`, which no arm dispatches: \
             {footer:?}"
        );
    }
    assert!(
        footer.contains("j/k scroll") && footer.contains("Esc/Enter close"),
        "the footer must name the verbs the box really dispatches: {footer:?}"
    );
}

/// ...and the substitution is scoped to `/shell-show`, so a caller row whose
/// verbs *are* dispatched still paints.
#[test]
fn a_caller_footer_still_paints_where_its_verbs_are_dispatched() {
    let theme = theme();
    let mut init = InfoDialog::new(
        Arc::clone(&theme),
        InfoType::ProjectInit,
        vec!["  candidate".to_string()],
    );
    init.set_custom_footer(HintLine::from_items(&[("a", "publish"), ("Esc", "cancel")]));

    let footer = painted_footer(&mut init, &theme);
    assert!(
        footer.contains("a publish"),
        "the de-advertisement leaked off /shell-show: {footer:?}"
    );
}

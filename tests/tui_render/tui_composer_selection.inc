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

use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::tui::app::{App, Dialog};
use codegg::tui::components::dialogs::help::HelpRoute;
use codegg::tui::document_session::TuiDocumentSession;
use codegg::tui::editor::EditorMode;
use codegg::tui::input::{InputAction, InputMode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;
use std::sync::Arc;
use tokio::sync::mpsc;

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

/// Draw the app and hand back the frame's buffer.
fn render(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("render the app");
    terminal.backend().buffer().clone()
}

/// The text the frame painted, row by row.
fn frame_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut text = String::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

/// The characters the frame painted with the selection's own colours.
///
/// The caret block is painted **over** the selection's head — the caret
/// sits on the head — so the head character is the one character of a
/// selection the frame does not paint with the selection's colours.
fn painted_selection(app: &App, buffer: &Buffer) -> String {
    let theme = &app.ui_state.theme;
    let area = buffer.area;
    let mut painted = String::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            if cell.bg == theme.selection && cell.fg == theme.selection_text {
                painted.push_str(cell.symbol());
            }
        }
    }
    painted
}

/// The characters the frame painted with the caret block's colours.
///
/// The block is the caret's cell, and the caret is the selection's head, so
/// this is the one character of a selection the selection's own colours do
/// not reach.
fn painted_caret(app: &App, buffer: &Buffer) -> String {
    let theme = &app.ui_state.theme;
    let area = buffer.area;
    let mut painted = String::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            if cell.bg == theme.primary && cell.fg == theme.background {
                painted.push_str(cell.symbol());
            }
        }
    }
    painted
}

/// A core client that answers nothing.
///
/// The editor's mode is read off an open document, and the test only needs
/// the document to exist: nothing in this file talks to a daemon.
struct SilentClient;

#[async_trait::async_trait]
impl CoreClient for SilentClient {
    async fn request(
        &self,
        _request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        Err(AppError::Other(anyhow::anyhow!(
            "the composer selection tests never reach the daemon"
        )))
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        rx
    }
}

/// Open an editor document whose mode is `mode`.
///
/// The badge describes the surface that holds the keyboard, so a test that
/// wants the editor's side of that rule needs an editor in a mode the
/// composer is not in.
fn open_editor_in(app: &mut App, mode: EditorMode) {
    app.editor_state.session = Some(TuiDocumentSession::new(Arc::new(SilentClient)));
    app.editor_state
        .session
        .as_mut()
        .expect("the document is open")
        .presentation_mut()
        .mode = mode;
}

// ---------------------------------------------------------------------------
// The selection verbs
// ---------------------------------------------------------------------------

/// `v` is a **mode**: it enters the composer's select mode and leaves it.
///
/// The badge reads the mode, so the accessor the badge uses is the one the
/// test reads.
#[test]
fn v_is_a_mode_the_badge_can_read() {
    let mut app = test_app();
    compose(&mut app, "hello world", 2);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    assert!(
        app.prompt_state.prompt.is_selecting(),
        "`v` did not enter the composer's select mode"
    );

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    assert!(
        !app.prompt_state.prompt.is_selecting(),
        "`v` did not leave the composer's select mode"
    );
}

/// The badge reads `[SEL]` while the composer holds the keyboard in its
/// select mode, and `[NOR]` outside it.
///
/// The badge describes the surface that holds the keyboard, and here that
/// surface is the composer.
#[test]
fn the_composer_badge_shows_select_while_it_is_selecting() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    let text = frame_text(&render(&mut app, 100, 30));
    assert!(
        text.contains("[SEL]"),
        "the badge does not read [SEL] while the composer is selecting: {text}"
    );

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    let text = frame_text(&render(&mut app, 100, 30));
    assert!(
        text.contains("[NOR]"),
        "the badge does not read [NOR] outside the select mode: {text}"
    );
}

/// The badge describes the surface that holds the keyboard, from both sides.
///
/// With the **editor** in select mode and the **composer** holding the
/// keyboard, the badge reads the composer's mode. It used to read the
/// editor's, so an editor in select mode showed `[SEL]` above a composer
/// that was in Normal mode.
#[test]
fn the_composer_badge_describes_the_surface_that_holds_the_keyboard() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);
    open_editor_in(&mut app, EditorMode::Select);

    let text = frame_text(&render(&mut app, 100, 30));
    assert!(
        text.contains("[NOR]"),
        "the badge does not read the composer's own mode: {text}"
    );
    assert!(
        !text.contains("[SEL]"),
        "the badge read the editor's mode over the composer: {text}"
    );
}

/// A surface that holds the keyboard is not the editor's.
///
/// The badge describes the surface that holds the keys, and "not the
/// composer" is not the same thing as "the editor": the timeline overlay
/// takes the composer out without the editor taking over. The badge then
/// reads the composer's own **resolved** mode — and reading the editor's
/// mode there would name a surface the reader is not in, which is the
/// symptom the badge had once already: a panel holding a live PTY claimed
/// the Insert keys and the row said `[NOR]` while the reader typed.
#[test]
fn the_badge_reads_the_composer_while_another_surface_holds_the_keys() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);
    app.ui_state.input_mode = InputMode::Insert;
    app.ui_state.timeline_visible = true;

    let text = frame_text(&render(&mut app, 100, 30));
    assert!(
        text.contains("[INS]"),
        "the badge did not read the composer's resolved mode: {text}"
    );
    assert!(
        !text.contains("[NOR]"),
        "the badge read the editor's mode over the composer: {text}"
    );
}

/// `e` in Normal mode selects the word the caret landed on.
///
/// The rendered selection is what the reader sees, so the test reads the
/// frame rather than the model. The frame paints the word's characters with
/// the selection's colours and the word's last one with the caret block's,
/// because the caret sits on the head of the selection.
#[test]
fn a_word_motion_selects_the_word_in_normal_mode() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);

    let buffer = render(&mut app, 100, 30);
    assert_eq!(
        format!(
            "{}{}",
            painted_selection(&app, &buffer),
            painted_caret(&app, &buffer)
        ),
        "hello",
        "`e` did not paint the word the caret landed on"
    );
}

/// A movement outside the select mode drops the held selection.
///
/// The selection the word motion held is gone from the frame, not merely
/// from the model.
#[test]
fn a_movement_outside_select_mode_drops_the_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);

    let buffer = render(&mut app, 100, 30);
    assert_eq!(
        painted_selection(&app, &buffer),
        "",
        "the movement in Normal mode kept the selection on screen"
    );
}

/// A movement in the select mode grows the held selection.
#[test]
fn a_movement_in_select_mode_grows_the_selection() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);

    let buffer = render(&mut app, 100, 30);
    assert_eq!(
        format!(
            "{}{}",
            painted_selection(&app, &buffer),
            painted_caret(&app, &buffer)
        ),
        "he",
        "the movement in the select mode did not grow the selection"
    );
}

/// Every path that moves the caret reaches the same rule.
///
/// The claim table's verbs, `h`/`l`, the arrows and `Home`/`End` all have to
/// answer "what does this movement do to the selection" the same way. The
/// rule says a movement in the select mode **grows** the selection, so a
/// path that reached a different rule would drop it instead.
#[test]
fn every_caret_moving_path_resolves_the_same_rule() {
    let paths: Vec<(&str, KeyEvent)> = vec![
        (
            "the claim table's `e`",
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        ),
        (
            "the claim table's `b`",
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        ),
        ("`h`", KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
        ("`l`", KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
        (
            "the left arrow",
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        ),
        (
            "the right arrow",
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        ),
        ("`Home`", KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
        ("`End`", KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
    ];

    for (description, key) in paths {
        let mut app = test_app();
        compose(&mut app, "hello world", 4);
        press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
        let armed = app
            .prompt_state
            .prompt
            .selection_range()
            .expect("`v` did not arm a selection");

        press(&mut app, key.code, key.modifiers);

        let grown = app
            .prompt_state
            .prompt
            .selection_range()
            .unwrap_or_else(|| panic!("{description}: the movement dropped the selection"));
        assert!(
            grown.0 <= armed.0 && grown.1 >= armed.1,
            "{description}: the selection did not grow — {armed:?} became {grown:?}"
        );
    }
}

/// A composer that loses the keyboard does not come back holding a select
/// mode.
///
/// The select mode belongs to the surface that holds the keyboard, so it
/// cannot survive the keyboard's leaving.
#[test]
fn the_composer_loses_its_select_mode_when_the_keyboard_leaves_it() {
    let mut app = test_app();
    compose(&mut app, "hello", 0);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    assert!(
        app.prompt_state.prompt.is_selecting(),
        "`v` did not enter the select mode"
    );

    // A side panel takes the keyboard.
    assert!(app.panel_host.focus(), "the side panel took the keyboard");

    // The next key is dispatched while the panel holds the keyboard, and the
    // composer's mode is brought back in step with the surface that now
    // holds it.
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);

    assert!(
        !app.prompt_state.prompt.is_selecting(),
        "the composer kept its select mode across the panel"
    );
}

/// The `w` verb moves to the next word's start and selects nothing.
///
/// Only `e` and `b` — the movements that land on a word's **edge** — select a
/// word.
#[test]
fn w_does_not_select_a_word() {
    let mut app = test_app();
    compose(&mut app, "hello world", 0);

    press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);

    assert_eq!(caret(&app), 6, "`w` did not reach the next word");
    assert_eq!(
        app.prompt_state.prompt.selection_range(),
        None,
        "`w` selected a word"
    );
}

// ---------------------------------------------------------------------------
// The copy gesture, and the route that owns it
// ---------------------------------------------------------------------------

/// `space` and then `y` in the help box the chord opened copies the
/// composer's selection.
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

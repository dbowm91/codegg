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
//! The word motions answer with `codegg::tui::text_geometry`, and the
//! vertical motions with `codegg::tui::composer_geometry`: the composer is
//! not allowed a second opinion about where a word is or where a row starts,
//! and a composer that disagreed with the shared geometry would be two
//! surfaces answering the same question differently.
//!
//! The vertical motions are **visual-row** motions (ADR-0042's amendment),
//! and they are claimed only when the composer's content does not fit its
//! box. Both facts are layout facts, so the tests below lay the composer out
//! before they press a key: a composer that has never been drawn has no
//! measurement, and an unmeasured composer does not claim the key.

use codegg::tui::app::App;
use codegg::tui::composer_geometry::VisualLayout;
use codegg::tui::input::{InputAction, InputMode};
use codegg::tui::text_geometry;
use codegg::tui::wrap::wrap_styled_spans_by_word_rows;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::text::Span;
use ratatui::Terminal;

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
// The visual-row motions
// ---------------------------------------------------------------------------

/// Lay the composer out, so the claim has a measurement.
///
/// The `j`/`k` claim is a **layout** fact: the composer claims them when its
/// content does not fit its box, and the box only exists once the app has
/// rendered. A test that presses a key without rendering first is testing
/// the *unmeasured* case, where the key falls through to the transcript.
fn render_app(app: &mut App, width: u16, height: u16) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("render the app");
}

/// The visual layout the composer drew.
///
/// The composer is not allowed a second opinion about where a row starts: the
/// test asks the same geometry module the composer asks.
fn visual_layout(app: &App) -> VisualLayout {
    let text = app.prompt_state.prompt.get_text();
    let width = app.prompt_state.prompt.wrap_width;
    let rows = wrap_styled_spans_by_word_rows(&[Span::raw(text.clone())], width);
    VisualLayout::new(&text, &rows)
}

/// A prompt whose content overflows the composer's box.
///
/// The box grows to fit its content up to the pane's budget, so "overflows"
/// means "more rows than the box can show" — which takes a text long enough
/// to wrap onto more rows than the box has.
fn overflowing_prompt() -> String {
    (0..200).map(|index| format!("w{index:03} ")).collect()
}

/// `j` moves the caret down a **visual row**, and the transcript does not
/// scroll.
///
/// The row is a row of the wrapped text: the prompt here has no newline in it
/// at all, and it still occupies several rows.
#[test]
fn j_moves_the_caret_down_a_visual_row() {
    let mut app = test_app();
    compose(&mut app, &overflowing_prompt(), 0);
    render_app(&mut app, 80, 24);
    assert!(
        app.prompt_state.prompt.content_overflows(),
        "precondition: the composer's content overflows its box"
    );
    let layout = visual_layout(&app);
    let expected = layout.byte_at(1, 0).expect("the second row has a cell");

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        expected,
        "`j` did not move the caret down a visual row"
    );
}

/// `k` moves the caret up a visual row.
#[test]
fn k_moves_the_caret_up_a_visual_row() {
    let mut app = test_app();
    compose(&mut app, &overflowing_prompt(), 0);
    render_app(&mut app, 80, 24);
    let layout = visual_layout(&app);
    let second_row = layout.byte_at(1, 0).expect("the second row has a cell");
    compose(&mut app, &overflowing_prompt(), second_row);
    render_app(&mut app, 80, 24);

    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);

    assert_eq!(
        caret(&app),
        layout.byte_at(0, 0).expect("the first row has a cell"),
        "`k` did not move the caret up a visual row"
    );
}

/// The vertical motions keep the column where the target row allows.
#[test]
fn the_vertical_motions_keep_the_column() {
    let mut app = test_app();
    compose(&mut app, &overflowing_prompt(), 3);
    render_app(&mut app, 80, 24);
    let layout = visual_layout(&app);
    let expected = layout.byte_at(1, 3).expect("the second row has a cell");

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);

    assert_eq!(caret(&app), expected, "the column should have been kept");
}

/// The vertical motions land on exactly the offset the shared **visual**
/// geometry computes.
///
/// The composer answers "where is the row below this byte" with the shared
/// geometry module, and so does this test: a composer with a second opinion
/// about where a row starts is two surfaces answering the same question
/// differently.
#[test]
fn the_vertical_motions_land_on_the_shared_geometrys_offsets() {
    let text = overflowing_prompt();
    for (width, height) in [(40u16, 24u16), (80, 24), (120, 24)] {
        let mut app = test_app();
        compose(&mut app, &text, 0);
        render_app(&mut app, width, height);
        let layout = visual_layout(&app);
        assert!(
            layout.row_count() > 1,
            "the text must wrap onto more than one row at width {width}"
        );
        assert!(
            app.prompt_state.prompt.content_overflows(),
            "precondition: the composer's content overflows its box at width {width}"
        );

        // Every twenty-fifth byte: the widget's own test walks every byte of
        // the text, and this one walks the *dispatch* — that the key reaches
        // the geometry — across the pane's widths.
        for byte in (0..=text.len()).step_by(25) {
            let mut app = test_app();
            compose(&mut app, &text, byte);
            render_app(&mut app, width, height);
            let layout = visual_layout(&app);
            let position = layout.caret_position(byte).expect("the caret is on a row");
            let target = (position.row + 1).min(layout.row_count() - 1);
            let expected = if target == position.row {
                // Clamped at the last row: the caret stays where it is.
                byte
            } else {
                layout
                    .byte_at(target, position.column)
                    .or_else(|| layout.row_cells(target).last().map(|cell| cell.byte_end))
                    .unwrap_or_else(|| layout.row_source_start(target).unwrap_or(0))
            };

            press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
            assert_eq!(
                caret(&app),
                expected,
                "`j` disagreed with the shared geometry at width {width}, byte {byte}"
            );
        }
    }
}

/// `j` and `k` are claimed only when the composer's content **overflows** its
/// box.
///
/// On a composer whose content fits they fall through to transcript scroll,
/// which is what ADR-0042's conditional pair says. The caret staying put is
/// the observable half; the scroll is the other half, and it is checked in
/// the claim table's own tests.
#[test]
fn the_vertical_motions_do_not_apply_when_the_content_fits() {
    let mut app = test_app();
    compose(&mut app, "one two three", 4);
    render_app(&mut app, 80, 24);
    assert!(
        !app.prompt_state.prompt.content_overflows(),
        "precondition: the composer's content fits its box"
    );

    press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(
        caret(&app),
        4,
        "`j` moved the caret on a composer whose content fits"
    );

    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(
        caret(&app),
        4,
        "`k` moved the caret on a composer whose content fits"
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
// The page motions
// ---------------------------------------------------------------------------

/// `Ctrl+d` pages the **composer** when its content overflows the box.
///
/// The innermost surface first: the composer is the surface the user is
/// looking at, and its content does not fit.
#[test]
fn ctrl_d_pages_the_composer_when_its_content_overflows() {
    let mut app = test_app();
    compose(&mut app, &overflowing_prompt(), 0);
    render_app(&mut app, 80, 24);
    assert!(
        app.prompt_state.prompt.content_overflows(),
        "precondition: the composer's content overflows its box"
    );
    let scroll_before = app.prompt_state.prompt.scroll;

    press(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);

    assert!(
        app.prompt_state.prompt.scroll > scroll_before,
        "`Ctrl+d` must page the composer when its content overflows the box"
    );
}

/// `Ctrl+d` pages the **transcript** when the composer's content fits.
///
/// The transcript's page is unchanged when the composer fits: the composer's
/// box holds its content, so there is nothing of the composer's to page.
#[test]
fn ctrl_d_pages_the_transcript_when_the_composers_content_fits() {
    let mut app = test_app();
    for index in 0..40 {
        app.messages_state
            .messages
            .add_user_message(format!("transcript line {index}"), None);
    }
    // The test app has no session, so the transcript is never laid out by the
    // render: the widget's width and viewport are set here, or its line
    // count is zero and it cannot scroll at all.
    app.messages_state.messages.set_width(80);
    app.messages_state.messages.set_visible_height(10);
    compose(&mut app, "a short prompt", 0);
    render_app(&mut app, 80, 24);
    assert!(
        !app.prompt_state.prompt.content_overflows(),
        "precondition: the composer's content fits its box"
    );
    let composer_scroll = app.prompt_state.prompt.scroll;
    app.messages_state.messages.scroll = 0;

    press(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);

    assert_eq!(
        app.prompt_state.prompt.scroll, composer_scroll,
        "the composer must not page when its content fits"
    );
    assert!(
        app.messages_state.messages.scroll > 0,
        "`Ctrl+d` must page the transcript when the composer's content fits"
    );
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

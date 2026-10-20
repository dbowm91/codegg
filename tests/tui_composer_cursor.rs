//! Composer keyboard ownership and cursor presence.
//!
//! The composer is the main buffer's input box — the "LLM buffer" the user
//! types into. It draws a caret, and where that caret is comes from two
//! facts: the resolved input mode, and whether the composer currently holds
//! the keyboard.
//!
//! These tests pin both facts, because the two ways they can go wrong look
//! identical from the outside:
//!
//! 1. **A caret on a surface that does not have the keyboard.** A user who
//!    selected a panel and typed into it must not watch a caret sitting in
//!    the composer they are not typing in.
//! 2. **A caret whose shape disagrees with the `[INS]`/`[NOR]` badge.** The
//!    badge is the app's own statement about the mode; a caret that says
//!    something else teaches the wrong keymap.
//!
//! The blinking Insert caret is the terminal's own cursor, not an app-driven
//! one: the app runs a single animation cadence, and a second timer for a
//! blinking caret is not allowed. Ratatui hides the terminal cursor whenever
//! a frame did not position it, so "position nothing" is how the composer
//! shows no cursor at all.

use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::tui::app::App;
use codegg::tui::components::prompt::PromptWidget;
use codegg::tui::document_session::TuiDocumentSession;
use codegg::tui::input::InputMode;
use codegg::tui::route::Route;
use codegg::tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::Modifier;
use ratatui::Terminal;
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// A backend that records what the app did with the terminal cursor
// ---------------------------------------------------------------------------

/// `TestBackend` keeps its cursor state private, so a test that wants to
/// assert "the app positioned the terminal cursor" — or "the app left it
/// alone" — has to observe the calls. This backend records the visibility
/// and the position the app asked for.
#[derive(Debug)]
struct RecordingBackend {
    buffer: Buffer,
    cursor_visible: bool,
    cursor_position: Position,
    position_calls: usize,
}

impl RecordingBackend {
    fn new(width: u16, height: u16) -> Self {
        Self {
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            cursor_visible: false,
            cursor_position: Position::new(0, 0),
            position_calls: 0,
        }
    }
}

impl Backend for RecordingBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            self.buffer[(x, y)] = cell.clone();
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.cursor_position)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.cursor_position = position.into();
        self.position_calls += 1;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.buffer.reset();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        if clear_type == ClearType::All {
            self.buffer.reset();
        }
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(Size::new(self.buffer.area.width, self.buffer.area.height))
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(WindowSize {
            columns_rows: Size::new(self.buffer.area.width, self.buffer.area.height),
            pixels: Size::new(0, 0),
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// What one rendered frame left behind.
struct Frame {
    buffer: Buffer,
    cursor_visible: bool,
    cursor_position: Position,
    position_calls: usize,
}

impl Frame {
    fn text(&self) -> String {
        (0..self.buffer.area.height)
            .map(|y| {
                (0..self.buffer.area.width)
                    .map(|x| self.buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn is_reversed(&self, x: u16, y: u16) -> bool {
        self.buffer[(x, y)].modifier.contains(Modifier::REVERSED)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

fn draw(app: &mut App, width: u16, height: u16) -> Frame {
    let backend = RecordingBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .unwrap();
    let backend = terminal.backend();
    Frame {
        buffer: backend.buffer.clone(),
        cursor_visible: backend.cursor_visible,
        cursor_position: backend.cursor_position,
        position_calls: backend.position_calls,
    }
}

/// The caret's cell in the composer's own area, as the app computed it.
fn caret_cell(app: &App) -> (u16, u16) {
    let area = app.prompt_area.expect("the composer is rendered");
    app.prompt_state
        .prompt
        .caret_cell(area)
        .expect("the caret is on screen")
}

/// Take a side panel's focus, the way the app's own focus gesture does.
///
/// A panel is a Normal-mode surface: the app parks the composer's mode when
/// a panel takes the keyboard, and the key chain consults a focused panel
/// only in Normal. Parking the mode here mirrors that.
fn focus_a_side_panel(app: &mut App) {
    assert!(
        app.panel_host.focus(),
        "precondition: the side panel takes the keyboard"
    );
    app.ui_state.input_mode = InputMode::Normal;
}

/// A core client that answers nothing.
///
/// The editor's "the buffer holds the keyboard" fact only needs a session to
/// exist; nothing in this file talks to a daemon.
struct SilentClient;

#[async_trait::async_trait]
impl CoreClient for SilentClient {
    async fn request(
        &self,
        _request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        Err(AppError::Other(anyhow::anyhow!(
            "the composer cursor tests never reach the daemon"
        )))
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        rx
    }
}

// ---------------------------------------------------------------------------
// The caret cell
// ---------------------------------------------------------------------------

/// The caret sits one row below the composer's mode/prefix row, because that
/// row is the badge and the prompt marker.
#[test]
fn the_caret_cell_is_one_row_below_the_prefix_row() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("hello".to_string());
    widget.set_cursor(0);

    let area = Rect::new(4, 10, 40, 5);
    assert_eq!(widget.caret_cell(area), Some((4, 11)));
}

/// The caret advances a cell per character, because the column is a display
/// width and not a byte offset.
#[test]
fn the_caret_cell_follows_the_text_column() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("hello".to_string());

    let area = Rect::new(4, 10, 40, 5);
    widget.set_cursor(3);
    assert_eq!(widget.caret_cell(area), Some((7, 11)));
    widget.set_cursor(5);
    assert_eq!(widget.caret_cell(area), Some((9, 11)));
}

/// A second line of the buffer is a second row of the composer.
#[test]
fn the_caret_cell_is_on_the_second_line_of_a_multiline_buffer() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("one\ntwo".to_string());

    let area = Rect::new(4, 10, 40, 5);
    // Byte 4 is the start of "two": column zero of the second row.
    widget.set_cursor(4);
    assert_eq!(widget.caret_cell(area), Some((4, 12)));
    // Byte 7 is the end of "two": column three of the second row.
    widget.set_cursor(7);
    assert_eq!(widget.caret_cell(area), Some((7, 12)));
}

/// The horizontal scroll moves the text under the caret, so the caret moves
/// with it. A caret that stayed behind would be painted over the wrong
/// character.
#[test]
fn the_caret_cell_follows_the_horizontal_scroll() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("a long single line of prompt text".to_string());
    widget.set_cursor(30);
    widget.horizontal_scroll = 20;

    let area = Rect::new(4, 10, 40, 5);
    assert_eq!(widget.caret_cell(area), Some((14, 11)));
}

/// A caret whose line has scrolled off the top is not on screen, so it has
/// no cell. Returning a cell anyway would put the terminal cursor on a row
/// that shows something else.
#[test]
fn the_caret_cell_is_none_when_the_caret_line_is_scrolled_off() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("one\ntwo\nthree".to_string());
    widget.set_cursor(2);
    widget.scroll = 2;

    let area = Rect::new(4, 10, 40, 5);
    assert_eq!(widget.caret_cell(area), None);
}

/// A zero-sized area has no cells to point at.
#[test]
fn the_caret_cell_is_none_for_an_empty_area() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("hello".to_string());

    assert_eq!(widget.caret_cell(Rect::new(0, 0, 0, 0)), None);
    assert_eq!(widget.caret_cell(Rect::new(0, 0, 10, 0)), None);
    assert_eq!(widget.caret_cell(Rect::new(0, 0, 0, 10)), None);
}

// ---------------------------------------------------------------------------
// Keyboard ownership
// ---------------------------------------------------------------------------

/// The ordinary case: the user is typing in the main buffer.
#[test]
fn the_composer_holds_the_keyboard_in_the_main_buffer() {
    let mut app = test_app();
    assert!(app.composer_holds_keyboard());
}

/// A selected panel is another surface, so the composer does not hold the
/// keyboard. This is the case the user named: "when the LLM buffer is not
/// active, there is no cursor".
#[test]
fn a_focused_side_panel_takes_the_keyboard() {
    let mut app = test_app();
    focus_a_side_panel(&mut app);
    assert!(!app.composer_holds_keyboard());
}

/// Giving the panel back gives the keyboard back, with no explicit
/// invalidation: ownership is derived, so it cannot go stale.
#[test]
fn ownership_follows_the_focus_without_an_invalidation_call() {
    let mut app = test_app();
    assert!(app.composer_holds_keyboard());

    focus_a_side_panel(&mut app);
    assert!(!app.composer_holds_keyboard());

    app.panel_host.blur();
    app.ui_state.input_mode = InputMode::Insert;
    assert!(app.composer_holds_keyboard());
}

/// A modal owns the keyboard outright.
#[test]
fn an_open_dialog_takes_the_keyboard() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Normal;
    app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    assert!(
        !app.focus_manager.is_empty(),
        "precondition: a dialog is open"
    );
    assert!(!app.composer_holds_keyboard());
}

/// The jump-to-message overlay claims the keys before the composer sees
/// them, so the composer must not claim a caret.
#[test]
fn the_timeline_overlay_takes_the_keyboard() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    app.ui_state.timeline_visible = true;

    assert!(!app.composer_holds_keyboard());
}

/// The editor route keeps the ordinary composer editable unless the user
/// explicitly focused the document, so opening the editor does not take the
/// keyboard away from composition.
#[test]
fn the_editor_route_without_a_focused_buffer_leaves_the_composer_alone() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    app.ui_state.routes.navigate_to(Route::Editor);

    assert!(app.composer_holds_keyboard());
}

/// With the document's buffer focused, the document holds the keyboard and
/// the composer shows no caret.
#[test]
fn a_focused_editor_buffer_takes_the_keyboard() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    app.ui_state.routes.navigate_to(Route::Editor);
    app.editor_state.session = Some(TuiDocumentSession::new(Arc::new(SilentClient)));

    assert!(!app.composer_holds_keyboard());

    let frame = draw(&mut app, 100, 30);
    assert_eq!(frame.position_calls, 0, "the app positioned nothing");
    assert!(!frame.cursor_visible, "the terminal cursor stays hidden");
}

// ---------------------------------------------------------------------------
// The block cursor
// ---------------------------------------------------------------------------

/// Normal mode paints a solid block at the caret.
#[test]
fn the_composer_paints_a_block_in_normal_mode() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Normal;
    app.prompt_state.prompt.set_text("hello".to_string());

    let frame = draw(&mut app, 100, 30);
    let (x, y) = caret_cell(&app);
    assert!(frame.is_reversed(x, y), "the caret cell carries the block");
}

/// Insert mode does not paint a block: the terminal's own cursor is the
/// caret there, and painting both would show two carets.
#[test]
fn the_composer_paints_no_block_in_insert_mode() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    app.prompt_state.prompt.set_text("hello".to_string());

    let frame = draw(&mut app, 100, 30);
    let (x, y) = caret_cell(&app);
    assert!(!frame.is_reversed(x, y), "the caret cell carries no block");
}

/// An empty composer still has a caret, and the block is at column zero of
/// the first line. A composer that hides its caret when it is empty is a
/// composer the user cannot find.
#[test]
fn the_composer_paints_a_block_in_normal_mode_even_when_empty() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Normal;
    app.prompt_state.prompt.set_text(String::new());

    let frame = draw(&mut app, 100, 30);
    let (x, y) = caret_cell(&app);
    assert!(
        frame.is_reversed(x, y),
        "the empty composer still has a block"
    );
}

/// A panel that holds the keyboard leaves the composer with no caret at all.
#[test]
fn the_composer_paints_no_block_when_a_panel_holds_the_keyboard() {
    let mut app = test_app();
    focus_a_side_panel(&mut app);
    app.prompt_state.prompt.set_text("hello".to_string());

    let frame = draw(&mut app, 100, 30);
    let (x, y) = caret_cell(&app);
    assert!(
        !frame.is_reversed(x, y),
        "the composer shows no block while a panel has the keyboard"
    );
}

// ---------------------------------------------------------------------------
// The terminal cursor
// ---------------------------------------------------------------------------

/// Insert mode positions the terminal's own cursor at the caret, which is
/// where the terminal's blink comes from — no app-driven cadence.
#[test]
fn insert_mode_positions_the_terminal_cursor_at_the_caret() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    app.prompt_state.prompt.set_text("hello".to_string());

    let frame = draw(&mut app, 100, 30);
    let (x, y) = caret_cell(&app);
    assert_eq!(frame.position_calls, 1, "the cursor was positioned once");
    assert!(frame.cursor_visible, "the terminal cursor is shown");
    assert_eq!(frame.cursor_position, Position::new(x, y));
}

/// An empty composer still positions the terminal cursor: the caret is at
/// column zero of the first line.
#[test]
fn insert_mode_positions_the_terminal_cursor_for_an_empty_composer() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    app.prompt_state.prompt.set_text(String::new());

    let frame = draw(&mut app, 100, 30);
    let (x, y) = caret_cell(&app);
    assert_eq!(frame.cursor_position, Position::new(x, y));
}

/// Normal mode paints the block instead, so the app positions nothing: a
/// positioned terminal cursor would be a second caret.
#[test]
fn normal_mode_positions_no_terminal_cursor() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Normal;
    app.prompt_state.prompt.set_text("hello".to_string());

    let frame = draw(&mut app, 100, 30);
    assert_eq!(frame.position_calls, 0, "the app positioned nothing");
    assert!(!frame.cursor_visible, "the terminal cursor stays hidden");
}

/// A panel that holds the keyboard leaves the terminal cursor alone.
#[test]
fn a_panel_holding_the_keyboard_positions_no_terminal_cursor() {
    let mut app = test_app();
    focus_a_side_panel(&mut app);
    app.prompt_state.prompt.set_text("hello".to_string());

    let frame = draw(&mut app, 100, 30);
    assert_eq!(frame.position_calls, 0, "the app positioned nothing");
    assert!(!frame.cursor_visible, "the terminal cursor stays hidden");
}

/// The block and the terminal cursor are mutually exclusive by construction:
/// a frame carries one or the other, never both.
#[test]
fn the_block_and_the_terminal_cursor_are_never_both_present() {
    for mode in [InputMode::Insert, InputMode::Normal] {
        let mut app = test_app();
        app.ui_state.input_mode = mode;
        app.prompt_state.prompt.set_text("hello".to_string());

        let frame = draw(&mut app, 100, 30);
        let (x, y) = caret_cell(&app);
        let block = frame.is_reversed(x, y);
        let terminal = frame.position_calls > 0;
        assert!(
            !(block && terminal),
            "{mode:?} showed both a block and a terminal cursor"
        );
        assert_eq!(
            block,
            mode == InputMode::Normal,
            "{mode:?} drew the wrong one of the two carets"
        );
    }
}

/// A resize moves the composer, so the caret moves with it. The cursor is
/// positioned per frame from the derived cell, so it cannot stay behind at a
/// stale position.
#[test]
fn the_terminal_cursor_follows_a_resize() {
    let mut app = test_app();
    app.ui_state.input_mode = InputMode::Insert;
    app.prompt_state.prompt.set_text("hello".to_string());

    let _ = draw(&mut app, 100, 30);
    let (first_x, first_y) = caret_cell(&app);

    let frame = draw(&mut app, 60, 40);
    let (second_x, second_y) = caret_cell(&app);
    assert_ne!(
        (first_x, first_y),
        (second_x, second_y),
        "the resize was expected to move the composer"
    );
    assert_eq!(frame.cursor_position, Position::new(second_x, second_y));
}

// ---------------------------------------------------------------------------
// The badge and the cursor agree
// ---------------------------------------------------------------------------

/// The badge is the app's own statement about the mode, and the caret is
/// the user's statement about where they are. They must not disagree.
///
/// The rule, stated once: **when the composer holds the keyboard, the badge
/// says `[INS]` exactly when the terminal cursor is positioned and `[NOR]`
/// exactly when the block is painted. When the composer does not hold the
/// keyboard, neither caret is present at all.**
#[test]
fn the_badge_and_the_cursor_agree_in_every_focus_state() {
    // (description, composer holds the keyboard)
    let cases: Vec<(&str, bool)> = vec![
        ("main buffer, Insert", true),
        ("main buffer, Normal", true),
        ("side panel focused", false),
    ];

    for (description, expect_composer) in cases {
        for mode in [InputMode::Insert, InputMode::Normal] {
            let mut app = test_app();
            app.ui_state.input_mode = mode;
            app.prompt_state.prompt.set_text("hello".to_string());
            if !expect_composer {
                focus_a_side_panel(&mut app);
            }

            let frame = draw(&mut app, 100, 30);
            let (x, y) = caret_cell(&app);
            let block = frame.is_reversed(x, y);
            let terminal = frame.position_calls > 0;
            let text = frame.text();

            if expect_composer {
                // The badge reports the composer's own mode.
                let expected_label = if mode == InputMode::Insert {
                    "[INS]"
                } else {
                    "[NOR]"
                };
                assert!(
                    text.contains(expected_label),
                    "{description} / {mode:?}: the badge does not read {expected_label}"
                );
                assert_eq!(
                    block,
                    mode == InputMode::Normal,
                    "{description} / {mode:?}: the block disagrees with the badge"
                );
                assert_eq!(
                    terminal,
                    mode == InputMode::Insert,
                    "{description} / {mode:?}: the terminal cursor disagrees with the badge"
                );
            } else {
                assert!(
                    !block && !terminal,
                    "{description} / {mode:?}: the composer showed a caret it does not own"
                );
            }
        }
    }
}

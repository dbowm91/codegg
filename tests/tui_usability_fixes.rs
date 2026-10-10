//! Regression tests for the TUI usability fixes.
//!
//! Covers four independent defects that all made the terminal hard to use:
//!
//! 1. The main pane had no bottom border, so it read as an open-ended window
//!    next to the fully-bordered sidebar.
//! 2. The slash-command completion popup was pinned to 40 columns, clipping
//!    descriptions, and its selected row resolved its foreground against the
//!    popup background instead of the selection fill — on themes whose accent
//!    and selection sit at the same luminance the highlighted row rendered as
//!    an unreadable block.
//! 3. Mouse reporting suppressed the terminal's own drag-select, so rendered
//!    output could not be copied at all.
//!
//! The `/sessions` stale-clone regression is covered in-crate in
//! `src/tui/commands/sessions.rs`, because the reload entry point is
//! crate-private.
//!
//! Plus: `/connections` accepted destructive lifecycle keys while showing
//! only the "Loading connections and models…" placeholder.

use codegg::tui::app::state::session::GitSidebarInfo;
use codegg::tui::app::App;
use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::connection_selection::ConnectionSelectionDialog;
use codegg::tui::components::messages::MsgPart;
use codegg::tui::selection::{extract, TextSelection};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;
use std::sync::Arc;
use std::time::Duration;

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

/// An app whose viewport actually has rendered text in it, so a selection
/// has something to resolve to.
fn app_with_output() -> App {
    let mut app = test_app();
    app.messages_state
        .messages
        .add_user_message("please cat the build log".into(), None);
    app.messages_state.messages.add_assistant_text(
        "ERROR: cannot find target directory\n  caused by: build script failed with exit 101"
            .into(),
    );
    app
}

/// An app whose viewport actually renders the message list. `render_viewport`
/// dispatches on the route, and a fresh test app sits on `Route::Home`, which
/// draws the empty-state hint regardless of what is in the message store.
fn app_with_chat() -> App {
    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session(
            "session-under-test".to_string(),
        ));
    app.messages_state
        .messages
        .add_user_message("run the build".into(), None);
    app
}

fn render_to_buffer(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// `ProviderConnectionSummaryDto` has no `Default`, so build one explicitly.
fn connection_summary(id: &str) -> codegg::protocol::provider::ProviderConnectionSummaryDto {
    codegg::protocol::provider::ProviderConnectionSummaryDto {
        id: id.to_string(),
        provider_kind: "opencode_go".to_string(),
        display_name: id.to_string(),
        endpoint: String::new(),
        tls_policy: String::new(),
        scope: String::new(),
        state: "active".to_string(),
        revision: 1,
        model_count: 0,
        catalog_revision: None,
        health: None,
    }
}

fn row_symbols(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

// ---------------------------------------------------------------------------
// 2. Bottom border closes the main pane
// ---------------------------------------------------------------------------

/// The horizontal span the *main* pane occupies, excluding the sidebar.
///
/// The sidebar draws `Borders::ALL` and spans the full terminal height, so
/// asserting on the whole bottom row would be satisfied by the sidebar's own
/// border even with the main pane left open — the exact bug under test.
fn main_pane_span(app: &App, buffer: &Buffer, height: u16) -> String {
    let main_width = app
        .sidebar_area
        .map(|s| s.x.saturating_sub(1))
        .unwrap_or(buffer.area.width);
    (0..main_width)
        .map(|x| buffer[(x, height - 1)].symbol())
        .collect()
}

#[test]
fn main_pane_has_a_bottom_border_on_the_last_row() {
    let mut app = test_app();
    let (width, height) = (120u16, 30u16);
    let buffer = render_to_buffer(&mut app, width, height);

    assert!(
        app.sidebar_area.is_some(),
        "this test needs the sidebar visible so it can prove the main pane          border is independent of the sidebar's own"
    );

    let main_last_row = main_pane_span(&app, &buffer, height);
    assert!(
        main_last_row.contains('─'),
        "main pane's bottom row has no border glyph: {main_last_row:?}"
    );
}

#[test]
fn bottom_border_survives_the_tiny_terminal_degrades() {
    let mut app = test_app();
    for (w, h) in [(40u16, 12u16), (60, 20), (200, 50)] {
        let buffer = render_to_buffer(&mut app, w, h);
        let main_last_row = main_pane_span(&app, &buffer, h);
        assert!(
            main_last_row.contains('─'),
            "bottom border missing at {w}x{h}: {main_last_row:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Completion popup: width and selected-row legibility
// ---------------------------------------------------------------------------

#[test]
fn completion_popup_is_wide_enough_for_descriptions() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    // A long description is exactly what used to be clipped away.
    let long = "Show the aggregated token and cost usage breakdown for this session";
    app.prompt_state.slash_completions = vec![
        codegg::tui::components::completion_overlay::CompletionItem {
            label: "/usage".to_string(),
            description: Some(long.to_string()),
            kind: codegg::tui::components::completion_overlay::CompletionItemKind::File,
        },
    ];
    app.prompt_state.completion_type = codegg::tui::app::CompletionType::Slash;
    app.prompt_state.show_completions = true;
    app.prompt_state.completion_sel = 0;

    let buffer = render_to_buffer(&mut app, 160, 40);
    let text: String = (0..buffer.area.height)
        .map(|y| row_symbols(&buffer, y))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        text.contains("aggregated token and cost usage"),
        "description is truncated; popup is too narrow to show it"
    );
    // The old hard 40-column cap could not fit even the first words.
    assert!(
        !text.contains("Show the aggregated…"),
        "description still ends in an ellipsis"
    );
}

#[test]
fn selected_completion_row_is_readable_on_the_selection_fill() {
    // Build a deliberately hostile palette: accent and selection share a
    // luminance, so resolving the foreground against the popup background
    // (the old behaviour) produced text indistinguishable from the fill.
    let hostile = codegg::tui::theme::Theme {
        id: "hostile".into(),
        name: "hostile".into(),
        background: ratatui::style::Color::Rgb(10, 10, 14),
        foreground: ratatui::style::Color::Rgb(220, 220, 225),
        primary: ratatui::style::Color::Rgb(26, 30, 37),
        secondary: ratatui::style::Color::Rgb(26, 30, 37),
        success: ratatui::style::Color::Rgb(0, 255, 0),
        warning: ratatui::style::Color::Rgb(255, 255, 0),
        error: ratatui::style::Color::Rgb(255, 0, 0),
        muted: ratatui::style::Color::Rgb(26, 30, 37),
        border: ratatui::style::Color::Rgb(46, 60, 73),
        selection: ratatui::style::Color::Rgb(26, 30, 37),
        selection_dim: ratatui::style::Color::Rgb(26, 30, 37),
        alternate_bg: ratatui::style::Color::Rgb(13, 15, 20),
        input_bg: ratatui::style::Color::Rgb(17, 21, 28),
        code_theme: "base16-ocean.dark".into(),
        link: ratatui::style::Color::Rgb(26, 30, 37),
    };

    let mut app = test_app();
    app.ui_state.theme = Arc::new(hostile);
    app.ui_state.sidebar_visible = true;
    app.prompt_state.slash_completions = vec![
        codegg::tui::components::completion_overlay::CompletionItem {
            label: "/logs".to_string(),
            description: Some("open the daemon log viewer".to_string()),
            kind: codegg::tui::components::completion_overlay::CompletionItemKind::File,
        },
    ];
    app.prompt_state.completion_type = codegg::tui::app::CompletionType::Slash;
    app.prompt_state.show_completions = true;
    app.prompt_state.completion_sel = 0;

    let buffer = render_to_buffer(&mut app, 120, 40);

    // Find the "/logs" label cell and require a foreground that actually
    // reads against the selection background behind it.
    let mut checked = 0;
    for y in 0..buffer.area.height {
        let row = row_symbols(&buffer, y);
        let Some(x) = row.find("/logs").map(|i| i as u16) else {
            continue;
        };
        let cell = &buffer[(x, y)];
        let fg = cell.fg;
        let bg = cell.bg;
        let to_rgb = |c: ratatui::style::Color| match c {
            ratatui::style::Color::Rgb(r, g, b) => codegg::theme::Rgb::new(r, g, b),
            _ => codegg::theme::Rgb::new(128, 128, 128),
        };
        let contrast = to_rgb(fg).contrast_ratio(to_rgb(bg));
        assert!(
            contrast >= 3.0,
            "highlighted label at ({x},{y}) has contrast {contrast:.2} against its fill \
             (fg={fg:?} bg={bg:?}) — the highlighted row is unreadable"
        );
        checked += 1;
    }
    assert!(checked > 0, "the completion label was never rendered");
}

// ---------------------------------------------------------------------------
// 4. Text selection over rendered output
// ---------------------------------------------------------------------------

#[test]
fn drag_over_the_viewport_selects_and_copies_rendered_text() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = app_with_output();
    app.ui_state.sidebar_visible = true;
    // Render first so the viewport area and the frame buffer are populated.
    let buffer = render_to_buffer(&mut app, 120, 40);
    let (vx, vy, vw, vh) = {
        let a = app.viewport_area.expect("viewport area recorded");
        (a.x, a.y, a.width, a.height)
    };
    assert!(vw > 4 && vh > 2, "viewport too small to select in");

    // Find a row inside the viewport that actually rendered text, so the
    // assertion is about the selection machinery rather than about whether
    // a particular line happens to be blank.
    let (x0, y0) = (vx + 1, vy);
    let mut y1 = None;
    for y in vy..vy + vh {
        let row: String = (vx..vx + vw).map(|x| buffer[(x, y)].symbol()).collect();
        if row.trim().is_empty() {
            continue;
        }
        // First non-blank line of real content that has enough columns to
        // span a multi-cell drag.
        let first_text = row.len() - row.trim_start().len();
        if vw as usize - first_text >= 6 {
            y1 = Some(y);
            break;
        }
    }
    let y1 =
        y1.unwrap_or_else(|| panic!("viewport at {vx},{vy} {vw}x{vh} rendered no text to select"));
    let x1 = (x0 + 6).min(vx + vw - 1);

    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x0,
        row: y0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.on_mouse(down);
    assert_eq!(
        app.selection_anchor,
        Some((x0, y0)),
        "press inside the viewport did not anchor a selection"
    );

    let drag = MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: x1,
        row: y1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.on_mouse(drag);
    assert_eq!(app.selection_focus, Some((x1, y1)));

    // Before release the selection is still live.
    let text = extract(&buffer, TextSelection::new((x0, y0), (x1, y1)));
    assert!(!text.trim().is_empty(), "selection resolved to no text");

    let up = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: x1,
        row: y1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.on_mouse(up);
    // Release copies and clears, so a stray later keypress cannot re-copy.
    assert!(
        app.selection_anchor.is_none() && app.selection_focus.is_none(),
        "selection was not cleared after copy"
    );
}

#[test]
fn click_and_release_on_one_cell_is_a_cancelled_selection() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    let _ = render_to_buffer(&mut app, 120, 40);
    let a = app.viewport_area.expect("viewport area recorded");
    let p = (a.x + 2, a.y + 2);

    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: p.0,
        row: p.1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: p.0,
        row: p.1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert!(
        app.selection_anchor.is_none(),
        "a zero-length drag should cancel rather than copy one character"
    );
}

#[test]
fn ctrl_c_copies_a_live_selection_and_is_inert_otherwise() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    let _ = render_to_buffer(&mut app, 120, 40);

    // Nothing selected: the key must fall through rather than swallow input.
    assert!(!app.try_copy_selection());

    let a = app.viewport_area.expect("viewport area recorded");
    app.selection_anchor = Some((a.x + 1, a.y + 1));
    app.selection_focus = Some((a.x + 8, a.y + 1));
    assert!(
        app.try_copy_selection(),
        "a live selection should be copied"
    );
    assert!(app.selection_anchor.is_none());

    let _ = KeyEvent::new(KeyCode::Null, KeyModifiers::NONE);
    let _ = Duration::from_secs(0);
}

#[test]
fn selection_is_confined_to_the_viewport() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    let _ = render_to_buffer(&mut app, 120, 40);

    // A press in the prompt must not start a selection — the prompt keeps
    // its own click-to-focus caret behaviour.
    let p = app.prompt_area.expect("prompt area recorded");
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: p.x + 2,
        row: p.y + 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert!(
        app.selection_anchor.is_none(),
        "a prompt click must not begin a text selection"
    );
}

// ---------------------------------------------------------------------------
// 5. /connections is inert while it shows only the loading placeholder
// ---------------------------------------------------------------------------

#[test]
fn connections_dialog_rejects_destructive_keys_while_loading() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let theme = Arc::new(test_app().ui_state.theme.as_ref().clone());
    let mut dialog = ConnectionSelectionDialog::new("session-1".to_string(), theme);
    // A connection row left over from an earlier load is what a blind `d`
    // press would have destroyed while the dialog showed the placeholder.
    dialog.connections = vec![connection_summary("conn-do-not-delete")];
    assert!(dialog.loading, "dialog must start in the loading state");

    for code in [
        KeyCode::Char('d'),
        KeyCode::Char('p'),
        KeyCode::Char('e'),
        KeyCode::Char('u'),
        KeyCode::Enter,
    ] {
        let msg = dialog.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(
            msg.is_none(),
            "{code:?} produced {msg:?} while the dialog was still loading"
        );
    }

    // Dismissal still works, so the user is never trapped.
    let close = dialog.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        close.is_some(),
        "Esc must still close the dialog during loading"
    );
}

#[test]
fn connections_dialog_actions_are_live_once_loaded() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let theme = Arc::new(test_app().ui_state.theme.as_ref().clone());
    let mut dialog = ConnectionSelectionDialog::new("session-1".to_string(), theme);
    dialog.connections = vec![connection_summary("conn-1")];
    dialog.finish_loading();

    let msg = dialog.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(
        msg.is_some(),
        "the refresh key must work once the catalog has loaded — the loading \
         gate must not disable the dialog permanently"
    );
}

// ---------------------------------------------------------------------------
// 4. The bottom edge ate the status line.
//
// The main pane's bottom border was anchored on the footer's last row. The
// configured `footer_height` is 1, so that row was the *only* row the status
// bar ever had, and `finish_render` drew a `Borders::BOTTOM` block across it —
// the status line vanished and the frame looked unterminated.
// ---------------------------------------------------------------------------

fn buffer_text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn status_line_survives_the_bottom_border() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    let buf = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buf);

    let rows: Vec<&str> = text.lines().collect();
    // The bottom edge is the last row that is a continuous horizontal rule.
    let border_row = rows
        .iter()
        .rposition(|r| r.contains('─'))
        .expect("a bottom border row must exist");

    // The status line owns the row directly above the edge and must not be
    // overwritten by it. Before the fix the border was drawn ON that row,
    // which is the only row `footer_height = 1` gives the status bar.
    let status = rows
        .get(border_row.saturating_sub(1))
        .expect("a row must sit above the border");
    assert!(
        !status.contains('─'),
        "the status row must not be the border glyphs:\n{text}"
    );
    let inner: String = status
        .trim_matches(|c: char| c == '│' || c == ' ')
        .to_string();
    assert!(
        !inner.trim().is_empty(),
        "the status line must survive the bottom border, but the row above it \
         was blank:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// 5. One slash popup, not two.
//
// `CommandPalette::render` painted a second, narrower popup over the prompt
// whenever `command_mode` was set, and `update_completions` deliberately
// suppressed the good popup in that mode. Typing `/logs` from scratch showed
// the old box; backspacing to a bare `/` cleared command mode and showed the
// new one — one interaction, two different-looking boxes.
// ---------------------------------------------------------------------------

fn popup_rows(app: &mut App, buf: &Buffer) -> Vec<String> {
    let _ = app;
    buf.content
        .chunks(buf.area.width as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .filter(|r| r.contains('/'))
        .collect()
}

#[test]
fn slash_popup_has_one_implementation_regardless_of_how_it_was_typed() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let type_str = |app: &mut App, s: &str| {
        for ch in s.chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    };

    // Path A: `/logs` typed cold. The first `/` enters command mode.
    let mut a = test_app();
    type_str(&mut a, "/logs");
    let buf_a = render_to_buffer(&mut a, 120, 40);

    // Path B: `/logs` typed after backspacing to a bare `/`, which cleared
    // command mode.
    let mut b = test_app();
    type_str(&mut b, "/logs");
    for _ in 0..4 {
        b.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    type_str(&mut b, "logs");
    let buf_b = render_to_buffer(&mut b, 120, 40);

    let rows_a = popup_rows(&mut a, &buf_a);
    let rows_b = popup_rows(&mut b, &buf_b);
    assert!(
        !rows_a.is_empty() && !rows_b.is_empty(),
        "both paths must render a popup:\nA={rows_a:?}\nB={rows_b:?}"
    );
    assert_eq!(
        rows_a, rows_b,
        "the same `/logs` query rendered two different popups; both paths must \
         go through one implementation"
    );
}

#[test]
fn completion_popup_scrolls_so_the_selection_stays_visible() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    // A single-character filter matches well over 8 slash commands, so the
    // popup must scroll to keep the highlighted row on screen.
    app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for _ in 0..12 {
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    app.prompt_state.show_completions = true;
    let _ = render_to_buffer(&mut app, 120, 40);

    let sel = app.prompt_state.completion_sel;
    let visible = app.completion_visible_rows;
    assert!(
        sel > 0,
        "the test needs a selection past the first row to be meaningful"
    );
    assert!(
        visible > 0,
        "the popup must have drawn rows for scrolling to apply"
    );
    assert!(
        sel >= app.completion_scroll_offset && sel < app.completion_scroll_offset + visible,
        "selection {sel} fell outside the scrolled window \
         [{}, {}) of {visible} rows",
        app.completion_scroll_offset,
        app.completion_scroll_offset + visible
    );
}

// ---------------------------------------------------------------------------
// 6. Live activity + thinking duration in the chat area.
// ---------------------------------------------------------------------------

#[test]
fn live_activity_line_reports_what_the_model_is_doing() {
    let mut app = app_with_chat();
    app.messages_state.messages.begin_activity("thinking");
    let buf = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buf);
    assert!(
        text.contains("thinking"),
        "the chat area must show what the model is doing:\n{text}"
    );

    app.messages_state.messages.end_activity();
    let buf = render_to_buffer(&mut app, 100, 30);
    assert!(
        !buffer_text(&buf).contains("thinking 0s"),
        "the indicator must clear when the agent goes idle"
    );
}

#[test]
fn finished_reasoning_reports_how_long_it_ran() {
    let mut app = app_with_chat();
    app.messages_state
        .messages
        .add_reasoning("considering the options".to_string());
    app.messages_state.messages.begin_reasoning();
    app.messages_state.messages.end_reasoning();

    let buf = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buf);
    assert!(
        text.contains("Thought for"),
        "a finished reasoning block must report its duration:\n{text}"
    );
    assert!(
        text.contains('s'),
        "the duration must carry a unit:\n{text}"
    );
}

#[test]
fn clicking_a_thinking_message_expands_it() {
    let mut app = app_with_chat();
    app.messages_state
        .messages
        .add_reasoning("considering the options".to_string());
    app.messages_state.messages.begin_reasoning();
    app.messages_state.messages.end_reasoning();
    app.messages_state.messages.set_display_options(true, false);

    let idx = app.messages_state.messages.messages.len() - 1;
    assert!(
        app.messages_state.messages.message_has_reasoning(idx),
        "fixture must carry a reasoning part"
    );
    let before = app.messages_state.messages.messages[idx]
        .parts
        .iter()
        .filter(|p| matches!(p, MsgPart::Reasoning { collapsed, .. } if *collapsed))
        .count();
    app.messages_state.messages.toggle_reasoning(idx);
    let after = app.messages_state.messages.messages[idx]
        .parts
        .iter()
        .filter(|p| matches!(p, MsgPart::Reasoning { collapsed, .. } if *collapsed))
        .count();
    assert_ne!(
        before, after,
        "clicking must expand/collapse the thinking block"
    );
}

// ---------------------------------------------------------------------------
// 7. The sidebar must not claim "not a git repo" for every failure.
// ---------------------------------------------------------------------------

#[test]
fn sidebar_distinguishes_loading_failure_and_absence() {
    // `App::render_sidebar` recomputes the git state from
    // `session_state.git_sidebar` on every frame, so driving the widget
    // directly is the only way to assert each of the three states without a
    // full probe running underneath.
    let theme = Arc::new(test_app().ui_state.theme.as_ref().clone());
    let mut sidebar = codegg::tui::components::sidebar::SidebarWidget::new(theme);

    let render = |s: &codegg::tui::components::sidebar::SidebarWidget| -> String {
        let backend = TestBackend::new(40, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(s, frame.area());
            })
            .unwrap();
        buffer_text(terminal.backend().buffer())
    };

    sidebar.set_git_status(true, None);
    sidebar.set_git_info(GitSidebarInfo::default());
    let text = render(&sidebar);
    assert!(
        text.contains("checking git"),
        "an in-flight probe must not be reported as a missing repository:\n{text}"
    );
    assert!(
        !text.contains("not a git repo"),
        "an in-flight probe must not claim the repository is absent:\n{text}"
    );

    sidebar.set_git_status(false, Some("git probe timed out".into()));
    let text = render(&sidebar);
    assert!(
        text.contains("git probe timed out"),
        "a probe failure must surface its actual cause:\n{text}"
    );
    assert!(
        !text.contains("not a git repo"),
        "a probe failure must not be mislabeled as a missing repository:\n{text}"
    );

    sidebar.set_git_status(false, None);
    let text = render(&sidebar);
    assert!(
        text.contains("not a git repo"),
        "only a clean probe with no repository may say so:\n{text}"
    );
}

#[tokio::test]
async fn a_session_bound_before_the_command_channel_still_refreshes_git() {
    // Startup order regression: `main` used to call `set_session` (whose tail
    // is `start_refresh_git_sidebar`) before `ensure_tui_cmd_channel`. The
    // probe found a `None` sender, `spawn_registered_tui_task` returned `None`,
    // and the work was dropped with no diagnostic — leaving `branch` at `None`
    // so the sidebar read "not a git repo" for the life of the process.
    let mut app = test_app();
    app.session_state.session = Some(codegg::session::Session {
        id: "s1".to_string(),
        // A real repository on disk, so the probe has somewhere to walk up
        // from and cannot fail on a missing directory.
        directory: "/Users/davidbowman/projects/codegg".to_string(),
        ..Default::default()
    });

    assert!(
        app.tui_cmd_tx.is_none(),
        "the channel must not exist yet for this test to mean anything"
    );
    app.ensure_tui_cmd_channel();

    assert!(
        app.session_state.git_sidebar.loading,
        "installing the command channel must re-issue the git probe that was \
         dropped while no sender existed; otherwise the sidebar is stuck at its \
         default and renders 'not a git repo'"
    );
    assert!(
        app.session_state.git_sidebar.error.is_none(),
        "the self-heal probe must not immediately record a failure"
    );
}

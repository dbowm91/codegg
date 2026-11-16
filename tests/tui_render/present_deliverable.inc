//! Harness practice adoption M003: the turn-tail deliverable card.
//!
//! The card is derived from the `present` tool result the transcript already
//! carries — there is no second store of what a turn delivered — so these
//! tests drive it the only way production does: record the call and its
//! result in the message list, then read the frame.
//!
//! A refused declaration is the other half. It draws no card; the tool call at
//! its own position carries the failure, and its reason is one disclosure
//! away, exactly as every other failing tool's reason is.

use codegg::session::message::ToolStatus;
use codegg::tui::app::present_card::MAX_PRESENT_CARD_ROWS;
use codegg::tui::app::App;
use codegg::tui::route::Route;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

/// The canonical text `present` returns for a verified declaration.
const DECLARATION: &str =
    "Presented 2 files this turn:\n  src/a.rs\n    the new module\n  src/b.rs";

/// An app on the session route, where the transcript — and therefore the
/// turn-tail card — is the surface on screen.
fn test_app() -> App {
    let mut app = App::new_for_testing("/tmp".into());
    app.ui_state
        .routes
        .navigate_to(Route::Session("session-test".into()));
    app
}

fn render_at(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .expect("the frame draws");
    terminal.backend().buffer().clone()
}

fn buffer_rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

/// Record one `present` call and its result, the way the event path does.
fn record_present(app: &mut App, id: &str, output: &str, success: bool) {
    app.messages_state.messages.add_tool_call(
        id.to_string(),
        "present".to_string(),
        serde_json::json!({"files": []}),
    );
    app.messages_state.messages.update_tool_call(
        id,
        output.to_string(),
        if success {
            ToolStatus::Completed
        } else {
            ToolStatus::Error
        },
        None,
        None,
        None,
    );
}

#[test]
fn the_card_lists_the_declared_files_in_declared_order() {
    let mut app = test_app();
    record_present(&mut app, "call-1", DECLARATION, true);

    let rows = buffer_rows(&render_at(&mut app, 120, 40));

    let header = rows
        .iter()
        .position(|row| row.contains("delivered 2 files"))
        .expect("the card's header is on screen");
    let first = rows
        .iter()
        .position(|row| row.contains("src/a.rs"))
        .expect("the first declared file is on screen");
    let second = rows
        .iter()
        .position(|row| row.contains("src/b.rs"))
        .expect("the second declared file is on screen");
    assert_eq!(header + 1, first, "the first file sits under the header");
    assert_eq!(first + 1, second, "declared order, one row per file");
    assert!(
        rows[first].contains("the new module"),
        "the description renders on the file's own row: {:?}",
        rows[first]
    );
}

#[test]
fn a_session_with_no_declaration_renders_no_card() {
    let mut app = test_app();

    let rows = buffer_rows(&render_at(&mut app, 120, 40));

    assert!(
        !rows.iter().any(|row| row.contains("delivered ")),
        "the card claims no rows when nothing was declared"
    );
}

#[test]
fn a_refused_declaration_is_a_failed_call_with_no_card() {
    let mut app = test_app();
    let refusal = "present refused 1 entry; no file was recorded\n  src/gone.rs: missing — nothing exists at this path";
    record_present(&mut app, "call-1", refusal, false);

    let rows = buffer_rows(&render_at(&mut app, 120, 40));
    assert!(
        !rows.iter().any(|row| row.contains("delivered ")),
        "a refusal draws no card"
    );
    let call_row = rows
        .iter()
        .position(|row| row.contains("present"))
        .expect("the failed call is at its own position");
    assert!(
        rows[call_row].contains("✗ present"),
        "the call is marked failed in the transcript: {:?}",
        rows[call_row]
    );

    // The reason is behind the call's own disclosure, as it is for every other
    // failing tool — this asserts it is reachable there, not that it is lost.
    let index = app.messages_state.messages.message_count() - 1;
    assert!(app.messages_state.messages.toggle_tool_output(index));
    let expanded = buffer_rows(&render_at(&mut app, 120, 40));
    assert!(
        expanded.iter().any(|row| row.contains("src/gone.rs")),
        "the reason names the refused path once the call is expanded"
    );
    assert!(
        !expanded.iter().any(|row| row.contains("delivered ")),
        "expanding the failure still draws no card"
    );
}

#[test]
fn the_card_is_bounded_and_says_how_many_files_it_hides() {
    let mut app = test_app();
    let mut declaration = String::from("Presented 12 files this turn:");
    for index in 0..12 {
        declaration.push_str(&format!("\n  src/f{index}.rs"));
    }
    record_present(&mut app, "call-1", &declaration, true);

    let rows = buffer_rows(&render_at(&mut app, 120, 40));

    let header = rows
        .iter()
        .position(|row| row.contains("delivered 12 files"))
        .expect("the header states the true total");
    assert!(
        rows[header].contains(&format!("showing {}", MAX_PRESENT_CARD_ROWS - 1)),
        "the header states how many of them it is showing: {:?}",
        rows[header]
    );
    let last = rows
        .iter()
        .rposition(|row| row.contains("src/f"))
        .expect("at least one file is listed");
    assert_eq!(
        last - header,
        usize::from(MAX_PRESENT_CARD_ROWS) - 1,
        "the card occupies exactly its bound, header included"
    );
}

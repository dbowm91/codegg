//! Regression coverage for the TUI presentation fixes in this batch:
//! streaming lag, dropped approvals, todo visibility, key remapping,
//! sidebar width, and the live token meter.

use codegg::tui::app::{App, TodoEntry, TuiMsg};
use ratatui::layout::Rect;

// ---------------------------------------------------------------- streaming

/// The live output meter used to re-run a full `cl100k_base` BPE encode over
/// the whole accumulated buffer on every delta, which is quadratic per turn
/// and stalled the single-threaded event loop. It must now accumulate only
/// the new delta.
#[test]
fn live_output_token_estimate_accumulates_incrementally() {
    let mut app = App::new_for_testing("/tmp/codegg-regression-live-tokens".to_string());

    let alpha = "the first chunk of streamed output";
    let beta = "and the second chunk that follows it";

    app.add_live_output_delta(alpha);
    let after_alpha = app.session_state.live_output_tokens;
    app.add_live_output_delta(beta);
    let after_both = app.session_state.live_output_tokens;

    assert!(
        after_alpha > 0,
        "first delta must contribute a non-zero token estimate"
    );
    assert!(
        after_both > after_alpha,
        "the running total must grow as output streams"
    );

    // Additive accumulation: the total is the sum of the per-delta counts,
    // never a re-encode of the joined buffer.
    let expected = codegg::context::compaction::ContextTracker::estimate_tokens(alpha) as u64
        + codegg::context::compaction::ContextTracker::estimate_tokens(beta) as u64;
    assert_eq!(
        after_both, expected,
        "live token total must equal the sum of the per-delta estimates"
    );

    app.reset_live_token_estimate();
    assert_eq!(app.session_state.live_output_tokens, 0);
    assert!(app.session_state.live_output_text.is_empty());
}

#[test]
fn empty_live_output_delta_is_a_no_op() {
    let mut app = App::new_for_testing("/tmp/codegg-regression-live-tokens-empty".to_string());
    app.add_live_output_delta("only");
    let before = app.session_state.live_output_tokens;
    app.add_live_output_delta("");
    assert_eq!(app.session_state.live_output_tokens, before);
}

// ------------------------------------------------------------------- todos

fn entries(specs: &[(&str, &str)]) -> Vec<TodoEntry> {
    specs
        .iter()
        .map(|(content, status)| TodoEntry {
            content: (*content).to_string(),
            status: (*status).to_string(),
            priority: "medium".to_string(),
        })
        .collect()
}

#[test]
fn todo_strip_reserves_no_rows_without_a_todo_list() {
    let app = App::new_for_testing("/tmp/codegg-regression-todo-empty".to_string());
    assert_eq!(app.todo_strip_height(), 0);
}

#[test]
fn todo_strip_is_one_row_compact_and_expands_on_toggle() {
    let mut app = App::new_for_testing("/tmp/codegg-regression-todo-expand".to_string());
    app.set_todos(entries(&[
        ("one", "pending"),
        ("two", "in_progress"),
        ("three", "completed"),
    ]));

    assert_eq!(app.todo_strip_height(), 1, "compact form is a single line");

    app.process_msg(TuiMsg::ToggleTodoList);
    assert!(app.todo_expanded);
    assert_eq!(
        app.todo_strip_height(),
        4,
        "expanded form is a header plus one row per task"
    );

    app.process_msg(TuiMsg::ToggleTodoList);
    assert!(!app.todo_expanded);
    assert_eq!(app.todo_strip_height(), 1);
}

#[test]
fn expanded_todo_strip_is_bounded_for_long_lists() {
    let mut app = App::new_for_testing("/tmp/codegg-regression-todo-bounded".to_string());
    let many: Vec<(&str, &str)> = (0..40).map(|_| ("task", "pending")).collect();
    app.set_todos(entries(&many));
    app.process_msg(TuiMsg::ToggleTodoList);

    let cap = codegg::tui::app::MAX_TODO_STRIP_TASKS as u16;
    assert_eq!(
        app.todo_strip_height(),
        cap + 1,
        "a long list must not grow the strip without bound"
    );
}

#[test]
fn completed_count_excludes_cancelled_and_pending_items() {
    let mut app = App::new_for_testing("/tmp/codegg-regression-todo-count".to_string());
    app.set_todos(entries(&[
        ("done a", "completed"),
        ("done b", "COMPLETED"),
        ("cancelled", "cancelled"),
        ("waiting", "pending"),
        ("blocked", "blocked"),
    ]));
    assert_eq!(app.todos_completed(), 2);
}

#[test]
fn todo_bubble_distinguishes_every_status() {
    assert_eq!(App::todo_bubble("completed"), "\u{2714}");
    assert_eq!(App::todo_bubble("in_progress"), "\u{25cf}");
    assert_eq!(App::todo_bubble("blocked"), "\u{25c8}");
    assert_eq!(App::todo_bubble("cancelled"), "\u{2716}");
    assert_eq!(App::todo_bubble("pending"), "\u{25cb}");
    // Unknown statuses fall back to the "not started" bubble rather than
    // rendering a stray glyph.
    assert_eq!(App::todo_bubble("something-new"), "\u{25cb}");
}

/// `AppEvent::TodoUpdated` is published on the process-local bus, so before
/// this was forwarded as a `CoreEvent` a daemon-connected TUI never saw a
/// todo list change after startup.
#[test]
fn todo_list_updated_core_event_populates_the_strip() {
    let mut app = App::new_for_testing("/tmp/codegg-regression-todo-event".to_string());
    assert!(app.todos.is_empty());

    let event = codegg::protocol::core::CoreEvent::TodoListUpdated {
        session_id: "s-1".into(),
        revision: 7,
        items: vec![
            serde_json::json!({
                "id": "1",
                "content": "Investigate request-direction stream capture",
                "status": "in_progress",
                "priority": "high",
            }),
            serde_json::json!({
                "id": "2",
                "content": "Track C: decide and implement",
                "status": "pending",
                "priority": "medium",
            }),
        ],
    };

    assert!(
        app.apply_core_event(event),
        "a todo update must request a redraw"
    );
    assert_eq!(app.todos.len(), 2);
    assert_eq!(app.todos[0].status, "in_progress");
    assert_eq!(app.todos[0].priority, "high");
    assert_eq!(app.todos[1].status, "pending");
    assert_eq!(app.todos_completed(), 0);
}

#[test]
fn malformed_todo_entries_are_skipped_not_fatal() {
    let mut app = App::new_for_testing("/tmp/codegg-regression-todo-malformed".to_string());
    let event = codegg::protocol::core::CoreEvent::TodoListUpdated {
        session_id: "s-1".into(),
        revision: 1,
        items: vec![
            serde_json::json!({ "status": "pending" }), // no content
            serde_json::json!({ "content": 42 }),       // wrong type
            serde_json::json!({ "content": "survivor" }),
        ],
    };
    app.apply_core_event(event);
    assert_eq!(app.todos.len(), 1);
    assert_eq!(app.todos[0].content, "survivor");
    // Missing fields fall back rather than producing an empty status.
    assert_eq!(app.todos[0].status, "pending");
}

// ------------------------------------------------------------------ layout

#[test]
fn hidden_sidebar_returns_the_full_width_to_the_chat() {
    let app = App::new_for_testing("/tmp/codegg-regression-layout".to_string());
    let layout = &app.ui_state.layout;
    let area = Rect::new(0, 0, 200, 50);

    let visible = layout.split(area, true);
    let hidden = layout.split(area, false);

    assert!(
        visible.len() >= 2,
        "a visible sidebar must reserve its own columns"
    );
    assert_eq!(
        hidden.len(),
        1,
        "a hidden sidebar must not reserve a dead column strip"
    );
    assert_eq!(
        hidden[0].width, area.width,
        "the chat pane must regain the whole window when the sidebar is hidden"
    );
    assert!(
        visible[0].width < area.width,
        "with the sidebar shown, the chat pane is narrower"
    );
}

#[test]
fn narrow_terminal_never_reserves_a_starved_sidebar() {
    let app = App::new_for_testing("/tmp/codegg-regression-layout-narrow".to_string());
    let area = Rect::new(0, 0, 10, 20);
    let chunks = app.ui_state.layout.split(area, true);
    assert_eq!(chunks.len(), 1, "too-narrow frames get no sidebar split");
    assert_eq!(chunks[0], area);
}

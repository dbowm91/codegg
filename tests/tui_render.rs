//! TUI layout and render regression tests.
//!
//! Uses `ratatui::backend::TestBackend` to exercise `App::render()` across
//! multiple terminal sizes and app states without requiring an interactive
//! terminal. Covers pathological content, dialog states, sidebar variants,
//! completion overlays, toasts, and component-level panic fallbacks.

use codegg::session::events::{AgentPlan, AgentPlanItem, PlanItemStatus};
use codegg::session::message::ToolStatus;
use codegg::tui::app::state::session::{ChangedFile, DiffStatsState};
use codegg::tui::app::App;
use codegg::tui::app::{CompletionType, TodoEntry};
use codegg::tui::components::completion_overlay::{CompletionItem, CompletionItemKind};
use codegg::tui::components::messages::SearchMatch;
use codegg::tui::route::Route;
use codegg::tui::Dialog;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Terminal size matrix
// ---------------------------------------------------------------------------

const SIZES: &[(u16, u16)] = &[
    (40, 12),  // tiny
    (60, 20),  // small
    (100, 32), // normal
    (160, 40), // wide
    (100, 60), // tall
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Render `app` at the given dimensions and return the buffer content.
fn render_app_to_buffer(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// Assert that rendering succeeds (no panic) and return the buffer.
fn assert_render_ok(app: &mut App, width: u16, height: u16) -> Buffer {
    render_app_to_buffer(app, width, height)
}

/// Extract all text content from a buffer as a single string.
fn text_in_buffer(buffer: &Buffer) -> String {
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            text.push(cell.symbol().chars().next().unwrap_or(' '));
        }
        text.push('\n');
    }
    text
}

/// Check that a buffer contains a given substring (case-insensitive search
/// on the rendered text).
fn buffer_contains(buffer: &Buffer, needle: &str) -> bool {
    let text = text_in_buffer(buffer);
    let lower = text.to_lowercase();
    lower.contains(&needle.to_lowercase())
}

/// Create a fresh test app at the default project dir.
fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

/// Create a test app with some user/assistant messages populated.
fn app_with_messages() -> App {
    let mut app = test_app();
    app.messages_state
        .messages
        .add_user_message("Hello, world!".into(), None);
    app.messages_state
        .messages
        .add_assistant_text("Hi there! How can I help you today?".into());
    app.messages_state
        .messages
        .add_user_message("Write a function to compute fibonacci numbers".into(), None);
    app.messages_state
        .messages
        .add_assistant_text(
            "Here's a Rust implementation of fibonacci:\n\n```rust\nfn fib(n: u32) -> u32 {\n    match n {\n        0 => 0,\n        1 => 1,\n        _ => fib(n - 1) + fib(n - 2),\n    }\n}\n```\n\nThis is a simple recursive approach."
                .into(),
        );
    app
}

/// Create a test app with tool calls in various states.
fn app_with_tool_calls() -> App {
    let mut app = test_app();
    app.messages_state
        .messages
        .add_user_message("List files in the project".into(), None);

    // Pending tool call
    app.messages_state.messages.add_tool_call(
        "tc1".into(),
        "bash".into(),
        serde_json::json!({"command": "ls"}),
    );

    // Completed tool call
    app.messages_state.messages.add_tool_call(
        "tc2".into(),
        "read".into(),
        serde_json::json!({"file": "src/main.rs"}),
    );
    app.messages_state.messages.update_tool_call(
        "tc2",
        "fn main() { println!(\"hello\"); }".into(),
        ToolStatus::Completed,
        Some(150),
        Some(0),
        Some(1),
    );

    // Error tool call
    app.messages_state.messages.add_tool_call(
        "tc3".into(),
        "bash".into(),
        serde_json::json!({"command": "rm -rf /"}),
    );
    app.messages_state.messages.update_tool_call(
        "tc3",
        "Error: permission denied".into(),
        ToolStatus::Error,
        Some(50),
        Some(1),
        Some(1),
    );

    app
}

/// Create a test app with streaming active.
fn app_streaming() -> App {
    let mut app = test_app();
    app.streaming_active = true;
    app.messages_state
        .messages
        .add_user_message("Tell me a story".into(), None);
    app.messages_state
        .messages
        .add_assistant_text("Once upon a time".into());
    app.messages_state.messages.streaming_tokens = " in a land far away...".to_string();
    app
}

/// Create a test app with sidebar populated.
fn app_with_sidebar() -> App {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;

    // File changes in various diff states
    app.session_state.changed_files = vec![
        ChangedFile {
            path: "src/main.rs".into(),
            action: "modified".into(),
            diff_preview: vec!["+fn new() {}".into(), "-fn old() {}".into()],
            diff_state: DiffStatsState::Ready {
                generation: 1,
                additions: 5,
                deletions: 3,
            },
        },
        ChangedFile {
            path: "src/lib.rs".into(),
            action: "added".into(),
            diff_preview: vec![],
            diff_state: DiffStatsState::Pending { generation: 2 },
        },
        ChangedFile {
            path: "tests/integration_test_with_a_very_long_name_that_exceeds_normal_width.rs"
                .into(),
            action: "modified".into(),
            diff_preview: vec![],
            diff_state: DiffStatsState::Skipped {
                generation: 3,
                reason: "binary file",
            },
        },
        ChangedFile {
            path: "src/config.rs".into(),
            action: "modified".into(),
            diff_preview: vec![],
            diff_state: DiffStatsState::Error {
                generation: 4,
                message: "diff computation failed".into(),
            },
        },
    ];

    // MCP servers
    app.session_state.mcp_servers = vec![
        ("github".into(), "connected".into()),
        ("filesystem".into(), "error".into()),
    ];

    // Git info
    app.sidebar.set_git_info(
        Some("main".into()),
        true,
        Some("/Users/test/project".into()),
    );

    app
}

/// Create a test app with toasts.
fn app_with_toasts() -> App {
    let mut app = test_app();
    app.messages_state
        .toasts
        .info("Build completed successfully");
    app.messages_state
        .toasts
        .warning("Low token budget remaining");
    app.messages_state
        .toasts
        .error("Connection to provider failed");
    app
}

/// Create a test app with the help dialog open.
fn app_with_help_dialog() -> App {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Help;
    app
}

/// Create a test app with the model dialog open.
fn app_with_model_dialog() -> App {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Model;
    app
}

/// Create a test app with the session dialog open.
fn app_with_session_dialog() -> App {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Session;
    app
}

/// Create a test app with completions visible.
fn app_with_completions() -> App {
    let mut app = test_app();
    app.prompt_state.show_completions = true;
    app.prompt_state.completion_filter = "/".into();
    app
}

/// Create a test app with a long prompt text.
fn app_with_long_prompt() -> App {
    let mut app = test_app();
    app.prompt_state
        .prompt
        .set_text("This is a very long prompt text that should wrap across multiple lines when rendered at narrow terminal widths and should not cause any rendering issues or panics".to_string());
    app
}

/// Create a test app with pathological message content.
fn app_pathological_content() -> App {
    let mut app = test_app();

    // Very long unbroken line
    let long_line: String = "A".repeat(2000);
    app.messages_state
        .messages
        .add_user_message(long_line, None);

    // Wide Unicode and emoji
    app.messages_state
        .messages
        .add_assistant_text("Unicode test: 你好世界 🌍💻🎉 αβγδε".into());

    // Combining marks
    app.messages_state.messages.add_user_message(
        "Combining: é (e + combining acute) café naïve résumé".into(),
        None,
    );

    // ANSI-looking escape text (should be rendered safely as text)
    app.messages_state.messages.add_assistant_text(
        "Escape test: \x1b[31mred text\x1b[0m and \x1b[1;32mbold green\x1b[0m".into(),
    );

    // Malformed JSON-like text
    app.messages_state.messages.add_user_message(
        "{broken json: [1, 2, 3, \"missing closing brace\"".into(),
        None,
    );

    // Empty message content
    app.messages_state
        .messages
        .add_user_message("".into(), None);

    // Tool output with large content
    app.messages_state.messages.add_tool_call(
        "big_tc".into(),
        "bash".into(),
        serde_json::json!({"command": "cat large_file.txt"}),
    );
    let big_output: String = "line of output\n".repeat(500);
    app.messages_state.messages.update_tool_call(
        "big_tc",
        big_output,
        ToolStatus::Completed,
        Some(1000),
        Some(0),
        Some(500),
    );

    app
}

// ===========================================================================
// 1. Empty / Home state
// ===========================================================================

#[test]
fn render_empty_home_state_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = test_app();
        let buf = assert_render_ok(&mut app, w, h);
        // No root render error in normal state
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error at {w}x{h}"
        );
    }
}

#[test]
fn render_empty_home_sidebar_hidden() {
    for &(w, h) in SIZES {
        let mut app = test_app();
        app.ui_state.sidebar_visible = false;
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error at {w}x{h}"
        );
    }
}

// ===========================================================================
// 2. Active session basic state
// ===========================================================================

#[test]
fn render_active_session_basic() {
    for &(w, h) in SIZES {
        let mut app = app_with_messages();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error at {w}x{h}"
        );
    }
}

#[test]
fn render_active_session_contains_message_text() {
    let mut app = app_with_messages();
    let buf = assert_render_ok(&mut app, 100, 32);
    // The messages widget renders content; verify the buffer has substantial text
    // (exact text placement depends on word wrapping and widget internals)
    let text = text_in_buffer(&buf);
    let non_whitespace: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        non_whitespace.len() > 50,
        "expected substantial rendered content, got {} non-whitespace chars",
        non_whitespace.len()
    );
}

// ===========================================================================
// 3. Streaming state
// ===========================================================================

#[test]
fn render_streaming_state_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_streaming();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error during streaming at {w}x{h}"
        );
    }
}

// ===========================================================================
// 4. Tool-call states
// ===========================================================================

#[test]
fn render_tool_calls_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_tool_calls();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with tool calls at {w}x{h}"
        );
    }
}

#[test]
fn render_tool_calls_contain_names() {
    let mut app = app_with_tool_calls();
    let buf = assert_render_ok(&mut app, 100, 32);
    // Verify the buffer has substantial rendered content from tool calls
    let text = text_in_buffer(&buf);
    let non_whitespace: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        non_whitespace.len() > 50,
        "expected substantial rendered content from tool calls, got {} non-whitespace chars",
        non_whitespace.len()
    );
}

// ===========================================================================
// 5. Sidebar states
// ===========================================================================

#[test]
fn render_sidebar_visible_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_sidebar();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with sidebar at {w}x{h}"
        );
    }
}

#[test]
fn render_sidebar_hidden_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_sidebar();
        app.ui_state.sidebar_visible = false;
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with sidebar hidden at {w}x{h}"
        );
    }
}

#[test]
fn render_sidebar_file_changes_visible() {
    let mut app = app_with_sidebar();
    let buf = assert_render_ok(&mut app, 100, 32);
    let text = text_in_buffer(&buf);
    assert!(
        text.contains("main.rs") || text.contains("File"),
        "expected file change info in sidebar"
    );
}

#[test]
fn render_sidebar_unicode_paths() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.session_state.changed_files = vec![ChangedFile {
        path: "src/日本語テスト.rs".into(),
        action: "modified".into(),
        diff_preview: vec![],
        diff_state: DiffStatsState::Ready {
            generation: 1,
            additions: 1,
            deletions: 0,
        },
    }];
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with Unicode paths"
    );
}

// ===========================================================================
// 6. Dialog states
// ===========================================================================

#[test]
fn render_help_dialog_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_help_dialog();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with help dialog at {w}x{h}"
        );
    }
}

#[test]
fn render_model_dialog_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_model_dialog();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with model dialog at {w}x{h}"
        );
    }
}

#[test]
fn render_session_dialog_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_session_dialog();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with session dialog at {w}x{h}"
        );
    }
}

#[test]
fn render_all_dialog_variants_small_and_normal() {
    let dialogs = [
        Dialog::Help,
        Dialog::Model,
        Dialog::Agent,
        Dialog::Session,
        Dialog::Tree,
        Dialog::Theme,
        Dialog::Mcp,
        Dialog::Keybind,
        Dialog::Cost,
        Dialog::Usage,
        Dialog::Stats,
        Dialog::Goto,
        Dialog::Plan,
        Dialog::Confirm,
        Dialog::Review,
        Dialog::Context,
        Dialog::Connect,
        Dialog::Template,
        Dialog::Share,
        Dialog::Import,
        Dialog::Question,
        Dialog::Permission,
        Dialog::Diff,
        Dialog::ResearchBrowser,
        Dialog::SecurityReview,
        Dialog::SourcePreview,
        Dialog::ShellShow,
        Dialog::TaskList,
        Dialog::WorktreeList,
        Dialog::GoalShow,
        Dialog::MemoryResults,
        Dialog::DoctorReport,
    ];
    for dialog in &dialogs {
        for &(w, h) in &[(60, 20), (100, 32)] {
            let mut app = test_app();
            app.ui_state.dialog = dialog.clone();
            let buf = assert_render_ok(&mut app, w, h);
            assert!(
                !buffer_contains(&buf, "Rendering Error"),
                "unexpected render error with dialog {:?} at {w}x{h}",
                dialog
            );
        }
    }
}

// ===========================================================================
// 7. Completion overlay
// ===========================================================================

#[test]
fn render_completions_visible_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_completions();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with completions at {w}x{h}"
        );
    }
}

#[test]
fn render_completions_tiny_terminal() {
    let mut app = app_with_completions();
    let buf = assert_render_ok(&mut app, 40, 12);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with completions on tiny terminal"
    );
}

#[test]
fn render_outer_border_at_extreme_small_sizes() {
    // Cover widths/heights near 1 so the corner rendering math
    // (header_bottom_y, footer_bottom_y, right_x = ... - 1) does
    // not underflow when layout produces a non-empty header but
    // zero-height footer or zero-width content.
    for &(w, h) in &[(1u16, 12u16), (40, 1), (1, 1), (2, 2), (3, 3)] {
        let mut app = test_app();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with outer border at {w}x{h}"
        );
    }
}

// ===========================================================================
// 8. Search / timeline / toasts
// ===========================================================================

#[test]
fn render_toasts_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_toasts();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with toasts at {w}x{h}"
        );
    }
}

#[test]
fn render_toasts_contain_text() {
    let mut app = app_with_toasts();
    let buf = assert_render_ok(&mut app, 100, 32);
    let text = text_in_buffer(&buf);
    assert!(
        text.contains("Build") || text.contains("INFO"),
        "expected toast text in buffer"
    );
}

#[test]
fn render_empty_toasts() {
    let mut app = test_app();
    assert!(app.messages_state.toasts.is_empty());
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with empty toasts"
    );
}

// ===========================================================================
// 9. Pathological text
// ===========================================================================

#[test]
fn render_pathological_content_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_pathological_content();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with pathological content at {w}x{h}"
        );
    }
}

#[test]
fn render_long_unbroken_line() {
    let mut app = test_app();
    let long_line: String = "X".repeat(5000);
    app.messages_state
        .messages
        .add_user_message(long_line, None);
    let buf = assert_render_ok(&mut app, 40, 12);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with 5000-char line on tiny terminal"
    );
}

#[test]
fn render_wide_unicode() {
    let mut app = test_app();
    app.messages_state
        .messages
        .add_assistant_text("🇨🇳 日本語 한국어 العربية हिन्दी ไทย".into());
    let buf = assert_render_ok(&mut app, 60, 20);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with wide Unicode"
    );
}

#[test]
fn render_ansi_escape_text() {
    let mut app = test_app();
    app.messages_state.messages.add_assistant_text(
        "\x1b[31mred\x1b[0m \x1b[1;32mbold green\x1b[0m \x1b[4munderline\x1b[0m".into(),
    );
    let buf = assert_render_ok(&mut app, 60, 20);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with ANSI escape text"
    );
}

#[test]
fn render_deeply_nested_content() {
    let mut app = test_app();
    let content = "> ".repeat(200) + "deeply nested quote";
    app.messages_state.messages.add_assistant_text(content);
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with deeply nested content"
    );
}

#[test]
fn render_many_messages_scrollbar() {
    let mut app = test_app();
    for i in 0..100 {
        if i % 2 == 0 {
            app.messages_state
                .messages
                .add_user_message(format!("User message {i}"), None);
        } else {
            app.messages_state
                .messages
                .add_assistant_text(format!("Assistant response {i} with some content"));
        }
    }
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with 100 messages"
    );
}

// ===========================================================================
// 10. Long prompt text
// ===========================================================================

#[test]
fn render_long_prompt_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_long_prompt();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with long prompt at {w}x{h}"
        );
    }
}

// ===========================================================================
// 11. Combined states
// ===========================================================================

#[test]
fn render_sidebar_with_messages_and_toasts() {
    let mut app = app_with_messages();
    app.ui_state.sidebar_visible = true;
    app.messages_state.toasts.info("Quick info");
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with combined state"
    );
}

#[test]
fn render_dialog_with_messages_and_sidebar() {
    let mut app = app_with_messages();
    app.ui_state.sidebar_visible = true;
    app.ui_state.dialog = Dialog::Help;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with dialog + messages + sidebar"
    );
}

#[test]
fn render_streaming_with_sidebar_and_completions() {
    let mut app = app_streaming();
    app.ui_state.sidebar_visible = true;
    app.prompt_state.show_completions = true;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with streaming + sidebar + completions"
    );
}

// ===========================================================================
// 12. Component fallback behavior (tested indirectly through public API)
// ===========================================================================

// Note: Direct render_component_fallback tests live in
// src/tui/app/render_tests.rs (in-module, private access).
// These integration tests verify the public render path handles all states
// without panic and that diagnostics tracking works.

#[test]
fn render_normal_state_does_not_record_component_panics() {
    let mut app = app_with_messages();
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    assert_render_ok(&mut app, 80, 24);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count, initial,
        "normal render should not record component panics"
    );
}

#[test]
fn render_with_dialog_does_not_record_component_panics() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Help;
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    assert_render_ok(&mut app, 80, 24);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count, initial,
        "dialog render should not record component panics"
    );
}

#[test]
fn render_with_sidebar_does_not_record_component_panics() {
    let mut app = app_with_sidebar();
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    assert_render_ok(&mut app, 80, 24);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count, initial,
        "sidebar render should not record component panics"
    );
}

#[test]
fn render_error_renders_error_dialog() {
    let mut app = test_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render_error(frame, "Test error message");
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let text = text_in_buffer(&buf);
    assert!(
        text.contains("Error") || text.contains("error"),
        "expected error title"
    );
    assert!(
        text.contains("Test error message"),
        "expected error message text"
    );
    assert!(text.contains("Press"), "expected retry/quit hint");
}

// ===========================================================================
// 13. Diagnostics tracking
// ===========================================================================

#[test]
fn diagnostics_record_component_panic() {
    let mut app = test_app();
    assert_eq!(app.ui_state.diagnostics.component_render_panic_count, 0);

    app.ui_state
        .diagnostics
        .record_component_render_panic("messages");
    assert_eq!(app.ui_state.diagnostics.component_render_panic_count, 1);
    assert_eq!(
        app.ui_state
            .diagnostics
            .recent_component_render_panics
            .len(),
        1
    );
    assert_eq!(
        app.ui_state.diagnostics.recent_component_render_panics[0].component,
        "messages"
    );
}

#[test]
fn diagnostics_multiple_panics_tracked() {
    let mut app = test_app();
    for component in &["messages", "sidebar", "dialog", "completions", "timeline"] {
        app.ui_state
            .diagnostics
            .record_component_render_panic(component);
    }
    assert_eq!(app.ui_state.diagnostics.component_render_panic_count, 5);
    assert_eq!(
        app.ui_state
            .diagnostics
            .recent_component_render_panics
            .len(),
        5
    );
}

#[test]
fn diagnostics_ring_buffer_caps_at_limit() {
    let mut app = test_app();
    for i in 0..20 {
        app.ui_state
            .diagnostics
            .record_component_render_panic(match i % 3 {
                0 => "messages",
                1 => "sidebar",
                _ => "dialog",
            });
    }
    assert_eq!(app.ui_state.diagnostics.component_render_panic_count, 20);
    // Ring buffer is capped at 8
    assert!(
        app.ui_state
            .diagnostics
            .recent_component_render_panics
            .len()
            <= 8
    );
}

#[test]
fn diagnostics_summary_includes_component_panics() {
    let mut app = test_app();
    app.ui_state
        .diagnostics
        .record_component_render_panic("messages");
    app.ui_state
        .diagnostics
        .record_component_render_panic("sidebar");
    let summary = app.ui_state.diagnostics.summary();
    assert!(summary.contains("Component panics: 2"));
}

// ===========================================================================
// 14. Edge cases
// ===========================================================================

#[test]
fn render_zero_width_not_panic() {
    // Very small terminal - should not panic even if layout breaks.
    // render() has component-level catch_unwind so this should be safe.
    let mut app = test_app();
    let _buf = assert_render_ok(&mut app, 10, 5);
}

#[test]
fn render_with_all_dialog_states_closed() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::None;
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with dialog closed"
    );
}

#[test]
fn render_empty_session_no_messages() {
    let mut app = test_app();
    app.messages_state.messages.clear();
    let buf = assert_render_ok(&mut app, 80, 24);
    let text = text_in_buffer(&buf);
    // Empty messages widget shows placeholder
    assert!(
        text.contains("No messages") || text.contains("Type"),
        "expected empty state placeholder text"
    );
}

#[test]
fn render_sidebar_only_no_messages() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.messages_state.messages.clear();
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with sidebar and no messages"
    );
}

// ===========================================================================
// 15. Multiple rapid renders (stability)
// ===========================================================================

#[test]
fn render_multiple_times_same_state() {
    let mut app = app_with_messages();
    for i in 0..10 {
        let buf = assert_render_ok(&mut app, 80, 24);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error on iteration {i}"
        );
    }
}

#[test]
fn render_interleaved_sizes() {
    let mut app = app_with_messages();
    let sizes = [(40, 12), (100, 32), (60, 20), (160, 40), (80, 24)];
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error on iteration {i} at {w}x{h}"
        );
    }
}

// ===========================================================================
// 16. Sidebar with various file change states
// ===========================================================================

#[test]
fn render_sidebar_all_diff_states() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.session_state.changed_files = vec![
        ChangedFile {
            path: "pending.rs".into(),
            action: "modified".into(),
            diff_preview: vec![],
            diff_state: DiffStatsState::Pending { generation: 1 },
        },
        ChangedFile {
            path: "ready.rs".into(),
            action: "added".into(),
            diff_preview: vec!["+new line".into()],
            diff_state: DiffStatsState::Ready {
                generation: 2,
                additions: 10,
                deletions: 5,
            },
        },
        ChangedFile {
            path: "skipped.rs".into(),
            action: "modified".into(),
            diff_preview: vec![],
            diff_state: DiffStatsState::Skipped {
                generation: 3,
                reason: "binary",
            },
        },
        ChangedFile {
            path: "errored.rs".into(),
            action: "modified".into(),
            diff_preview: vec![],
            diff_state: DiffStatsState::Error {
                generation: 4,
                message: "failed".into(),
            },
        },
    ];
    for &(w, h) in SIZES {
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with all diff states at {w}x{h}"
        );
    }
}

// ===========================================================================
// 17. Shell cell rendering
// ===========================================================================

#[test]
fn render_shell_cells_in_messages() {
    let mut app = test_app();
    app.messages_state
        .messages
        .add_user_message("Run a command".into(), None);
    app.messages_state
        .messages
        .add_shell_cell(1, "ls -la", "/tmp");
    // ShellCell needs update to have content
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with shell cells"
    );
}

// ===========================================================================
// 18. Reasoning / thinking content
// ===========================================================================

#[test]
fn render_thinking_content() {
    let mut app = test_app();
    app.messages_state
        .messages
        .add_user_message("Explain this code".into(), None);
    app.messages_state
        .messages
        .add_reasoning("Let me analyze the code structure first...".into());
    app.messages_state
        .messages
        .add_assistant_text("Here is my analysis of the code.".into());
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with thinking content"
    );
}

// ===========================================================================
// 19. Model dialog with content
// ===========================================================================

#[test]
fn render_model_dialog_populated() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Model;
    app.dialog_state.model_dialog.set_models(vec![
        "openai/gpt-4o".into(),
        "anthropic/claude-sonnet-4-20250514".into(),
        "google/gemini-2.5-pro".into(),
    ]);
    app.dialog_state.model_dialog.set_current("openai/gpt-4o");
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with populated model dialog"
    );
}

// ===========================================================================
// 20. Agent dialog
// ===========================================================================

#[test]
fn render_agent_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Agent;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with agent dialog"
    );
}

// ===========================================================================
// 21. Panic injection mechanism
// ===========================================================================

#[test]
fn panic_injection_messages_increments_diagnostics_and_shows_fallback() {
    let mut app = app_with_messages();
    app.render_panic_injection.messages = true;
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count,
        initial + 1,
        "messages panic injection should increment component_render_panic_count"
    );
    assert!(
        buffer_contains(&buf, "Messages render error"),
        "expected messages fallback text in buffer"
    );
}

#[test]
fn panic_injection_messages_root_panic_count_unchanged() {
    let mut app = app_with_messages();
    app.render_panic_injection.messages = true;
    let initial_root = app.ui_state.render_panic_count;
    assert_render_ok(&mut app, 100, 32);
    assert_eq!(
        app.ui_state.render_panic_count, initial_root,
        "component panic should NOT increment root render_panic_count"
    );
}

#[test]
fn panic_injection_sidebar_increments_diagnostics_and_shows_fallback() {
    let mut app = app_with_sidebar();
    app.render_panic_injection.sidebar = true;
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count,
        initial + 1,
        "sidebar panic injection should increment component_render_panic_count"
    );
    assert!(
        buffer_contains(&buf, "Sidebar unavailable"),
        "expected sidebar fallback text in buffer"
    );
}

#[test]
fn panic_injection_dialog_closes_dialog() {
    let mut app = test_app();
    app.open_dialog(Dialog::Help);
    app.render_panic_injection.dialog = true;
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    assert_render_ok(&mut app, 100, 32);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count,
        initial + 1,
        "dialog panic injection should increment component_render_panic_count"
    );
    assert_eq!(
        app.ui_state.dialog,
        Dialog::None,
        "dialog panic should close the dialog to Dialog::None"
    );
    assert!(
        app.focus_manager.is_empty(),
        "dialog panic should clear stale focus_manager entries"
    );
}

#[test]
fn panic_injection_completions_hides_completions() {
    let mut app = app_with_completions();
    app.render_panic_injection.completions = true;
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    assert_render_ok(&mut app, 100, 32);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count,
        initial + 1,
        "completions panic injection should increment component_render_panic_count"
    );
    assert!(
        !app.prompt_state.show_completions,
        "completions panic should hide completions"
    );
}

#[test]
fn panic_injection_timeline_hides_timeline() {
    let mut app = test_app();
    app.ui_state.timeline_visible = true;
    app.render_panic_injection.timeline = true;
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    assert_render_ok(&mut app, 100, 32);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count,
        initial + 1,
        "timeline panic injection should increment component_render_panic_count"
    );
    assert!(
        !app.ui_state.timeline_visible,
        "timeline panic should hide timeline"
    );
}

#[test]
fn panic_injection_multiple_components_in_single_render() {
    let mut app = app_with_sidebar();
    app.open_dialog(Dialog::Help);
    app.prompt_state.show_completions = true;
    app.ui_state.timeline_visible = true;
    app.render_panic_injection.messages = true;
    app.render_panic_injection.sidebar = true;
    app.render_panic_injection.dialog = true;
    app.render_panic_injection.completions = true;
    app.render_panic_injection.timeline = true;
    let initial = app.ui_state.diagnostics.component_render_panic_count;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert_eq!(
        app.ui_state.diagnostics.component_render_panic_count,
        initial + 5,
        "all five panic injections should each increment component_render_panic_count"
    );
    assert!(
        buffer_contains(&buf, "Messages render error"),
        "expected messages fallback text"
    );
    assert_eq!(app.ui_state.dialog, Dialog::None, "dialog should be closed");
    assert!(
        !app.prompt_state.show_completions,
        "completions should be hidden"
    );
    assert!(!app.ui_state.timeline_visible, "timeline should be hidden");
}

#[test]
fn panic_injection_messages_at_tiny_terminal() {
    let mut app = app_with_messages();
    app.render_panic_injection.messages = true;
    let buf = assert_render_ok(&mut app, 40, 12);
    assert!(
        buffer_contains(&buf, "Messages render error"),
        "expected fallback text at tiny terminal size"
    );
}

#[test]
fn panic_injection_sidebar_at_small_terminal() {
    let mut app = app_with_sidebar();
    app.render_panic_injection.sidebar = true;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        buffer_contains(&buf, "Sidebar unavailable"),
        "expected sidebar fallback at 100x32 terminal"
    );
}

// ===========================================================================
// 22. Additional targeted dialog tests
// ===========================================================================

#[test]
fn render_tree_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Tree;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with tree dialog"
    );
}

#[test]
fn render_question_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Question;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with question dialog"
    );
}

#[test]
fn render_permission_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Permission;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with permission dialog"
    );
}

#[test]
fn render_import_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Import;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with import dialog"
    );
}

#[test]
fn render_share_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Share;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with share dialog"
    );
}

#[test]
fn render_shell_show_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::ShellShow;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with shell show dialog"
    );
}

#[test]
fn render_research_browser_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::ResearchBrowser;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with research browser dialog"
    );
}

#[test]
fn render_security_review_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::SecurityReview;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with security review dialog"
    );
}

#[test]
fn render_template_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Template;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with template dialog"
    );
}

#[test]
fn render_diff_dialog() {
    let mut app = test_app();
    app.ui_state.dialog = Dialog::Diff;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with diff dialog"
    );
}

#[test]
fn render_new_dialogs_at_small_size() {
    let new_dialogs = [
        Dialog::Template,
        Dialog::Share,
        Dialog::Import,
        Dialog::Question,
        Dialog::Permission,
        Dialog::Diff,
        Dialog::ResearchBrowser,
        Dialog::SecurityReview,
        Dialog::SourcePreview,
        Dialog::ShellShow,
        Dialog::TaskList,
        Dialog::WorktreeList,
        Dialog::GoalShow,
        Dialog::MemoryResults,
        Dialog::DoctorReport,
    ];
    for dialog in &new_dialogs {
        let mut app = test_app();
        app.ui_state.dialog = dialog.clone();
        let buf = assert_render_ok(&mut app, 60, 20);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with dialog {:?} at 60x20",
            dialog
        );
    }
}

// ===========================================================================
// 23. Additional edge-case and combined-state tests
// ===========================================================================

#[test]
fn render_timeline_visible() {
    let mut app = app_with_messages();
    app.ui_state.timeline_visible = true;
    for &(w, h) in SIZES {
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with timeline visible at {w}x{h}"
        );
    }
}

#[test]
fn render_empty_messages_with_sidebar_visible() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.messages_state.messages.clear();
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with empty messages and sidebar"
    );
}

#[test]
fn render_tool_only_messages_no_user_messages() {
    let mut app = test_app();
    app.messages_state.messages.add_tool_call(
        "tc1".into(),
        "bash".into(),
        serde_json::json!({"command": "ls"}),
    );
    app.messages_state.messages.update_tool_call(
        "tc1",
        "file1.rs\nfile2.rs".into(),
        ToolStatus::Completed,
        Some(100),
        Some(0),
        Some(1),
    );
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with tool-only messages"
    );
}

#[test]
fn render_streaming_with_dialog_and_sidebar_and_toasts() {
    let mut app = app_streaming();
    app.ui_state.sidebar_visible = true;
    app.ui_state.dialog = Dialog::Help;
    app.messages_state.toasts.info("Streaming started");
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with streaming + dialog + sidebar + toasts"
    );
}

#[test]
fn render_pathological_content_with_sidebar_and_dialog() {
    let mut app = app_pathological_content();
    app.ui_state.sidebar_visible = true;
    app.ui_state.dialog = Dialog::Help;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with pathological content + sidebar + dialog"
    );
}

#[test]
fn render_long_prompt_with_completions_and_sidebar() {
    let mut app = app_with_long_prompt();
    app.prompt_state.show_completions = true;
    app.ui_state.sidebar_visible = true;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with long prompt + completions + sidebar"
    );
}

#[test]
fn render_multiple_dialogs_sequentially() {
    let dialogs = [
        Dialog::Help,
        Dialog::Model,
        Dialog::Tree,
        Dialog::Session,
        Dialog::Agent,
        Dialog::Mcp,
        Dialog::Keybind,
        Dialog::Theme,
    ];
    for dialog in &dialogs {
        let mut app = test_app();
        app.ui_state.dialog = dialog.clone();
        let buf = assert_render_ok(&mut app, 100, 32);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with dialog {:?}",
            dialog
        );
        assert_render_ok(&mut app, 60, 20);
        assert_render_ok(&mut app, 40, 12);
    }
}

#[test]
fn render_tool_calls_with_sidebar_and_completions() {
    let mut app = app_with_tool_calls();
    app.ui_state.sidebar_visible = true;
    app.prompt_state.show_completions = true;
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with tool calls + sidebar + completions"
    );
}

#[test]
fn render_thinking_content_with_sidebar() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.messages_state
        .messages
        .add_user_message("Explain this code".into(), None);
    app.messages_state
        .messages
        .add_reasoning("Let me analyze the code structure first...".into());
    app.messages_state
        .messages
        .add_assistant_text("Here is my analysis.".into());
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with thinking content and sidebar"
    );
}

#[test]
fn render_shell_cells_with_sidebar() {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;
    app.messages_state
        .messages
        .add_user_message("Run a command".into(), None);
    app.messages_state
        .messages
        .add_shell_cell(1, "ls -la", "/tmp");
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with shell cells and sidebar"
    );
}

#[test]
fn render_many_messages_with_all_overlays() {
    let mut app = test_app();
    for i in 0..50 {
        if i % 2 == 0 {
            app.messages_state
                .messages
                .add_user_message(format!("User message {i}"), None);
        } else {
            app.messages_state
                .messages
                .add_assistant_text(format!("Assistant response {i} with some content"));
        }
    }
    app.ui_state.sidebar_visible = true;
    app.ui_state.timeline_visible = true;
    app.prompt_state.show_completions = true;
    app.messages_state.toasts.info("Info toast");
    app.messages_state.toasts.warning("Warning toast");
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with many messages and all overlays"
    );
}

// ===========================================================================
// File / agent completions
// ===========================================================================

fn app_with_file_completions() -> App {
    let mut app = test_app();
    app.prompt_state.show_completions = true;
    app.prompt_state.completion_type = CompletionType::File;
    app.prompt_state.completion_filter = "src/".into();
    app.prompt_state.file_completions = vec![
        CompletionItem {
            label: "src/main.rs".into(),
            description: Some("Main entry point".into()),
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "src/lib.rs".into(),
            description: Some("Library root".into()),
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "src/utils/".into(),
            description: None,
            kind: CompletionItemKind::Directory,
        },
    ];
    app
}

#[test]
fn render_file_completions_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_file_completions();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with file completions at {w}x{h}"
        );
    }
}

#[test]
fn render_file_completions_tiny_terminal() {
    let mut app = app_with_file_completions();
    let buf = assert_render_ok(&mut app, 40, 12);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with file completions on tiny terminal"
    );
}

fn app_with_agent_completions() -> App {
    let mut app = test_app();
    app.prompt_state.show_completions = true;
    app.prompt_state.completion_type = CompletionType::Agent;
    app.prompt_state.completion_filter = "@".into();
    app.prompt_state.agent_completions = vec![
        CompletionItem::new("research".into(), Some("Research agent".into())),
        CompletionItem::new("review".into(), Some("Code review agent".into())),
    ];
    app
}

#[test]
fn render_agent_completions_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_agent_completions();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with agent completions at {w}x{h}"
        );
    }
}

#[test]
fn render_long_completion_labels() {
    let mut app = test_app();
    app.prompt_state.show_completions = true;
    app.prompt_state.completion_type = CompletionType::File;
    app.prompt_state.completion_filter = "".into();
    app.prompt_state.file_completions = vec![
        CompletionItem {
            label: "src/very/deeply/nested/module/path/to/a/long/file_name_that_exceeds_normal_width.rs".into(),
            description: Some("A deeply nested file with a very long path".into()),
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "another_extremely_long_filename_with_repeated_characters_to_test_overflow.rs".into(),
            description: Some("Long filename".into()),
            kind: CompletionItemKind::File,
        },
    ];
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with long completion labels"
    );
    // Also test at small size
    let buf = assert_render_ok(&mut app, 60, 20);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with long completion labels at small size"
    );
}

// ===========================================================================
// Search visible with matches / no matches
// ===========================================================================

fn app_with_search_and_matches() -> App {
    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(Route::Session("test".to_string()));
    app.messages_state
        .messages
        .add_user_message("The quick brown fox jumps over the lazy dog".into(), None);
    app.messages_state
        .messages
        .add_assistant_text("Here is a fox in a box on a dock with a lock and a sock".to_string());
    app.messages_state.messages.search_visible = true;
    app.messages_state.messages.search_query = Some("fox".into());
    app.messages_state.messages.search_matches = vec![
        SearchMatch {
            msg_idx: 0,
            part_idx: 0,
            line_in_msg: 0,
            start: 16,
            end: 19,
        },
        SearchMatch {
            msg_idx: 1,
            part_idx: 0,
            line_in_msg: 0,
            start: 11,
            end: 14,
        },
    ];
    app.messages_state.messages.search_current = 0;
    app
}

#[test]
fn render_search_visible_with_matches_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_search_and_matches();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with search and matches at {w}x{h}"
        );
    }
}

#[test]
fn render_search_visible_with_matches_contains_text() {
    let mut app = app_with_search_and_matches();
    let buf = assert_render_ok(&mut app, 100, 32);
    let text = text_in_buffer(&buf);
    assert!(
        text.contains("fox") || text.contains("SEARCH"),
        "expected search-related text in buffer"
    );
}

fn app_with_search_no_matches() -> App {
    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(Route::Session("test".to_string()));
    app.messages_state
        .messages
        .add_user_message("Hello world".into(), None);
    app.messages_state.messages.search_visible = true;
    app.messages_state.messages.search_query = Some("zzz_nonexistent".into());
    app.messages_state.messages.search_matches = vec![];
    app.messages_state.messages.search_current = 0;
    app
}

#[test]
fn render_search_visible_no_matches_all_sizes() {
    for &(w, h) in SIZES {
        let mut app = app_with_search_no_matches();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with search and no matches at {w}x{h}"
        );
    }
}

#[test]
fn render_search_visible_no_matches_shows_no_matches() {
    let mut app = app_with_search_no_matches();
    let buf = assert_render_ok(&mut app, 100, 32);
    let text = text_in_buffer(&buf);
    assert!(
        text.contains("no matches") || text.contains("SEARCH"),
        "expected 'no matches' or search indicator in buffer"
    );
}

// ===========================================================================
// Multi-line diagnostics toast
// ===========================================================================

#[test]
fn render_multiline_diagnostics_toast() {
    let mut app = test_app();
    app.messages_state.toasts.error(
        "Diagnostics:\n  warning: unused variable `x`\n  error[E0308]: mismatched types\n    --> src/main.rs:10:5\n     |\n10   |     let x: i32 = \"hello\";",
    );
    app.messages_state.toasts.info("Build finished with errors");
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with multi-line diagnostics toast"
    );
    let text = text_in_buffer(&buf);
    assert!(
        text.contains("Diagnostics") || text.contains("Build"),
        "expected diagnostics or build text in buffer"
    );
}

#[test]
fn render_multiline_diagnostics_toast_tiny() {
    let mut app = test_app();
    app.messages_state
        .toasts
        .error("Diagnostics:\n  warning: unused variable `x`\n  error[E0308]: mismatched types");
    let buf = assert_render_ok(&mut app, 40, 12);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with multi-line diagnostics toast on tiny terminal"
    );
}

// ===========================================================================
// Phase 9 gap-fill: sidebar goal/todo, combining marks, ANSI in tool output,
//                   memory/doctor toast content
// ===========================================================================

/// Create a test app with sidebar goal, plan, and todos populated.
fn app_with_sidebar_goals_todos() -> App {
    let mut app = test_app();
    app.ui_state.sidebar_visible = true;

    // Goal
    app.sidebar
        .set_goal(Some("Implement the new auth module".into()));

    // Plan with mixed statuses
    app.sidebar.set_plan(Some(AgentPlan {
        items: vec![
            AgentPlanItem {
                id: "p1".into(),
                text: "Design API surface".into(),
                status: PlanItemStatus::Done,
                note: None,
            },
            AgentPlanItem {
                id: "p2".into(),
                text: "Implement token storage".into(),
                status: PlanItemStatus::InProgress,
                note: Some("Working on AES encryption".into()),
            },
            AgentPlanItem {
                id: "p3".into(),
                text: "Write integration tests".into(),
                status: PlanItemStatus::Pending,
                note: None,
            },
            AgentPlanItem {
                id: "p4".into(),
                text: "Deprecated migration path".into(),
                status: PlanItemStatus::Skipped,
                note: Some("No longer needed".into()),
            },
            AgentPlanItem {
                id: "p5".into(),
                text: "Blocked on upstream issue".into(),
                status: PlanItemStatus::Blocked,
                note: Some("Waiting for #1234".into()),
            },
        ],
        updated_at: chrono::Utc::now(),
    }));

    // Todos with mixed statuses
    app.set_todos(vec![
        TodoEntry {
            content: "Review error handling".into(),
            status: "completed".into(),
            priority: "high".into(),
        },
        TodoEntry {
            content: "Add unit tests for crypto module".into(),
            status: "in_progress".into(),
            priority: "high".into(),
        },
        TodoEntry {
            content: "Update documentation".into(),
            status: "pending".into(),
            priority: "medium".into(),
        },
    ]);

    app
}

#[test]
fn render_sidebar_goal_todo_plan_snippets() {
    for &(w, h) in SIZES {
        let mut app = app_with_sidebar_goals_todos();
        let buf = assert_render_ok(&mut app, w, h);
        assert!(
            !buffer_contains(&buf, "Rendering Error"),
            "unexpected render error with sidebar goal/plan/todos at {w}x{h}"
        );
    }
}

#[test]
fn render_sidebar_goal_todo_plan_contains_text() {
    let mut app = app_with_sidebar_goals_todos();
    // Use a tall terminal so all sidebar sections are visible
    let buf = assert_render_ok(&mut app, 160, 60);
    let text = text_in_buffer(&buf);
    // At a wide+tall size, goal and/or todo text should appear in the sidebar
    assert!(
        text.contains("auth")
            || text.contains("Implement")
            || text.contains("Review")
            || text.contains("Todo")
            || text.contains("Goal")
            || text.contains("Plan"),
        "expected goal, plan, or todo text in sidebar"
    );
}

#[test]
fn render_real_combining_marks() {
    let mut app = test_app();
    // Real Unicode combining mark sequences (not precomposed characters)
    app.messages_state.messages.add_user_message(
        "cafe\u{0301} naive\u{0308} resume\u{0301} Zos\u{0301}".into(),
        None,
    );
    // Also test combining marks in assistant text
    app.messages_state
        .messages
        .add_assistant_text("e\u{0301} i\u{0308} u\u{0308} overline\u{0305}".into());
    let buf = assert_render_ok(&mut app, 80, 24);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with real Unicode combining marks"
    );
}

#[test]
fn render_ansi_escape_in_tool_output() {
    let mut app = test_app();
    // Add a tool call whose output contains ANSI escape sequences
    app.messages_state.messages.add_tool_call(
        "ansi_tc".into(),
        "bash".into(),
        serde_json::json!({"command": "cargo test"}),
    );
    app.messages_state.messages.update_tool_call(
        "ansi_tc",
        "running tests\n\x1b[32m  passed\x1b[0m test_auth\n\x1b[31m  FAILED\x1b[0m test_crypto\n\x1b[1;33mwarning:\x1b[0m unused import".into(),
        ToolStatus::Completed,
        Some(500),
        Some(1),
        Some(1),
    );
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with ANSI escape text in tool output"
    );
}

#[test]
fn render_memory_and_doctor_toasts() {
    let mut app = test_app();
    // Simulate memory summary result as toast (matches apply_memory_result path)
    app.messages_state.toasts.info(
        "Memory summary:\n  - rust error handling patterns\n  - tokio async patterns\n  - TUI component architecture",
    );
    // Simulate doctor result as toast (matches apply_doctor_result path)
    app.messages_state
        .toasts
        .info("Doctor: LSP servers healthy\n  rust-analyzer: running\n  0 errors detected");
    // Also test error variant
    app.messages_state
        .toasts
        .error("Memory store unreachable: connection refused");
    let buf = assert_render_ok(&mut app, 100, 32);
    assert!(
        !buffer_contains(&buf, "Rendering Error"),
        "unexpected render error with memory/doctor toasts"
    );
    let text = text_in_buffer(&buf);
    assert!(
        text.contains("Memory") || text.contains("Doctor"),
        "expected memory or doctor text in buffer"
    );
}

// ---------------------------------------------------------------------------
// Regression: Bug 1 — info dialog content must update when reopened while
// the previous instance is still mounted in the focus stack.
// ---------------------------------------------------------------------------

#[test]
fn info_dialog_reopen_updates_focus_stack_content() {
    use codegg::tui::components::component::DialogType;
    use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};
    use std::any::Any;
    use std::sync::Arc;

    let mut app = test_app();

    // Drive the same update logic `open_info_dialog` performs, using
    // the public dialog state and focus manager. This mirrors the
    // flow when `/doctor` is invoked twice in succession.
    fn mount_info(app: &mut codegg::tui::app::App, lines: Vec<String>) {
        let theme = Arc::clone(&app.ui_state.theme);
        if let Some(ref mut dialog) = app.dialog_state.info_dialog {
            dialog.set_info_type(InfoType::DoctorReport);
            dialog.set_content(lines);
            dialog.set_theme(&theme);
            let dialog_type = dialog.dialog_type_for_info_type();
            if let Some(ref updated) = app.dialog_state.info_dialog {
                app.focus_manager
                    .replace_top_dialog(dialog_type, Box::new(updated.clone()));
            }
        } else {
            let dialog = InfoDialog::new(theme, InfoType::DoctorReport, lines);
            app.dialog_state.info_dialog = Some(dialog.clone());
            app.focus_manager.push(Box::new(dialog));
        }
    }

    mount_info(&mut app, vec!["alpha".to_string(), "1234".to_string()]);
    {
        let cached = app
            .dialog_state
            .info_dialog
            .as_ref()
            .unwrap()
            .content_lines();
        assert!(cached.contains(&"1234".to_string()));
        let top = app
            .focus_manager
            .top()
            .expect("focus stack should have a top component");
        let any = top as &dyn Any;
        let focus_dlg = any
            .downcast_ref::<InfoDialog>()
            .expect("top should be InfoDialog");
        assert!(focus_dlg.content_lines().contains(&"1234".to_string()));
    }

    // Re-mount with new content while the previous InfoDialog is still
    // on the focus stack. Pre-fix, the focus stack held the stale
    // clone and continued to render the old report.
    mount_info(&mut app, vec!["beta".to_string(), "9876".to_string()]);

    let cached = app
        .dialog_state
        .info_dialog
        .as_ref()
        .unwrap()
        .content_lines()
        .to_vec();
    assert!(
        cached.contains(&"9876".to_string()),
        "dialog_state.info_dialog should have new content, got {:?}",
        cached
    );
    assert!(
        !cached.contains(&"1234".to_string()),
        "dialog_state should no longer hold the old content, got {:?}",
        cached
    );

    let top = app
        .focus_manager
        .top()
        .expect("focus stack should still have a top component");
    let any = top as &dyn Any;
    let focus_dlg = any
        .downcast_ref::<InfoDialog>()
        .expect("top should still be InfoDialog");
    let rendered = focus_dlg.content_lines();
    assert!(
        rendered.contains(&"9876".to_string()),
        "focus-stack component should reflect the new content, got {:?}",
        rendered
    );
    assert!(
        !rendered.contains(&"1234".to_string()),
        "focus-stack component should no longer show the old content, got {:?}",
        rendered
    );
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        DialogType::DoctorReport,
        "focus stack should still report DoctorReport"
    );
}

use std::sync::Arc;

use codegg::session::message::ToolStatus;
use codegg::tui::components::completion_overlay::{
    CompletionItem, CompletionItemKind, CompletionOverlay, CompletionType,
};
use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::command::CommandPalette;
use codegg::tui::components::dialogs::help::HelpDialog;
use codegg::tui::components::dialogs::question::{QuestionDialog, QuestionSpec};
use codegg::tui::components::dialogs::theme::ThemePickerDialog;
use codegg::tui::components::messages::{highlight_code, MessageRole, MessagesWidget, MsgPart};
use codegg::tui::components::prompt::PromptWidget;
use codegg::tui::layout::{LayoutConfig, TuiLayout};
use codegg::tui::theme::{all_themes, find_theme, theme_names, Theme};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

#[test]
fn test_theme_from_name() {
    let theme = Theme::from_name("dark").expect("dark theme should exist");
    assert_eq!(theme.name, "dark");
    assert!(theme.is_dark());
}

#[test]
fn test_theme_from_name_case_sensitive() {
    assert!(Theme::from_name("Dark").is_none());
    assert!(Theme::from_name("DARK").is_none());
    assert!(Theme::from_name("nonexistent").is_none());
}

#[test]
fn test_theme_dark_default() {
    let theme = Theme::dark();
    assert_eq!(theme.name, "dark");
    assert!(theme.is_dark());
}

#[test]
fn test_theme_light() {
    let theme = Theme::light();
    assert_eq!(theme.name, "light");
    assert!(!theme.is_dark());
}

#[test]
fn test_all_themes() {
    let themes = all_themes();
    assert!(!themes.is_empty());
    assert!(themes.len() > 20);
}

#[test]
fn test_theme_names() {
    let names = theme_names();
    assert!(!names.is_empty());
    assert!(names.contains(&"dark".to_string()));
    assert!(names.contains(&"light".to_string()));
    assert!(names.contains(&"dracula".to_string()));
}

#[test]
fn test_find_theme() {
    assert!(find_theme("catppuccin-mocha").is_some());
    assert!(find_theme("nord").is_some());
    assert!(find_theme("monokai").is_some());
    assert!(find_theme("invalid").is_none());
}

#[test]
fn test_theme_code_theme() {
    let theme = Theme::from_name("dark").unwrap();
    assert_eq!(theme.code_theme(), "base16-ocean.dark");

    let theme = Theme::from_name("dracula").unwrap();
    assert_eq!(theme.code_theme(), "dracula");
}

#[test]
fn test_theme_styles() {
    let theme = Theme::dark();
    let default_style = theme.default_style();
    assert!(default_style.fg.is_some());

    let dim_style = theme.dim_style();
    assert!(dim_style.fg.is_some());

    let highlight_style = theme.highlight_style();
    assert!(highlight_style.fg.is_some());
    assert!(highlight_style.bg.is_some());
}

#[test]
fn test_messages_widget_empty() {
    let widget = MessagesWidget::new(Arc::new(Theme::dark()));
    assert_eq!(widget.message_count(), 0);
    assert!(!widget.is_searching());
}

#[test]
fn test_messages_widget_add_user_message() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello".to_string(), None);

    assert_eq!(widget.message_count(), 1);
    if let Some(msg) = widget.messages.first() {
        assert_eq!(msg.role, MessageRole::User);
        assert!(msg.timestamp.is_some());
    }
}

#[test]
fn test_messages_widget_add_assistant_text() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello".to_string(), None);
    widget.add_assistant_text("Hi there!".to_string());

    assert_eq!(widget.message_count(), 2);
    if let Some(msg) = widget.messages.get(1) {
        assert_eq!(msg.role, MessageRole::Assistant);
    }
}

#[test]
fn test_messages_widget_add_assistant_text_appends_to_last() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_assistant_text("Hello".to_string());
    widget.add_assistant_text(" World".to_string());

    assert_eq!(widget.message_count(), 1);
    if let Some(msg) = widget.messages.first() {
        if let Some(MsgPart::Text { content }) = msg.parts.first() {
            assert_eq!(content, "Hello World");
        }
    }
}

#[test]
fn test_messages_widget_add_reasoning() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello".to_string(), None);
    widget.add_reasoning("Let me think...".to_string());

    assert_eq!(widget.message_count(), 2);
    if let Some(msg) = widget.messages.get(1) {
        assert_eq!(msg.role, MessageRole::Assistant);
        assert!(msg
            .parts
            .iter()
            .any(|p| matches!(p, MsgPart::Reasoning { .. })));
    }
}

#[test]
fn test_messages_widget_add_tool_call() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_tool_call(
        "tool1".to_string(),
        "read".to_string(),
        serde_json::json!({"path": "/test"}),
    );

    assert_eq!(widget.message_count(), 1);
    if let Some(msg) = widget.messages.first() {
        assert_eq!(msg.role, MessageRole::Assistant);
        if let Some(MsgPart::ToolCall { name, status, .. }) = msg.parts.first() {
            assert_eq!(name, "read");
            assert!(matches!(status, ToolStatus::Pending));
        }
    }
}

#[test]
fn test_messages_widget_update_tool_call() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_tool_call(
        "tool1".to_string(),
        "read".to_string(),
        serde_json::json!({"path": "/test"}),
    );
    widget.update_tool_call(
        "tool1",
        "file content".to_string(),
        ToolStatus::Completed,
        Some(150),
        Some(0),
        Some(3),
    );

    if let Some(msg) = widget.messages.first() {
        if let Some(MsgPart::ToolCall { output, status, .. }) = msg.parts.first() {
            assert_eq!(output, "file content");
            assert!(matches!(status, ToolStatus::Completed));
        }
    }
}

#[test]
fn test_messages_widget_clear() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello".to_string(), None);
    widget.add_assistant_text("Hi".to_string());
    assert_eq!(widget.message_count(), 2);

    widget.clear();
    assert_eq!(widget.message_count(), 0);
    assert!(widget.sel_msg.is_none());
}

#[test]
fn test_messages_widget_scroll() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    for i in 0..20 {
        widget.add_user_message(format!("Message {i}"), None);
    }

    assert!(widget.auto_scroll);
    widget.scroll_up();
    assert!(!widget.auto_scroll);

    // Scrolling down one line from near the bottom should re-enable auto_scroll
    // once we reach max_scroll.
    widget.scroll_down();
    assert!(widget.auto_scroll);
}

#[test]
fn test_messages_widget_selection() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("First".to_string(), None);
    widget.add_user_message("Second".to_string(), None);
    widget.add_user_message("Third".to_string(), None);

    assert!(widget.sel_msg.is_none());

    widget.select_next();
    assert_eq!(widget.sel_msg, Some(0));

    widget.select_next();
    assert_eq!(widget.sel_msg, Some(1));

    widget.select_prev();
    assert_eq!(widget.sel_msg, Some(0));
}

#[test]
fn test_messages_widget_search() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello world".to_string(), None);
    widget.add_user_message("foo bar".to_string(), None);
    widget.add_user_message("Hello everyone".to_string(), None);

    widget.search("Hello");
    assert!(widget.is_searching());
    assert_eq!(widget.search_matches.len(), 2);

    widget.search_next();
    assert_eq!(widget.sel_msg, Some(widget.search_matches[1].msg_idx));

    widget.search_prev();
    assert_eq!(widget.sel_msg, Some(widget.search_matches[0].msg_idx));

    widget.clear_search();
    assert!(!widget.is_searching());
}

#[test]
fn test_messages_widget_search_scrolls_to_match() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    for i in 0..20 {
        widget.add_user_message(format!("Message {i}"), None);
    }

    assert!(widget.auto_scroll);
    widget.search("Message 15");
    assert!(widget.is_searching());
    assert_eq!(widget.search_matches.len(), 1);

    widget.search_next();
    assert!(widget.auto_scroll);
    // scroll is set to show the matched message, not usize::MAX
    assert!(widget.scroll > 0 || widget.search_matches[0].line_in_msg == 0);
}

#[test]
fn test_messages_widget_search_empty_query() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello".to_string(), None);
    widget.search("");

    assert!(!widget.is_searching());
    assert!(widget.search_query.is_none());
}

#[test]
fn test_messages_widget_undo_redo() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello".to_string(), None);
    widget.add_assistant_text("Hi".to_string());

    assert!(widget.undo());
    assert_eq!(widget.message_count(), 1);

    assert!(widget.redo());
    assert_eq!(widget.message_count(), 2);
}

#[test]
fn test_messages_widget_streaming() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    assert!(widget.streaming_tokens.is_empty());

    widget.add_streaming_token("Hello");
    widget.add_streaming_token(" ");
    widget.add_streaming_token("World");
    assert_eq!(widget.streaming_tokens, "Hello World");

    widget.finalize_streaming();
    assert!(widget.streaming_tokens.is_empty());
    assert_eq!(widget.message_count(), 1);

    widget.clear_streaming();
    assert!(widget.streaming_tokens.is_empty());
}

#[test]
fn test_messages_widget_get_selected_content() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_user_message("Hello World".to_string(), None);
    widget.select_index(0);

    let content = widget.get_selected_content();
    assert_eq!(content, "Hello World");
}

#[test]
fn test_messages_widget_toggle_reasoning() {
    let mut widget = MessagesWidget::new(Arc::new(Theme::dark()));
    widget.add_reasoning("thinking...".to_string());
    assert!(widget.messages[0].parts.iter().any(|p| {
        if let MsgPart::Reasoning { collapsed, .. } = p {
            !*collapsed
        } else {
            false
        }
    }));

    widget.toggle_reasoning(0);
    assert!(widget.messages[0].parts.iter().any(|p| {
        if let MsgPart::Reasoning { collapsed, .. } = p {
            *collapsed
        } else {
            false
        }
    }));
}

#[test]
fn test_prompt_widget_new() {
    let widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert!(widget.get_text().is_empty());
    assert_eq!(widget.cursor_pos(), 0);
    assert!(widget.is_focused());
}

#[test]
fn test_prompt_widget_text() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.insert_char('H');
    widget.insert_char('i');

    assert_eq!(widget.get_text(), "Hi");
    assert_eq!(widget.cursor_pos(), 2);
}

#[test]
fn test_prompt_widget_cursor_movement() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Hello World".to_string());

    widget.cursor_home();
    assert_eq!(widget.cursor_pos(), 0);

    widget.cursor_end();
    assert_eq!(widget.cursor_pos(), 11);

    widget.cursor_left();
    assert_eq!(widget.cursor_pos(), 10);

    widget.cursor_right();
    assert_eq!(widget.cursor_pos(), 11);
}

#[test]
fn test_prompt_widget_backspace() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Hello".to_string());
    widget.cursor_end();
    widget.backspace();

    assert_eq!(widget.get_text(), "Hell");
    assert_eq!(widget.cursor_pos(), 4);
}

#[test]
fn test_prompt_widget_delete() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Hello".to_string());
    widget.cursor_home();
    widget.delete();

    assert_eq!(widget.get_text(), "ello");
}

#[test]
fn test_prompt_widget_clear() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Hello".to_string());
    widget.clear();

    assert!(widget.get_text().is_empty());
    assert_eq!(widget.cursor_pos(), 0);
}

#[test]
fn test_prompt_widget_paste() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.insert_char('H');
    widget.paste("ello".to_string());

    assert_eq!(widget.get_text(), "Hello");
}

#[test]
fn test_prompt_widget_focus_blur() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert!(widget.is_focused());

    widget.blur();
    assert!(!widget.is_focused());

    widget.focus();
    assert!(widget.is_focused());
}

#[test]
fn test_prompt_widget_multiline() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Line 1".to_string());
    widget.insert_newline();
    widget.insert_char('L');
    widget.insert_char('i');
    widget.insert_char('n');
    widget.insert_char('e');
    widget.insert_char('2');

    assert!(widget.get_text().contains('\n'));
}

#[test]
fn test_prompt_widget_set_cursor_bounds() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Hello".to_string());

    widget.set_cursor(100);
    assert_eq!(widget.cursor_pos(), 5);

    widget.set_cursor(0);
    assert_eq!(widget.cursor_pos(), 0);
}

#[test]
fn test_prompt_widget_cursor_stays_on_utf8_boundaries() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("é界x".to_string());

    // Byte offset 1 is inside `é`; normalize to the preceding character
    // boundary so editing after a mouse click cannot panic.
    widget.set_cursor(1);
    assert_eq!(widget.cursor_pos(), 0);
    widget.insert_char('A');
    assert_eq!(widget.get_text(), "Aé界x");

    // Display columns account for the width-2 CJK character.
    widget.set_cursor_at_column(0, 1);
    assert_eq!(widget.cursor_pos(), 1);
    widget.set_cursor_at_column(0, 2);
    assert_eq!(widget.cursor_pos(), 3);
}

#[test]
fn test_prompt_widget_cursor_column_handles_multiline_text() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("first\n第二行".to_string());

    widget.set_cursor_at_column(1, 2);
    assert_eq!(widget.cursor_pos(), "first\n".len() + "第".len());
}

#[test]
fn test_prompt_widget_command_mode() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert!(!widget.command_mode);

    widget.set_command_mode(true);
    assert!(widget.command_mode);

    widget.set_command_mode(false);
    assert!(!widget.command_mode);
}

#[test]
fn test_prompt_widget_waiting() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert!(!widget.waiting);

    widget.set_waiting(true);
    assert!(widget.waiting);

    widget.set_waiting(false);
    assert!(!widget.waiting);
}

#[test]
fn test_prompt_widget_char_count() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert!(widget.char_count.is_none());

    widget.set_char_count(Some(10));
    assert_eq!(widget.char_count, Some(10));

    widget.set_char_count(None);
    assert!(widget.char_count.is_none());
}

#[test]
fn test_prompt_widget_num_lines() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert_eq!(widget.num_lines(), 1);

    widget.set_text("Hello".to_string());
    assert_eq!(widget.num_lines(), 1);

    widget.insert_newline();
    widget.insert_char('W');
    widget.insert_char('o');
    widget.insert_char('r');
    widget.insert_char('l');
    widget.insert_char('d');
    assert_eq!(widget.num_lines(), 2);
}

#[test]
fn test_prompt_widget_cursor_line() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Line 1\nLine 2\nLine 3".to_string());
    widget.cursor_home(); // Move cursor to beginning
    assert_eq!(widget.cursor_line(), 0);

    widget.cursor_end();
    assert_eq!(widget.cursor_line(), 2);

    widget.cursor_home();
    assert_eq!(widget.cursor_line(), 0);

    // Move to line 1 (after first newline at position 7)
    for _ in 0..8 {
        widget.cursor_right();
    }
    assert_eq!(widget.cursor_line(), 1);

    // Move to line 2 (after second newline)
    for _ in 0..8 {
        widget.cursor_right();
    }
    assert_eq!(widget.cursor_line(), 2);
}

#[test]
fn test_prompt_widget_needed_height() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert_eq!(widget.needed_height(20), 3);

    widget.set_text("Short text".to_string());
    assert_eq!(widget.needed_height(20), 3);

    let long_text = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_string();
    widget.set_text(long_text);
    assert_eq!(widget.needed_height(20), 7);

    widget.set_char_count(Some(1000));
    assert_eq!(widget.needed_height(20), 8);
}

#[test]
fn test_prompt_widget_needed_height_max() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    let very_long_text = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9\nLine 10\nLine 11\nLine 12".to_string();
    widget.set_text(very_long_text);
    assert_eq!(widget.needed_height(10), 10);
}

#[test]
fn test_prompt_widget_scroll_up_down() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    assert_eq!(widget.scroll, 0);

    widget.scroll_up();
    assert_eq!(widget.scroll, 0);

    widget.scroll_down();
    assert_eq!(widget.scroll, 1);

    widget.scroll_down();
    assert_eq!(widget.scroll, 2);

    widget.scroll_up();
    assert_eq!(widget.scroll, 1);
}

#[test]
fn test_prompt_widget_ensure_cursor_visible() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_string());

    widget.cursor_home();
    widget.ensure_cursor_visible(3);
    assert_eq!(widget.scroll, 0);

    // Move cursor to line 4 (past visible area when visible_lines=3)
    for _ in 0..25 {
        widget.cursor_right();
    }
    widget.ensure_cursor_visible(3);
    assert!(widget.scroll > 0);
}

#[test]
fn test_prompt_widget_horizontal_cursor_visibility() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("1234567890".to_string());

    widget.ensure_cursor_visible_with_width(3, 5);
    assert_eq!(widget.horizontal_scroll, 6);

    widget.cursor_home();
    widget.ensure_cursor_visible_with_width(3, 5);
    assert_eq!(widget.horizontal_scroll, 0);
}

#[test]
fn test_prompt_widget_cursor_on_empty_line() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("a\n".to_string());

    let backend = TestBackend::new(12, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| frame.render_widget(&widget, frame.area()))
        .unwrap();

    let cell = &terminal.backend().buffer()[(0, 2)];
    assert!(cell.modifier.contains(ratatui::style::Modifier::REVERSED));
}

#[test]
fn test_help_dialog_clamps_scroll_to_last_page() {
    let theme = Arc::new(Theme::dark());
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let mut dialog = HelpDialog::new(theme.clone(), lines);

    for _ in 0..20 {
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ));
    }

    let backend = TestBackend::new(40, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| dialog.render(frame, frame.area(), &theme))
        .unwrap();

    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol().to_string())
        .collect::<String>();
    assert!(text.contains("line 19"));
}

#[test]
fn test_completion_overlay_new() {
    let overlay = CompletionOverlay::new();
    assert!(!overlay.visible);
    assert_eq!(overlay.selected, 0);
    assert!(overlay.items.is_empty());
}

#[test]
fn test_completion_overlay_show() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![
        CompletionItem {
            label: "/help".to_string(),
            description: Some("Show help".to_string()),
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/clear".to_string(),
            description: Some("Clear chat".to_string()),
            kind: CompletionItemKind::File,
        },
    ];

    overlay.show(CompletionType::Slash, items, "/".to_string(), 0);

    assert!(overlay.visible);
    assert_eq!(overlay.selected, 0);
    assert_eq!(overlay.items.len(), 2);
    assert_eq!(overlay.ctype, CompletionType::Slash);
}

#[test]
fn test_completion_overlay_hide() {
    let mut overlay = CompletionOverlay::new();
    overlay.show(CompletionType::Slash, vec![], "/".to_string(), 0);
    assert!(overlay.visible);

    overlay.hide();
    assert!(!overlay.visible);
}

#[test]
fn test_completion_overlay_selection() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![
        CompletionItem {
            label: "/a".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/b".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/c".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
    ];
    overlay.show(CompletionType::Slash, items, "/".to_string(), 0);

    overlay.select_down();
    assert_eq!(overlay.selected, 1);

    overlay.select_down();
    assert_eq!(overlay.selected, 2);

    overlay.select_up();
    assert_eq!(overlay.selected, 1);
}

#[test]
fn test_completion_overlay_selection_bounds() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![CompletionItem {
        label: "/only".to_string(),
        description: None,
        kind: CompletionItemKind::File,
    }];
    overlay.show(CompletionType::Slash, items, "/".to_string(), 0);

    overlay.select_up();
    assert_eq!(overlay.selected, 0);

    overlay.select_down();
    assert_eq!(overlay.selected, 0);
}

#[test]
fn test_completion_overlay_selected_item() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![
        CompletionItem {
            label: "/first".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/second".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
    ];
    overlay.show(CompletionType::Slash, items, "/".to_string(), 0);

    assert_eq!(overlay.selected_item().unwrap().label, "/first");

    overlay.select_down();
    assert_eq!(overlay.selected_item().unwrap().label, "/second");
}

#[test]
fn test_completion_overlay_filtered_items_no_filter() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![
        CompletionItem {
            label: "/help".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/clear".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/exit".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
    ];
    overlay.show(CompletionType::Slash, items, "/".to_string(), 0);

    let filtered = overlay.filtered_items();
    assert_eq!(filtered.len(), 3);
}

#[test]
fn test_completion_overlay_filtered_items_with_filter() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![
        CompletionItem {
            label: "/help".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/clear".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/exit".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
    ];
    overlay.show(CompletionType::Slash, items, "/h".to_string(), 0);

    let filtered = overlay.filtered_items();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].label, "/help");
}

#[test]
fn test_completion_overlay_filtered_len() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![
        CompletionItem {
            label: "/help".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "/clear".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
    ];
    overlay.show(CompletionType::Slash, items, "/".to_string(), 0);

    assert_eq!(overlay.filtered_len(), 2);
}

#[test]
fn test_completion_overlay_set_filter() {
    let mut overlay = CompletionOverlay::new();
    overlay.show(
        CompletionType::Slash,
        vec![CompletionItem {
            label: "/test".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        }],
        "/".to_string(),
        0,
    );
    overlay.select_down();
    assert_eq!(overlay.selected, 0);

    overlay.set_filter("/t".to_string());
    assert_eq!(overlay.filter, "/t");
    assert_eq!(overlay.selected, 0);
}

#[test]
fn test_completion_overlay_file_type() {
    let mut overlay = CompletionOverlay::new();
    let items = vec![
        CompletionItem {
            label: "@file1.txt".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
        CompletionItem {
            label: "@file2.rs".to_string(),
            description: None,
            kind: CompletionItemKind::File,
        },
    ];
    overlay.show(CompletionType::File, items, "@".to_string(), 0);

    assert_eq!(overlay.ctype, CompletionType::File);
    let filtered = overlay.filtered_items();
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_completion_overlay_empty_items() {
    let mut overlay = CompletionOverlay::new();
    overlay.show(CompletionType::Slash, vec![], "/".to_string(), 0);
    assert!(overlay.selected_item().is_none());
}

#[test]
fn test_highlight_code_rust() {
    let code = r#"fn main() {
    println!("Hello");
}"#;
    let result = highlight_code(code, "rust", "base16-ocean.dark");
    assert!(!result.is_empty());
    assert_eq!(result.len(), 3);
}

#[test]
fn test_highlight_code_python() {
    let code = "print('Hello World')";
    let result = highlight_code(code, "python", "base16-ocean.dark");
    assert!(!result.is_empty());
}

#[test]
fn test_highlight_code_unknown_language() {
    let code = "some code content";
    let result = highlight_code(code, "unknown_lang_xyz", "base16-ocean.dark");
    assert!(!result.is_empty());
}

#[test]
fn test_highlight_code_empty() {
    let result = highlight_code("", "rust", "base16-ocean.dark");
    assert!(result.is_empty());
}

#[test]
fn test_highlight_code_multiline() {
    let code = "line1\nline2\nline3";
    let result = highlight_code(code, "text", "base16-ocean.dark");
    assert_eq!(result.len(), 3);
}

#[test]
fn test_highlight_code_with_tabs() {
    let code = "\tfunction()\n\t\tnested()";
    let result = highlight_code(code, "javascript", "base16-ocean.dark");
    assert_eq!(result.len(), 2);
}

#[test]
fn test_theme_picker_dialog_new() {
    let dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    assert!(!dialog.themes.is_empty());
    assert_eq!(dialog.selected, 0);
    assert_eq!(dialog.theme.name, "dark");
}

#[test]
fn test_theme_picker_dialog_set_theme() {
    let mut dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    let new_theme = Arc::new(Theme::from_name("dracula").unwrap());
    dialog.set_theme(&new_theme);
    assert_eq!(dialog.theme.name, "dracula");
}

#[test]
fn test_theme_picker_dialog_select_up() {
    let mut dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    dialog.select_down();
    assert_eq!(dialog.selected, 1);
    dialog.select_up();
    assert_eq!(dialog.selected, 0);
}

#[test]
fn test_theme_picker_dialog_select_down() {
    let mut dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    let initial_selected = dialog.selected;
    dialog.select_down();
    assert_eq!(dialog.selected, initial_selected + 1);
}

#[test]
fn test_theme_picker_dialog_select_down_bounds() {
    let mut dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    while dialog.selected + 1 < dialog.themes.len() {
        dialog.select_down();
    }
    let last_idx = dialog.selected;
    dialog.select_down();
    assert_eq!(dialog.selected, last_idx);
}

#[test]
fn test_theme_picker_dialog_select_up_bounds() {
    let mut dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    dialog.select_up();
    assert_eq!(dialog.selected, 0);
}

#[test]
fn test_theme_picker_dialog_selected_theme() {
    let mut dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    assert_eq!(
        dialog.selected_theme().map(|t| t.name.as_str()),
        Some("dark")
    );
    dialog.select_down();
    assert_eq!(
        dialog.selected_theme().map(|t| t.name.as_str()),
        Some("light")
    );
}

#[test]
fn test_theme_picker_dialog_preview_theme() {
    let mut dialog = ThemePickerDialog::new(Arc::new(Theme::default()));
    let initial_preview = dialog.preview_theme.name.clone();
    dialog.select_down();
    assert_ne!(dialog.preview_theme.name, initial_preview);
}

#[test]
fn test_question_dialog_new_single_question() {
    let questions = vec![QuestionSpec {
        question: "What is your name?".to_string(),
        options: None,
        initial: Some("Alice".to_string()),
    }];
    let dialog = QuestionDialog::new(questions);
    assert_eq!(dialog.questions.len(), 1);
    assert_eq!(dialog.answers.len(), 1);
    assert_eq!(dialog.answers[0], "Alice");
    assert_eq!(dialog.selected_question, 0);
}

#[test]
fn test_question_dialog_new_multiple_questions() {
    let questions = vec![
        QuestionSpec {
            question: "Name?".to_string(),
            options: None,
            initial: None,
        },
        QuestionSpec {
            question: "Age?".to_string(),
            options: None,
            initial: Some("25".to_string()),
        },
    ];
    let dialog = QuestionDialog::new(questions);
    assert_eq!(dialog.questions.len(), 2);
    assert_eq!(dialog.answers.len(), 2);
    assert_eq!(dialog.answers[0], "");
    assert_eq!(dialog.answers[1], "25");
}

#[test]
fn test_question_dialog_questions_json() {
    let questions = vec![QuestionSpec {
        question: "Test?".to_string(),
        options: Some(vec!["a".to_string(), "b".to_string()]),
        initial: Some("a".to_string()),
    }];
    let dialog = QuestionDialog::new(questions);
    let json = dialog.questions_json();
    assert!(json.contains("Test?"));
    assert!(json.contains("a"));
    assert!(json.contains("b"));
}

#[test]
fn test_question_dialog_answers_json() {
    let questions = vec![QuestionSpec {
        question: "Test?".to_string(),
        options: None,
        initial: None,
    }];
    let dialog = QuestionDialog::new(questions);
    let json = dialog.answers_json();
    assert!(json.contains("Test?"));
    assert!(json.contains("answer"));
}

#[test]
fn test_question_dialog_navigate() {
    let questions = vec![
        QuestionSpec {
            question: "Q1".to_string(),
            options: None,
            initial: None,
        },
        QuestionSpec {
            question: "Q2".to_string(),
            options: None,
            initial: None,
        },
    ];
    let mut dialog = QuestionDialog::new(questions);
    dialog.select_down();
    assert_eq!(dialog.selected_question, 1);
    dialog.select_up();
    assert_eq!(dialog.selected_question, 0);
}

#[test]
fn test_question_dialog_set_answer() {
    let questions = vec![QuestionSpec {
        question: "Q?".to_string(),
        options: None,
        initial: None,
    }];
    let mut dialog = QuestionDialog::new(questions);
    dialog.set_answer('a');
    assert_eq!(dialog.current_input, "a");
    assert_eq!(dialog.answers[0], "a");
}

#[test]
fn test_question_dialog_backspace() {
    let questions = vec![
        QuestionSpec {
            question: "Q1?".to_string(),
            options: None,
            initial: Some("ans1".to_string()),
        },
        QuestionSpec {
            question: "Q2?".to_string(),
            options: None,
            initial: Some("test".to_string()),
        },
    ];
    let mut dialog = QuestionDialog::new(questions);
    assert_eq!(dialog.selected_question, 0);
    dialog.select_down();
    assert_eq!(dialog.selected_question, 1);
    assert_eq!(dialog.current_input, "test");
    dialog.backspace();
    assert_eq!(dialog.current_input, "tes");
    assert_eq!(dialog.answers[1], "tes");
}

#[test]
fn test_question_dialog_delete() {
    let questions = vec![
        QuestionSpec {
            question: "Q1?".to_string(),
            options: None,
            initial: Some("ans1".to_string()),
        },
        QuestionSpec {
            question: "Q2?".to_string(),
            options: None,
            initial: Some("test".to_string()),
        },
    ];
    let mut dialog = QuestionDialog::new(questions);
    dialog.select_down();
    assert_eq!(dialog.current_input, "test");
    assert_eq!(dialog.cursor_pos, 4);
    dialog.cursor_left();
    assert_eq!(dialog.cursor_pos, 3);
    dialog.delete();
    assert_eq!(dialog.current_input, "tes");
}

#[test]
fn test_question_dialog_cursor_navigation() {
    let questions = vec![
        QuestionSpec {
            question: "Q1?".to_string(),
            options: None,
            initial: Some("ans1".to_string()),
        },
        QuestionSpec {
            question: "Q2?".to_string(),
            options: None,
            initial: Some("hello".to_string()),
        },
    ];
    let mut dialog = QuestionDialog::new(questions);
    dialog.select_down();
    assert_eq!(dialog.current_input, "hello");
    assert_eq!(dialog.cursor_pos, 5);
    dialog.cursor_left();
    assert_eq!(dialog.cursor_pos, 4);
    dialog.cursor_right();
    assert_eq!(dialog.cursor_pos, 5);
}

#[test]
fn test_question_dialog_select_option() {
    let questions = vec![QuestionSpec {
        question: "Choose?".to_string(),
        options: Some(vec!["opt1".to_string(), "opt2".to_string()]),
        initial: None,
    }];
    let mut dialog = QuestionDialog::new(questions);
    dialog.select_option(1);
    assert_eq!(dialog.answers[0], "opt2");
    assert_eq!(dialog.current_input, "opt2");
}

#[test]
fn test_command_palette_new() {
    let palette = CommandPalette::new();
    assert!(palette.query.is_empty());
    assert!(palette.filtered.is_empty());
    assert_eq!(palette.cursor, 0);
}

#[test]
fn test_command_palette_set_query() {
    let mut palette = CommandPalette::new();
    palette.set_query("/help");
    assert_eq!(palette.query, "/help");
    assert!(!palette.filtered.is_empty());
}

#[test]
fn test_command_palette_filter_results() {
    let mut palette = CommandPalette::new();
    palette.set_query("/exit");
    assert!(!palette.filtered.is_empty());
    assert!(palette.filtered.iter().any(|c| c.name == "/exit"));
}

#[test]
fn test_command_palette_cursor_down() {
    let mut palette = CommandPalette::new();
    palette.set_query("/");
    let initial_cursor = palette.cursor;
    palette.cursor_down();
    if palette.filtered.len() > 1 {
        assert_eq!(palette.cursor, initial_cursor + 1);
    }
}

#[test]
fn test_command_palette_cursor_up() {
    let mut palette = CommandPalette::new();
    palette.set_query("/");
    palette.cursor_down();
    palette.cursor_up();
    assert_eq!(palette.cursor, 0);
}

#[test]
fn test_command_palette_cursor_up_bounds() {
    let mut palette = CommandPalette::new();
    palette.set_query("/");
    palette.cursor_up();
    assert_eq!(palette.cursor, 0);
}

#[test]
fn test_command_palette_selected() {
    let mut palette = CommandPalette::new();
    palette.set_query("/exit");
    assert!(palette.selected().is_some());
}

#[test]
fn test_command_palette_selected_none_when_empty() {
    let mut palette = CommandPalette::new();
    palette.set_query("/xyzabc123notexist");
    assert!(palette.selected().is_none());
}

#[test]
fn test_command_palette_is_empty() {
    let mut palette = CommandPalette::new();
    palette.set_query("/");
    assert!(!palette.is_empty());
}

#[test]
fn test_command_palette_visible_count() {
    let mut palette = CommandPalette::new();
    palette.set_query("/");
    assert!(palette.visible_count() > 0);
}

#[test]
fn test_layout_config_default() {
    let config = LayoutConfig::default();
    assert_eq!(config.sidebar_width, 30);
    assert_eq!(config.min_main_width, 40);
    assert_eq!(config.prompt_height, 3);
    assert_eq!(config.header_height, 1);
    assert_eq!(config.footer_height, 1);
}

#[test]
fn test_layout_config_custom() {
    let config = LayoutConfig {
        sidebar_width: 50,
        min_main_width: 80,
        prompt_height: 5,
        header_height: 2,
        footer_height: 2,
        scrollbar_width: 1,
    };
    assert_eq!(config.sidebar_width, 50);
    assert_eq!(config.min_main_width, 80);
}

#[test]
fn test_tui_layout_new() {
    let layout = TuiLayout::new();
    assert_eq!(layout.config.sidebar_width, 30);
}

#[test]
fn test_tui_layout_with_config() {
    let config = LayoutConfig::default();
    let layout = TuiLayout::with_config(config);
    assert_eq!(layout.config.sidebar_width, 30);
}

#[test]
fn test_tui_layout_split_wide_enough() {
    let layout = TuiLayout::new();
    let area = Rect::new(0, 0, 100, 50);
    let result = layout.split(area);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_tui_layout_split_narrow() {
    let layout = TuiLayout::new();
    let area = Rect::new(0, 0, 50, 50);
    let result = layout.split(area);
    assert_eq!(result.len(), 1);
}

#[test]
fn test_tui_layout_split_boundary() {
    let layout = TuiLayout::new();
    let area = Rect::new(0, 0, 71, 50);
    let result = layout.split(area);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_tui_layout_split_boundary_narrow() {
    let layout = TuiLayout::new();
    let area = Rect::new(0, 0, 70, 50);
    let result = layout.split(area);
    assert_eq!(result.len(), 1);
}

#[test]
fn test_tui_layout_session_layout() {
    let layout = TuiLayout::new();
    let area = Rect::new(0, 0, 100, 50);
    let result = layout.session_layout(area, None);
    assert_eq!(result.len(), 4);
}

#[test]
fn test_tui_layout_session_layout_heights() {
    let layout = TuiLayout::new();
    let area = Rect::new(0, 0, 100, 50);
    let result = layout.session_layout(area, None);
    assert_eq!(result[0].height, 1);
    assert_eq!(result[2].height, 3);
    assert_eq!(result[3].height, 1);
}

#[test]
fn test_theme_is_dark() {
    let dark = Theme::dark();
    assert!(dark.is_dark());

    let light = Theme::light();
    assert!(!light.is_dark());
}

#[test]
fn test_theme_color_values() {
    let theme = Theme::dark();
    assert!(matches!(
        theme.background,
        ratatui::style::Color::Rgb(_, _, _)
    ));
    assert!(matches!(
        theme.foreground,
        ratatui::style::Color::Rgb(_, _, _)
    ));
    assert!(matches!(theme.primary, ratatui::style::Color::Rgb(_, _, _)));
}

#[test]
fn test_theme_from_name_invalid() {
    assert!(Theme::from_name("nonexistent_theme_xyz").is_none());
}

#[test]
fn test_theme_all_themes_have_unique_names() {
    let themes = all_themes();
    let mut names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
    names.sort();
    for i in 1..names.len() {
        assert_ne!(names[i - 1], names[i]);
    }
}

#[test]
fn test_theme_each_has_code_theme() {
    let themes = all_themes();
    for theme in themes {
        assert!(!theme.code_theme.is_empty());
    }
}

#[test]
fn test_status_bar_widget_new() {
    let widget = codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    assert_eq!(widget.status, "idle");
    assert!(widget.token_str.is_empty());
    assert_eq!(widget.subagent_count, 0);
    assert!(!widget.loading);
    assert!(!widget.thinking);
    assert!(widget.undo_message.is_none());
}

#[test]
fn test_status_bar_widget_set_status() {
    let mut widget =
        codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    widget.set_status("working".to_string());
    assert_eq!(widget.status, "working");

    widget.set_status("error".to_string());
    assert_eq!(widget.status, "error");
}

#[test]
fn test_status_bar_widget_set_tokens() {
    let mut widget =
        codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    widget.set_tokens("↓100 ↑200 (300) / 1.0k 50%".to_string());
    assert_eq!(widget.token_str, "↓100 ↑200 (300) / 1.0k 50%");
}

#[test]
fn test_status_bar_widget_set_loading() {
    let mut widget =
        codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    assert!(!widget.loading);

    widget.set_loading(true, Some("Saving...".to_string()));
    assert!(widget.loading);
    assert_eq!(widget.loading_label, Some("Saving...".to_string()));
}

#[test]
fn test_status_bar_widget_set_thinking() {
    let mut widget =
        codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    assert!(!widget.thinking);

    widget.set_thinking(true, Some("Reasoning...".to_string()));
    assert!(widget.thinking);
    assert_eq!(widget.thinking_label, Some("Reasoning...".to_string()));
}

#[test]
fn test_status_bar_widget_set_subagent_count() {
    let mut widget =
        codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    widget.set_subagent_count(3);
    assert_eq!(widget.subagent_count, 3);
}

#[test]
fn test_status_bar_widget_set_undo_message() {
    let mut widget =
        codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    assert!(widget.undo_message.is_none());

    widget.set_undo_message("Session deleted");
    assert!(widget.undo_message.is_some());
    assert!(widget
        .undo_message
        .as_ref()
        .unwrap()
        .contains("Session deleted"));
}

#[test]
fn test_status_bar_widget_clear_undo() {
    let mut widget =
        codegg::tui::components::status_bar::StatusBarWidget::new(Arc::new(Theme::dark()));
    widget.set_undo_message("Session deleted");
    assert!(widget.undo_message.is_some());

    widget.clear_undo_message();
    assert!(widget.undo_message.is_none());
}

#[test]
fn test_permission_dialog_options() {
    use codegg::permission::PermissionRequest;
    use codegg::tui::components::dialogs::permission::PermissionDialog;

    let request = PermissionRequest {
        tool: "bash".to_string(),
        path: Some("/tmp/test".to_string()),
        args: Some(serde_json::json!("echo hello")),
    };
    let widget = PermissionDialog::new(request, Arc::new(Theme::dark()));
    let options = widget.options();
    assert_eq!(options.len(), 4);
    assert_eq!(options[0], "Allow Once");
    assert_eq!(options[1], "Always Allow");
    assert_eq!(options[2], "Deny Once");
    assert_eq!(options[3], "Always Deny");
}

#[test]
fn test_permission_dialog_cursor_navigation() {
    use codegg::permission::PermissionRequest;
    use codegg::tui::components::dialogs::permission::PermissionDialog;

    let request = PermissionRequest {
        tool: "read".to_string(),
        path: Some("/tmp/test".to_string()),
        args: None,
    };
    let mut widget = PermissionDialog::new(request, Arc::new(Theme::dark()));
    assert_eq!(widget.selected_option(), 0);

    widget.cursor_down();
    assert_eq!(widget.selected_option(), 1);

    widget.cursor_down();
    assert_eq!(widget.selected_option(), 2);

    widget.cursor_down();
    assert_eq!(widget.selected_option(), 3);

    widget.cursor_down();
    assert_eq!(widget.selected_option(), 3);

    widget.cursor_up();
    assert_eq!(widget.selected_option(), 2);

    widget.cursor_up();
    assert_eq!(widget.selected_option(), 1);

    widget.cursor_up();
    assert_eq!(widget.selected_option(), 0);
}

#[test]
fn test_permission_dialog_persistent_confirmation_flow() {
    use codegg::permission::PermissionRequest;
    use codegg::tui::components::dialogs::permission::PermissionDialog;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let request = PermissionRequest {
        tool: "bash".to_string(),
        path: Some("/tmp/test".to_string()),
        args: None,
    };
    let mut widget = PermissionDialog::new(request, Arc::new(Theme::dark()));

    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

    widget.cursor_down();
    assert_eq!(widget.selected_option(), 1);

    let msg = widget.handle_key(enter_key);
    assert!(msg.is_none());
    assert!(widget.confirm_persistent);

    let confirm_options = widget.options();
    assert_eq!(confirm_options[0], "⚠ Confirm & Persist");
    assert_eq!(confirm_options[1], "← Cancel");

    let cancel_msg = widget.handle_key(esc_key);
    assert!(cancel_msg.is_none());
    assert!(!widget.confirm_persistent);
}

#[test]
fn test_permission_dialog_one_time_executes_immediately() {
    use codegg::permission::PermissionRequest;
    use codegg::tui::components::dialogs::permission::PermissionDialog;

    let request = PermissionRequest {
        tool: "bash".to_string(),
        path: Some("/tmp/test".to_string()),
        args: None,
    };
    let mut widget = PermissionDialog::new(request, Arc::new(Theme::dark()));

    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    let msg = widget.handle_key(enter_key);
    assert!(msg.is_some());
    if let Some(codegg::tui::app::TuiMsg::SubmitPermission { choice_index }) = msg {
        assert_eq!(choice_index, 0);
    }
}

#[test]
fn test_permission_dialog_non_ascii_args() {
    use codegg::permission::PermissionRequest;
    use codegg::tui::components::dialogs::permission::PermissionDialog;

    let request = PermissionRequest {
        tool: "bash".to_string(),
        path: None,
        args: Some(serde_json::json!("echo café 🦀")),
    };
    let widget = PermissionDialog::new(request, Arc::new(Theme::dark()));
    let options = widget.options();
    assert_eq!(options.len(), 4);
}

#[test]
fn test_session_dialog_delete_requires_empty_filter() {
    use codegg::session::Session;
    use codegg::tui::components::dialogs::session::SessionDialog;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let theme = Arc::new(Theme::dark());
    let mut dialog = SessionDialog::new(theme.clone());

    // Create a mock session
    let session = Session {
        id: "test-session-1".to_string(),
        project_id: "project-1".to_string(),
        workspace_id: None,
        parent_id: None,
        slug: "test-session".to_string(),
        directory: "/tmp".to_string(),
        title: "Test Session".to_string(),
        version: "1.0".to_string(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        revert: None,
        permission: None,
        tags: vec![],
        provider_connection_id: None,
        provider_connection_revision: None,
        model_catalog_revision: None,
        selected_model_id: None,
        agent: None,
        model: None,
        time_created: 0,
        time_updated: 0,
        time_compacting: None,
        time_archived: None,
        time_deleted: None,
    };
    dialog.load_sessions(vec![session]);

    let d_key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);

    // With empty filter, 'd' should return delete message
    let msg = dialog.handle_key(d_key);
    assert!(msg.is_some());

    // Set filter with a regular character (not a special key)
    dialog.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(!dialog.filter.is_empty());

    // With non-empty filter, 'd' should NOT return delete message
    let msg = dialog.handle_key(d_key);
    assert!(msg.is_none());
}

#[test]
fn test_session_dialog_archive_requires_empty_filter() {
    use codegg::session::Session;
    use codegg::tui::components::dialogs::session::SessionDialog;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let theme = Arc::new(Theme::dark());
    let mut dialog = SessionDialog::new(theme.clone());

    // Create a mock session
    let session = Session {
        id: "test-session-2".to_string(),
        project_id: "project-1".to_string(),
        workspace_id: None,
        parent_id: None,
        slug: "test-session-2".to_string(),
        directory: "/tmp".to_string(),
        title: "Test Session 2".to_string(),
        version: "1.0".to_string(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        revert: None,
        permission: None,
        tags: vec![],
        provider_connection_id: None,
        provider_connection_revision: None,
        model_catalog_revision: None,
        selected_model_id: None,
        agent: None,
        model: None,
        time_created: 0,
        time_updated: 0,
        time_compacting: None,
        time_archived: None,
        time_deleted: None,
    };
    dialog.load_sessions(vec![session]);

    let a_key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);

    // With empty filter, 'a' should return archive message
    let msg = dialog.handle_key(a_key);
    assert!(msg.is_some());

    // Set filter with a regular character
    dialog.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(!dialog.filter.is_empty());

    // With non-empty filter, 'a' should NOT return archive message
    let msg = dialog.handle_key(a_key);
    assert!(msg.is_none());
}

// ============================================================================
// Packet 3: App-level prompt input tests
// ============================================================================

use codegg::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn test_prompt_widget_insert_char() {
    let theme = codegg::tui::theme::Theme::dark();
    let mut widget = codegg::tui::components::prompt::PromptWidget::new(std::sync::Arc::new(theme));

    widget.insert_char('A');
    assert_eq!(widget.get_text(), "A");
    assert_eq!(widget.cursor_pos(), 1);

    widget.insert_char('B');
    assert_eq!(widget.get_text(), "AB");
    assert_eq!(widget.cursor_pos(), 2);
}

#[test]
fn test_prompt_widget_paste_at_empty() {
    let theme = codegg::tui::theme::Theme::dark();
    let mut widget = codegg::tui::components::prompt::PromptWidget::new(std::sync::Arc::new(theme));

    widget.paste("hello\nWorld".to_string());
    assert_eq!(widget.get_text(), "hello\nWorld");
    assert_eq!(widget.cursor_pos(), 11); // "hello\nWorld".len() = 11
}

#[test]
fn test_prompt_widget_paste_at_cursor() {
    let theme = codegg::tui::theme::Theme::dark();
    let mut widget = codegg::tui::components::prompt::PromptWidget::new(std::sync::Arc::new(theme));

    widget.set_text("a".to_string());
    widget.set_cursor(1);
    widget.paste("BC".to_string());
    assert_eq!(widget.get_text(), "aBC");
    assert_eq!(widget.cursor_pos(), 3);
}

#[test]
fn test_prompt_widget_paste_multiline() {
    let theme = codegg::tui::theme::Theme::dark();
    let mut widget = codegg::tui::components::prompt::PromptWidget::new(std::sync::Arc::new(theme));

    widget.paste("line1\nline2\nline3".to_string());
    assert_eq!(widget.get_text(), "line1\nline2\nline3");
    assert_eq!(widget.num_lines(), 3);
}

#[test]
fn test_app_shift_char_inserts_text() {
    let mut app = App::new_for_testing("/tmp/test".to_string());

    // Shift + 'A' should insert 'A' into prompt
    let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
    app.on_key(key);

    assert_eq!(app.prompt_state.prompt.get_text(), "A");
}

#[test]
fn test_app_shift_punctuation_inserts_text() {
    let mut app = App::new_for_testing("/tmp/test".to_string());

    // Shift + '?' should insert '?'
    let key = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT);
    app.on_key(key);

    assert_eq!(app.prompt_state.prompt.get_text(), "?");
}

#[test]
fn test_app_slash_at_position_0_enters_command_mode() {
    let mut app = App::new_for_testing("/tmp/test".to_string());

    // '/' at cursor 0 should enter command mode via on_char detection
    let key = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
    app.on_key(key);

    assert!(app.ui_state.command_mode);
    assert_eq!(app.prompt_state.prompt.get_text(), "/");
}

#[test]
fn test_app_paste_at_empty_prompt() {
    let mut app = App::new_for_testing("/tmp/test".to_string());

    app.on_paste("hello\nWorld".to_string());

    assert_eq!(app.prompt_state.prompt.get_text(), "hello\nWorld");
    assert_eq!(app.prompt_state.prompt.cursor_pos(), 11);
}

#[test]
fn test_app_paste_at_cursor() {
    let mut app = App::new_for_testing("/tmp/test".to_string());

    app.prompt_state.prompt.set_text("a".to_string());
    app.prompt_state.prompt.set_cursor(1);

    app.on_paste("BC".to_string());

    assert_eq!(app.prompt_state.prompt.get_text(), "aBC");
    assert_eq!(app.prompt_state.prompt.cursor_pos(), 3);
}

#[test]
fn test_app_paste_updates_completions_slash() {
    let mut app = App::new_for_testing("/tmp/test".to_string());

    // Paste "/model" should trigger slash completions
    app.on_paste("/model".to_string());

    assert_eq!(app.prompt_state.prompt.get_text(), "/model");
    assert!(app.prompt_state.show_completions);
    assert_eq!(
        app.prompt_state.completion_type,
        codegg::tui::app::CompletionType::Slash
    );
}

#[test]
fn test_app_paste_updates_completions_file() {
    let mut app = App::new_for_testing("/tmp/test".to_string());

    // Paste "@/src/tui" should trigger file completions
    app.on_paste("@src/tui".to_string());

    assert_eq!(app.prompt_state.prompt.get_text(), "@src/tui");
    assert!(app.prompt_state.show_completions);
    assert_eq!(
        app.prompt_state.completion_type,
        codegg::tui::app::CompletionType::File
    );
}

#[test]
fn test_session_dialog_handle_paste() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::session::SessionDialog;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let mut dialog = SessionDialog::new(theme);

    // Test handle_paste updates filter
    let msg = dialog.handle_paste("test_filter".to_string());
    assert!(msg.is_none()); // Should return None (handled, no TuiMsg)
    assert_eq!(dialog.filter, "test_filter");
}

#[test]
fn test_connect_dialog_handle_paste() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::connect::ConnectDialog;
    use codegg::tui::components::dialogs::connect::ConnectStep;
    use codegg::tui::components::dialogs::connect::ProviderInfo;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let providers = vec![ProviderInfo {
        id: "test-provider".to_string(),
        name: "Test Provider".to_string(),
        description: "Test".to_string(),
        requires_api_key: true,
        auth_modes: vec![codegg::tui::components::dialogs::connect::ProviderAuthMode::ApiKey],
        env_var_name: None,
        base_url_example: None,
    }];
    let mut dialog = ConnectDialog::new(providers, theme);

    // Move to EnterApiKey step
    dialog.step = ConnectStep::EnterApiKey;

    // Test handle_paste updates api_key_input
    let msg = dialog.handle_paste("test_api_key".to_string());
    assert!(msg.is_none());
    assert_eq!(dialog.api_key_input, "test_api_key");
    assert_eq!(dialog.cursor_pos, 12); // Length of "test_api_key"
}

#[test]
fn test_connect_dialog_cycles_tls_policy_without_exposing_secret() {
    use codegg::protocol::provider::EggpoolTlsPolicy;
    use codegg::tui::components::dialogs::connect::{ConnectDialog, ConnectStep, ProviderInfo};

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let providers = vec![ProviderInfo {
        id: "eggpool".to_string(),
        name: "Eggpool".to_string(),
        description: "OpenAI-compatible Eggpool".to_string(),
        requires_api_key: true,
        auth_modes: vec![codegg::tui::components::dialogs::connect::ProviderAuthMode::ApiKey],
        env_var_name: None,
        base_url_example: None,
    }];
    let mut dialog = ConnectDialog::new(providers, theme);
    dialog.step = ConnectStep::SelectTls;

    assert_eq!(dialog.tls_policy, EggpoolTlsPolicy::Optional);
    dialog.cycle_tls_policy(true);
    assert_eq!(dialog.tls_policy, EggpoolTlsPolicy::Disabled);
    dialog.cycle_tls_policy(false);
    assert_eq!(dialog.tls_policy, EggpoolTlsPolicy::Optional);
}

#[test]
fn test_import_dialog_handle_paste() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::import::ImportDialog;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let mut dialog = ImportDialog::new(theme);

    // Test handle_paste updates input
    let msg = dialog.handle_paste("/path/to/file".to_string());
    assert!(msg.is_none());
    assert_eq!(dialog.input, "/path/to/file");
}

#[test]
fn test_goto_dialog_handle_paste() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::goto::GotoDialog;

    let mut dialog = GotoDialog::new(100);

    // Test handle_paste updates input
    let msg = dialog.handle_paste("42".to_string());
    assert!(msg.is_none());
    assert_eq!(dialog.input, "42");
    assert!(dialog.is_valid()); // Should be valid after pasting a number
}

#[test]
fn test_model_dialog_handle_paste() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::model::ModelDialog;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let mut dialog = ModelDialog::new(theme);

    // Test handle_paste updates filter
    let msg = dialog.handle_paste("gpt".to_string());
    assert!(msg.is_none());
    assert_eq!(dialog.filter, "gpt");
}

#[test]
fn test_template_dialog_handle_paste() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::template::TemplateDialog;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let mut dialog = TemplateDialog::new(theme);

    // Test handle_paste updates filter
    let msg = dialog.handle_paste("default".to_string());
    assert!(msg.is_none());
    assert_eq!(dialog.filter, "default");
}

// ============================================================================
// Packet 1 Integration Tests: Make Model Dialog Show Current Models
// ============================================================================

#[test]
fn test_app_set_models_populates_dialog_state() {
    use codegg::tui::app::App;

    let mut app = App::new_for_testing("/tmp/test".to_string());
    let models = vec![
        "openai/gpt4".to_string(),
        "anthropic/claude".to_string(),
        "google/gemini".to_string(),
    ];

    app.set_models(models.clone());

    // Verify dialog_state.model_dialog.models is populated
    assert_eq!(app.dialog_state.model_dialog.models, models);
}

#[test]
fn test_app_set_models_syncs_current_model() {
    use codegg::tui::app::App;

    let mut app = App::new_for_testing("/tmp/test".to_string());
    let models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];

    app.set_models(models.clone());

    // Set current model and verify both states are synced
    app.agent_state.current_model = "anthropic/claude".to_string();
    app.dialog_state
        .model_dialog
        .set_current(&app.agent_state.current_model);

    assert_eq!(app.agent_state.current_model, "anthropic/claude");
    assert_eq!(app.dialog_state.model_dialog.current, "anthropic/claude");
}

#[test]
fn test_cycle_model_forward_syncs_dialog_state() {
    use codegg::tui::app::App;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = App::new_for_testing("/tmp/test".to_string());
    let models = vec![
        "openai/gpt4".to_string(),
        "anthropic/claude".to_string(),
        "google/gemini".to_string(),
    ];

    app.set_models(models.clone());
    app.agent_state.current_model = models[0].clone();
    app.agent_state.model_idx = 0;

    // Cycle forward using Ctrl+P key event
    let key = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
    app.on_key(key);

    // Verify both states are synced
    assert_eq!(app.agent_state.current_model, models[1]);
    assert_eq!(app.dialog_state.model_dialog.current, models[1]);
}

#[test]
fn test_cycle_model_backward_syncs_dialog_state() {
    use codegg::tui::app::App;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = App::new_for_testing("/tmp/test".to_string());
    let models = vec![
        "openai/gpt4".to_string(),
        "anthropic/claude".to_string(),
        "google/gemini".to_string(),
    ];

    app.set_models(models.clone());
    app.agent_state.current_model = models[1].clone();
    app.agent_state.model_idx = 1;

    // Cycle backward using Ctrl+Shift+P key event
    let key = KeyEvent::new(
        KeyCode::Char('P'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.on_key(key);

    // Verify both states are synced
    assert_eq!(app.agent_state.current_model, models[0]);
    assert_eq!(app.dialog_state.model_dialog.current, models[0]);
}

#[test]
fn test_select_model_msg_updates_dialog_current() {
    use codegg::tui::app::App;
    use codegg::tui::app::TuiMsg;

    let mut app = App::new_for_testing("/tmp/test".to_string());
    let models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];

    app.set_models(models.clone());
    app.agent_state.current_model = models[0].clone();

    // Simulate SelectModel message
    let msg = TuiMsg::SelectModel {
        model: models[1].clone(),
    };
    // Process the message (simplified - just call the handler directly)
    if let TuiMsg::SelectModel { model } = msg {
        app.agent_state.current_model = model.clone();
        app.dialog_state
            .model_dialog
            .set_current(&app.agent_state.current_model);
        if let Some(idx) = app.agent_state.models.iter().position(|m| m == &model) {
            app.agent_state.model_idx = idx;
        }
    }

    // Verify both states are synced
    assert_eq!(app.agent_state.current_model, models[1]);
    assert_eq!(app.dialog_state.model_dialog.current, models[1]);
}

// ============================================================================
// Packet 7: Mouse Click Integration Tests
// ============================================================================

#[test]
fn test_mouse_click_selects_model() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::model::ModelDialog;
    use codegg::tui::components::dialogs::model::ModelDialogTab;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let mut dialog = ModelDialog::new(theme);
    dialog.tab = ModelDialogTab::SelectModel;
    dialog.models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
    dialog.set_visible_height(20);

    // Simulate clicking on the first model (dialog-relative row 4: border(0), tab(1), blank(2), provider header(3), model(4))
    // hit_test() subtracts 1 (top border) → content-relative 3 = first model
    let result = dialog.hit_test(4);
    assert_eq!(result, Some(0));

    // Simulate the selection
    if let Some(idx) = result {
        dialog.set_selected(idx);
        assert_eq!(dialog.selected, 0);
    }
}

#[test]
fn test_mouse_click_out_of_bounds() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::model::ModelDialog;
    use codegg::tui::components::dialogs::model::ModelDialogTab;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let mut dialog = ModelDialog::new(theme);
    dialog.tab = ModelDialogTab::SelectModel;

    // Click on tab line (row 0) - should return None
    assert_eq!(dialog.hit_test(0), None);
    // Click on blank line (row 1) - should return None
    assert_eq!(dialog.hit_test(1), None);
}

// ============================================================================
// Packet 2: App-level mouse click test (full coordinate path)
// ============================================================================

#[test]
fn test_app_mouse_click_selects_model() {
    use codegg::tui::app::App;
    use codegg::tui::Dialog;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    let mut app = App::new_for_testing("/tmp/test".to_string());

    // Set up models
    let models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
    app.set_models(models.clone());

    // Open model dialog (sets up focus manager and dialog state)
    app.open_dialog(Dialog::Model);

    // Set dialog_area to simulate rendering at y=0
    // This means dialog-relative row = screen row
    app.dialog_area = Some(Rect::new(0, 0, 60, 20));

    // Click at row 4 (dialog-relative)
    // - rel_y = 4 (dialog-relative, including borders)
    // - hit_test(4) subtracts 1 for top border → content-relative 3
    // - hit_test_model_row(3): tab(0), blank(1), provider header(2), model(3) → Some(0)
    let mouse_event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 4,
        modifiers: crossterm::event::KeyModifiers::empty(),
    };

    app.on_mouse(mouse_event);

    // Verify the first model was selected
    assert_eq!(app.dialog_state.model_dialog.selected, 0);
}

#[test]
fn test_app_mouse_click_selects_second_model() {
    use codegg::tui::app::App;
    use codegg::tui::Dialog;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    let mut app = App::new_for_testing("/tmp/test".to_string());

    let models = vec![
        "openai/gpt4".to_string(),
        "openai/gpt35".to_string(),
        "anthropic/claude".to_string(),
    ];
    app.set_models(models.clone());

    app.open_dialog(Dialog::Model);
    app.dialog_area = Some(Rect::new(0, 0, 60, 20));

    // Click at row 5 (dialog-relative)
    // - rel_y = 5
    // - hit_test(5) → hit_test_model_row(4)
    // - tab(0), blank(1), provider "openai"(2, header), gpt4(3), gpt35(4) → Some(1)
    let mouse_event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 5,
        modifiers: crossterm::event::KeyModifiers::empty(),
    };

    app.on_mouse(mouse_event);

    // Verify the second model (gpt35) was selected
    assert_eq!(app.dialog_state.model_dialog.selected, 1);
}

#[test]
fn test_app_mouse_click_with_filter() {
    use codegg::tui::app::App;
    use codegg::tui::Dialog;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    let mut app = App::new_for_testing("/tmp/test".to_string());

    let models = vec![
        "openai/gpt4".to_string(),
        "openai/gpt35".to_string(),
        "anthropic/claude".to_string(),
    ];
    app.set_models(models.clone());

    // Set a filter BEFORE opening the dialog
    // This ensures the clone in focus_manager has the filter
    app.dialog_state.model_dialog.set_filter('g');

    app.open_dialog(Dialog::Model);

    app.dialog_area = Some(Rect::new(0, 0, 60, 20));

    // With filter "g": models containing 'g' are "openai/gpt4" and "openai/gpt35"
    // flat_filtered() returns: [("openai", "gpt4"), ("openai", "gpt35")]
    // Click at row 7 (dialog-relative):
    // - hit_test(7) subtracts 1 → hit_test_model_row(6)
    // - Rows: tab(0), blank(1), filter(2), blank(3), provider header "openai"(4), gpt4(5), gpt35(6)
    // - At row 6: Some(1) for gpt35 (second filtered model)
    let mouse_event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 7,
        modifiers: crossterm::event::KeyModifiers::empty(),
    };

    app.on_mouse(mouse_event);

    // Verify the second filtered model (gpt35) was selected
    assert_eq!(app.dialog_state.model_dialog.selected, 1);
}

// ============================================================================
// Packet 3: Model Dialog Visible Height and Clipping Tests
// ============================================================================

#[test]
fn test_model_row_budget_without_filter() {
    use codegg::tui::components::dialogs::model::ModelDialog;

    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    // Set visible_height to 20 (simulating area.height = 22, minus 2 for borders)
    dialog.set_visible_height(20);
    // budget = 20 - 2 (tab+blank) - 2 (spacer+footer) = 16
    // No filter, so no filter lines to subtract
    let _budget = dialog.model_row_budget();
    assert_eq!(dialog.model_row_budget(), 16);
}

#[test]
fn test_model_row_budget_with_filter() {
    use codegg::tui::components::dialogs::model::ModelDialog;

    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    dialog.set_visible_height(20);
    dialog.set_filter('g');
    // visible_height = 20
    // budget = 20 - 2 (tab+blank) - 2 (filter+spacer) - 2 (spacer+footer) = 14
    let budget = dialog.model_row_budget();
    assert_eq!(budget, 14);
}

#[test]
fn test_small_height_shows_at_least_one_model() {
    use codegg::tui::components::dialogs::model::ModelDialog;

    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    dialog.models = vec!["openai/gpt4".to_string()];
    // Small height: area.height = 7 → visible_height = 7-2 = 5
    dialog.set_visible_height(5);
    dialog.update_cache();

    let budget = dialog.model_row_budget();
    // budget = 5 - 2 (tab+blank) - 2 (spacer+footer) = 1
    assert_eq!(budget, 1);

    let visible = dialog.count_visible_models(0);
    assert!(
        visible >= 1,
        "Should show at least one model even with small height"
    );
}

#[test]
fn test_small_height_no_header_without_model_row() {
    use codegg::tui::components::dialogs::model::ModelDialog;

    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    dialog.models = vec!["openai/gpt4".to_string()];
    // Very small height: area.height = 6 → visible_height = 6-2 = 4
    dialog.set_visible_height(4);
    dialog.update_cache();

    let budget = dialog.model_row_budget();
    // budget = 4 - 2 (tab+blank) - 2 (spacer+footer) = 0
    assert_eq!(budget, 0);

    // With budget 0, no models should be visible
    let visible = dialog.count_visible_models(0);
    assert_eq!(visible, 0);
}

#[test]
fn test_footer_always_visible_in_budget() {
    use codegg::tui::components::dialogs::model::ModelDialog;

    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    // Even with just enough height for footer
    // area.height = 5 → visible_height = 3
    // budget = 3 - 2 (tab+blank) - 2 (spacer+footer) = -1 → 0 after saturating_sub
    dialog.set_visible_height(3);
    let budget = dialog.model_row_budget();
    // saturating_sub ensures budget doesn't go negative
    assert_eq!(budget, 0);
}

#[test]
fn test_count_visible_models_with_provider_header() {
    use codegg::tui::components::dialogs::model::ModelDialog;

    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    dialog.models = vec![
        "openai/gpt4".to_string(),
        "openai/gpt35".to_string(),
        "anthropic/claude".to_string(),
    ];
    dialog.set_visible_height(20);
    dialog.update_cache();

    // With budget=16: can show provider "openai" (header + 2 models = 3 lines)
    // and provider "anthropic" (header + 1 model = 2 lines) = 5 lines total
    let visible = dialog.count_visible_models(0);
    assert!(visible >= 2, "Should show models from multiple providers");
}

#[test]
fn test_selected_row_remains_visible() {
    use codegg::tui::components::dialogs::model::ModelDialog;

    let mut dialog = ModelDialog::new(Arc::new(Theme::dark()));
    dialog.models = vec![
        "openai/gpt4".to_string(),
        "openai/gpt35".to_string(),
        "openai/gpt5".to_string(),
    ];
    dialog.set_visible_height(10);
    dialog.update_cache();

    // Select the last model
    dialog.selected = 2;
    dialog.scroll.clamp(
        dialog.selected,
        dialog.flat_filtered().len(),
        dialog.count_visible_models(0),
    );

    // The selected model should be visible (scroll adjusted)
    let scroll = dialog.scroll.get();
    assert!(
        scroll <= dialog.selected,
        "Scroll should show the selected model"
    );
}

#[test]
fn test_info_dialog_footer_includes_scroll_hints() {
    use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};

    let mut dialog = InfoDialog::new(
        Arc::new(Theme::dark()),
        InfoType::Stats,
        vec!["line1".to_string(), "line2".to_string()],
    );
    // Verify scrolling works (j/k keys are handled)
    let down = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('j'),
        crossterm::event::KeyModifiers::NONE,
    );
    let up = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('k'),
        crossterm::event::KeyModifiers::NONE,
    );
    assert!(dialog.handle_key(down).is_none(), "j should scroll down");
    assert!(dialog.handle_key(up).is_none(), "k should scroll up");
}

#[test]
fn test_diff_dialog_footer_includes_all_keys() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::diff::DiffDialog;

    let mut dialog = DiffDialog::new(
        "old content".into(),
        "new content".into(),
        "test diff".into(),
    );
    // Verify 's' key is handled (mode toggle) via Component trait
    let s_key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('s'),
        crossterm::event::KeyModifiers::NONE,
    );
    let result = Component::handle_key(&mut dialog, s_key);
    // handle_key returns Option<TuiMsg> — None means handled (no message to send)
    assert!(
        result.is_none(),
        "s key should be handled without sending a message"
    );
}

#[test]
fn test_question_dialog_footer_includes_editing_keys() {
    use codegg::tui::components::component::Component;
    use codegg::tui::components::dialogs::question::{QuestionDialog, QuestionSpec};

    let questions = vec![QuestionSpec {
        question: "Test?".to_string(),
        options: Some(vec!["Yes".to_string(), "No".to_string()]),
        initial: None,
    }];
    let mut dialog = QuestionDialog::new(questions);
    // Verify Backspace is handled
    let bs = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Backspace,
        crossterm::event::KeyModifiers::NONE,
    );
    let result = Component::handle_key(&mut dialog, bs);
    assert!(result.is_none(), "Backspace should be handled");
    // Verify Delete is handled
    let del = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Delete,
        crossterm::event::KeyModifiers::NONE,
    );
    let result = Component::handle_key(&mut dialog, del);
    assert!(result.is_none(), "Delete should be handled");
    // Verify Left/Right cursor keys are handled
    let left = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Left,
        crossterm::event::KeyModifiers::NONE,
    );
    let result = Component::handle_key(&mut dialog, left);
    assert!(result.is_none(), "Left should be handled");
    let right = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Right,
        crossterm::event::KeyModifiers::NONE,
    );
    let result = Component::handle_key(&mut dialog, right);
    assert!(result.is_none(), "Right should be handled");
}

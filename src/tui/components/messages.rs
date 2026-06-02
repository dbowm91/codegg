use crate::session::message::ToolStatus;
use once_cell::sync::Lazy;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

mod layout;
use layout::MessageLayoutCache;

use super::super::theme::Theme;
use super::tool_output::ToolCallEntry;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);
static URL_REGEX: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"https?://[^\s<>"'`]+"#).expect("invalid URL regex"));
static FILE_PATH_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"(?:^|[\s])(\/(?:[a-zA-Z0-9._~-]+\/)*[a-zA-Z0-9._~-]+|~\/[a-zA-Z0-9._~-]+(?:\/[a-zA-Z0-9._~-]+)*|\.\.?\/[a-zA-Z0-9._~-]+(?:\/[a-zA-Z0-9._~-]+)*)"#).expect("invalid file path regex")
});

fn wrap_osc8(url: &str, text: &str) -> String {
    format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text)
}

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub msg_idx: usize,
    pub part_idx: usize,
    pub line_in_msg: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub enum MsgPart {
    Text {
        content: String,
    },
    Reasoning {
        content: String,
        collapsed: bool,
    },
    ToolCall {
        id: String,
        name: String,
        input: String,
        output: String,
        status: ToolStatus,
        duration_ms: Option<u64>,
        exit_code: Option<i32>,
        output_lines: Option<usize>,
    },
    Image {
        data_uri: String,
        alt_text: String,
        width: u32,
        height: u32,
    },
}

#[derive(Debug, Clone)]
pub struct UIMessage {
    pub role: MessageRole,
    pub parts: Vec<MsgPart>,
    pub timestamp: Option<i64>,
    pub is_plan_mode: Option<bool>,
}

impl UIMessage {
    pub fn is_thinking_first(&self) -> bool {
        self.parts.first().map(|p| matches!(p, MsgPart::Reasoning { .. })).unwrap_or(false)
    }
}

impl UIMessage {
    pub fn text_content(&self) -> String {
        let mut text = String::new();
        for part in &self.parts {
            match part {
                MsgPart::Text { content } => {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(content);
                }
                MsgPart::Reasoning { content, .. } => {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(content);
                }
                MsgPart::ToolCall {
                    name,
                    input,
                    output,
                    ..
                } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!(
                        "Tool call: {} input: {} output: {}",
                        name, input, output
                    ));
                }
                MsgPart::Image { alt_text, .. } => {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&format!("[Image: {}]", alt_text));
                }
            }
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
}

pub struct MessagesWidget {
    pub messages: Vec<UIMessage>,
    pub scroll: usize,
    pub auto_scroll: bool,
    pub theme: Arc<Theme>,
    pub show_thinking: bool,
    pub show_timestamps: bool,
    tool_calls: HashMap<String, ToolCallEntry>,
    pub sel_msg: Option<usize>,
    pub undo_stack: VecDeque<UIMessage>,
    pub streaming_tokens: String,
    pub assistant_is_thinking: bool,
    pub search_query: Option<String>,
    pub search_matches: Vec<SearchMatch>,
    pub search_current: usize,
    pub search_visible: bool,
    pub sel_tool_call: Option<usize>,
    pub tool_call_expanded: HashMap<String, bool>,
    pub visible_height: usize,
    flattened_tool_calls_cache: RefCell<Option<Vec<(usize, usize, String)>>>,
    message_layout_cache: RefCell<Option<MessageLayoutCache>>,
}

fn extract_tool_target(name: &str, input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(input);
    let val = match parsed {
        Ok(v) => v,
        Err(_) => return input.lines().next().unwrap_or("").to_string(),
    };
    match name {
        "read" | "write" | "edit" | "multiedit" | "glob" | "grep" => {
            val.get("path")
                .or_else(|| val.get("file_path"))
                .or_else(|| val.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "bash" | "exec" => {
            val.get("command")
                .or_else(|| val.get("cmd"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "task" => {
            val.get("prompt")
                .or_else(|| val.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "webfetch" => {
            val.get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        _ => {
            val.as_object()
                .and_then(|m| m.values().next())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
    }
    .chars()
    .take(60)
    .collect::<String>()
}

fn find_any_tag(text: &str, start: bool) -> Option<(usize, usize)> {
    let tags = if start {
        vec!["<think>", "<thought>", "<thinking>"]
    } else {
        vec!["</think>", "</thought>", "</thinking>"]
    };

    let mut best_match: Option<(usize, usize)> = None;

    for tag in tags {
        if let Some(pos) = text.to_lowercase().find(tag) {
            if best_match.is_none() || pos < best_match.unwrap().0 {
                best_match = Some((pos, tag.len()));
            }
        }
    }

    best_match
}

impl MessagesWidget {
    pub fn estimate_msg_lines(&self, msg: &UIMessage) -> usize {
        let mut lines = 1;
        if self.show_timestamps && msg.timestamp.is_some() {
            lines += 1;
        }
        for part in &msg.parts {
            match part {
                MsgPart::Text { content } => {
                    let mut text_lines = content.lines().count().max(1);
                    // Account for ┌─ LANG header lines added by render_markdown
                    // for code blocks with a language specifier.
                    let mut in_code = false;
                    let mut code_lang = String::new();
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with("```") {
                            if in_code {
                                if !code_lang.is_empty() {
                                    text_lines += 1; // ┌─ header line
                                }
                                code_lang.clear();
                                in_code = false;
                            } else {
                                code_lang = trimmed
                                    .trim_start_matches("```")
                                    .trim()
                                    .to_string();
                                in_code = true;
                            }
                        }
                    }
                    lines += text_lines;
                }
                MsgPart::Reasoning { content, collapsed } => {
                    lines += 1;
                    if self.show_thinking && !*collapsed {
                        lines += content.lines().count();
                    }
                }
                MsgPart::ToolCall { .. } => {
                    lines += 1;
                }
                MsgPart::Image { .. } => {
                    lines += 1;
                }
            }
        }
        // Account for streaming tokens rendered as 2 extra lines
        // (thinking indicator + token text) for the last assistant message.
        if msg.role == MessageRole::Assistant
            && !self.streaming_tokens.is_empty()
            && self.messages.last().is_some_and(|m| std::ptr::eq(m, msg))
        {
            lines += 2;
        }
        lines
    }

    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            messages: Vec::new(),
            scroll: 0,
            auto_scroll: true,
            theme,
            show_thinking: true,
            show_timestamps: false,
            tool_calls: HashMap::new(),
            sel_msg: None,
            undo_stack: VecDeque::new(),
            streaming_tokens: String::new(),
            assistant_is_thinking: false,
            search_query: None,
            search_matches: Vec::new(),
            search_current: 0,
            search_visible: false,
            sel_tool_call: None,
            tool_call_expanded: HashMap::new(),
            visible_height: 20,
            flattened_tool_calls_cache: RefCell::new(None),
            message_layout_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_auto_scroll(&mut self, val: bool) {
        self.auto_scroll = val;
    }

    pub fn get_message(&self, idx: usize) -> Option<&UIMessage> {
        self.messages.get(idx)
    }

    pub fn add_user_message(&mut self, text: String, is_plan_mode: Option<bool>) {
        let was_at_bottom = self.is_at_bottom();
        self.messages.push(UIMessage {
            role: MessageRole::User,
            parts: vec![MsgPart::Text { content: text }],
            timestamp: Some(chrono::Local::now().timestamp()),
            is_plan_mode,
        });
        self.flattened_tool_calls_cache.borrow_mut().take();
        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn add_assistant_text(&mut self, text: String) {
        let was_at_bottom = self.is_at_bottom();
        let mut target_msg = None;
        if let Some(last) = self.messages.last_mut() {
            if last.role == MessageRole::Assistant {
                target_msg = Some(last);
            }
        }

        if target_msg.is_none() {
            self.messages.push(UIMessage {
                role: MessageRole::Assistant,
                parts: vec![],
                timestamp: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                ),
                is_plan_mode: None,
            });
            self.assistant_is_thinking = false; // Reset for new message
            target_msg = self.messages.last_mut();
        }

        let msg = target_msg.unwrap();

        // Use a state machine to parse thinking tags in the streaming delta
        let mut current_pos = 0;

        while current_pos < text.len() {
            if self.assistant_is_thinking {
                // Look for the end of the thinking block
                let remaining = &text[current_pos..];
                if let Some(end_tag_pos) = find_any_tag(remaining, false) {
                    let thinking_chunk = &remaining[..end_tag_pos.0];
                    if !thinking_chunk.is_empty() {
                        if let Some(MsgPart::Reasoning { content, .. }) = msg.parts.last_mut() {
                            content.push_str(thinking_chunk);
                        } else {
                            msg.parts.push(MsgPart::Reasoning {
                                content: thinking_chunk.to_string(),
                                collapsed: false,
                            });
                        }
                    }
                    self.assistant_is_thinking = false;
                    current_pos += end_tag_pos.0 + end_tag_pos.1; // Skip end tag
                } else {
                    // Still thinking, append all remaining text
                    let thinking_chunk = remaining;
                    if let Some(MsgPart::Reasoning { content, .. }) = msg.parts.last_mut() {
                        content.push_str(thinking_chunk);
                    } else {
                        msg.parts.push(MsgPart::Reasoning {
                            content: thinking_chunk.to_string(),
                            collapsed: false,
                        });
                    }
                    break;
                }
            } else {
                // Look for the start of a thinking block
                let remaining = &text[current_pos..];
                if let Some(start_tag_pos) = find_any_tag(remaining, true) {
                    let text_chunk = &remaining[..start_tag_pos.0];
                    if !text_chunk.is_empty() {
                        if let Some(MsgPart::Text { content }) = msg.parts.last_mut() {
                            content.push_str(text_chunk);
                        } else {
                            msg.parts.push(MsgPart::Text { content: text_chunk.to_string() });
                        }
                    }
                    self.assistant_is_thinking = true;
                    current_pos += start_tag_pos.0 + start_tag_pos.1; // Skip start tag
                } else {
                    // Just normal text, append all remaining text
                    let text_chunk = remaining;
                    if let Some(MsgPart::Text { content }) = msg.parts.last_mut() {
                        content.push_str(text_chunk);
                    } else {
                        msg.parts.push(MsgPart::Text { content: text_chunk.to_string() });
                    }
                    break;
                }
            }
        }

        self.invalidate_layout_cache();
        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn add_reasoning(&mut self, reasoning: String) {
        let was_at_bottom = self.is_at_bottom();
        if let Some(last) = self.messages.last_mut() {
            if last.role == MessageRole::Assistant {
                if let Some(MsgPart::Reasoning { content, .. }) = last.parts.last_mut() {
                    content.push_str(&reasoning);
                } else {
                    last.parts.push(MsgPart::Reasoning {
                        content: reasoning,
                        collapsed: false,
                    });
                }
                self.invalidate_layout_cache();
                if self.auto_scroll && was_at_bottom {
                    self.scroll = usize::MAX;
                }
                return;
            }
        }
        self.messages.push(UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::Reasoning {
                content: reasoning,
                collapsed: false,
            }],
            timestamp: Some(chrono::Local::now().timestamp()),
            is_plan_mode: None,
        });
        self.invalidate_layout_cache();
        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn add_tool_call(&mut self, id: String, name: String, input: serde_json::Value) {
        let was_at_bottom = self.is_at_bottom();
        let input_str = serde_json::to_string_pretty(&input).unwrap_or_default();
        self.messages.push(UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::ToolCall {
                id: id.clone(),
                name,
                input: input_str,
                output: String::new(),
                status: ToolStatus::Pending,
                duration_ms: None,
                exit_code: None,
                output_lines: None,
            }],
            timestamp: Some(chrono::Local::now().timestamp()),
            is_plan_mode: None,
        });
        self.tool_calls.insert(
            id,
            ToolCallEntry {
                name: String::new(),
                input: String::new(),
                output: String::new(),
                status: ToolStatus::Pending,
                duration_ms: None,
                exit_code: None,
                output_lines: None,
                risk: crate::session::events::ToolRisk::Unknown,
                summary: None,
                cwd: None,
            },
        );
        self.flattened_tool_calls_cache.borrow_mut().take();
        self.invalidate_layout_cache();
        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn update_tool_call(
        &mut self,
        id: &str,
        output: String,
        status: ToolStatus,
        duration_ms: Option<u64>,
        exit_code: Option<i32>,
        output_lines: Option<usize>,
    ) {
        for msg in &mut self.messages {
            for part in &mut msg.parts {
                if let MsgPart::ToolCall {
                    id: part_id,
                    output: part_output,
                    status: part_status,
                    duration_ms: part_duration,
                    exit_code: part_exit_code,
                    output_lines: part_output_lines,
                    ..
                } = part
                {
                    if part_id == id {
                        *part_output = output.clone();
                        *part_status = status.clone();
                        *part_duration = duration_ms;
                        *part_exit_code = exit_code;
                        *part_output_lines = output_lines;
                    }
                }
            }
        }
        if let Some(entry) = self.tool_calls.get_mut(id) {
            entry.output = output;
            entry.status = status;
            entry.duration_ms = duration_ms;
            entry.exit_code = exit_code;
            entry.output_lines = output_lines;
        }
        self.flattened_tool_calls_cache.borrow_mut().take();
        self.invalidate_layout_cache();
        if self.auto_scroll {
            self.scroll = usize::MAX;
        }
    }

    pub fn toggle_reasoning(&mut self, msg_idx: usize) {
        if let Some(msg) = self.messages.get_mut(msg_idx) {
            for part in &mut msg.parts {
                if let MsgPart::Reasoning { collapsed, .. } = part {
                    *collapsed = !*collapsed;
                }
            }
        }
        self.invalidate_layout_cache();
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.tool_calls.clear();
        self.scroll = 0;
        self.sel_msg = None;
        self.undo_stack.clear();
        self.sel_tool_call = None;
        self.tool_call_expanded.clear();
        self.flattened_tool_calls_cache.borrow_mut().take();
        self.invalidate_layout_cache();
    }

    pub fn undo(&mut self) -> bool {
        if self.messages.is_empty() {
            return false;
        }
        if let Some(removed) = self.messages.pop() {
            self.undo_stack.push_front(removed);
        }
        if !self.messages.is_empty() {
            if let Some(last) = self.messages.last() {
                if last.role == MessageRole::Assistant {
                    if let Some(removed) = self.messages.pop() {
                        self.undo_stack.push_front(removed);
                    }
                }
            }
        }
        self.flattened_tool_calls_cache.borrow_mut().take();
        self.invalidate_layout_cache();
        self.scroll = 0;
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.undo_stack.is_empty() {
            return false;
        }
        let restored = self.undo_stack.pop_front();
        if let Some(msg) = restored {
            if msg.role == MessageRole::Assistant && !self.undo_stack.is_empty() {
                if let Some(user_msg) = self.undo_stack.pop_front() {
                    self.messages.push(user_msg);
                }
            }
            self.messages.push(msg);
        }
        self.flattened_tool_calls_cache.borrow_mut().take();
        self.invalidate_layout_cache();
        self.scroll = usize::MAX;
        true
    }

    fn flatten_tool_calls(&self) -> Vec<(usize, usize, String)> {
        if let Some(ref cache) = *self.flattened_tool_calls_cache.borrow() {
            return cache.clone();
        }
        let mut tool_calls = Vec::new();
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            for (part_idx, part) in msg.parts.iter().enumerate() {
                if let MsgPart::ToolCall { id, .. } = part {
                    tool_calls.push((msg_idx, part_idx, id.clone()));
                }
            }
        }
        *self.flattened_tool_calls_cache.borrow_mut() = Some(tool_calls.clone());
        tool_calls
    }

    fn get_layout_cache(&self) -> MessageLayoutCache {
        if let Some(ref cache) = *self.message_layout_cache.borrow() {
            return cache.clone();
        }
        let cache = self.build_layout_cache();
        *self.message_layout_cache.borrow_mut() = Some(cache.clone());
        cache
    }

    fn invalidate_layout_cache(&self) {
        *self.message_layout_cache.borrow_mut() = None;
    }

    fn build_layout_cache(&self) -> MessageLayoutCache {
        let mut offsets = Vec::with_capacity(self.messages.len());
        let mut total = 0usize;
        for (idx, msg) in self.messages.iter().enumerate() {
            let count = self.estimate_msg_lines(msg);
            offsets.push((idx, total, count));
            total += count;
        }
        MessageLayoutCache::new(offsets, total)
    }

    pub fn select_next_tool_call(&mut self) {
        let tool_calls = self.flatten_tool_calls();
        if tool_calls.is_empty() {
            return;
        }
        match self.sel_tool_call {
            Some(idx) if idx + 1 < tool_calls.len() => {
                self.sel_tool_call = Some(idx + 1);
            }
            None if !tool_calls.is_empty() => {
                self.sel_tool_call = Some(0);
            }
            _ => {}
        }
    }

    pub fn select_prev_tool_call(&mut self) {
        let tool_calls = self.flatten_tool_calls();
        if tool_calls.is_empty() {
            return;
        }
        match self.sel_tool_call {
            Some(idx) if idx > 0 => {
                self.sel_tool_call = Some(idx - 1);
            }
            Some(_) => {
                self.sel_tool_call = Some(tool_calls.len() - 1);
            }
            None if !tool_calls.is_empty() => {
                self.sel_tool_call = Some(tool_calls.len() - 1);
            }
            _ => {}
        }
    }

    pub fn toggle_selected_tool_call_expanded(&mut self) {
        let tool_calls = self.flatten_tool_calls();
        if let Some(idx) = self.sel_tool_call {
            if let Some((_, _, id)) = tool_calls.get(idx) {
                let expanded = self
                    .tool_call_expanded
                    .entry(id.to_string())
                    .or_insert(false);
                *expanded = !*expanded;
            }
        }
    }

    pub fn get_selected_tool_call_output(&self) -> Option<String> {
        let tool_calls = self.flatten_tool_calls();
        if let Some(idx) = self.sel_tool_call {
            if let Some((msg_idx, part_idx, _)) = tool_calls.get(idx) {
                if let Some(msg) = self.messages.get(*msg_idx) {
                    if let Some(MsgPart::ToolCall { output, .. }) = msg.parts.get(*part_idx) {
                        return Some(output.clone());
                    }
                }
            }
        }
        None
    }

    pub fn get_selected_content(&self) -> String {
        if let Some(idx) = self.sel_msg {
            if let Some(msg) = self.messages.get(idx) {
                let mut content = String::new();
                for part in &msg.parts {
                    match part {
                        MsgPart::Text { content: c } => {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str(c);
                        }
                        MsgPart::Reasoning { content: c, .. } => {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str(c);
                        }
                        MsgPart::ToolCall { name, output, .. } => {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str(&format!("[{name}] {output}"));
                        }
                        MsgPart::Image { alt_text, .. } => {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str(&format!("[Image: {}]", alt_text));
                        }
                    }
                }
                return content;
            }
        }
        String::new()
    }

    pub fn select_next(&mut self) {
        if self.messages.is_empty() {
            return;
        }
        let new_sel = match self.sel_msg {
            Some(idx) if idx + 1 < self.messages.len() => idx + 1,
            None => 0,
            _ => return,
        };
        self.sel_msg = Some(new_sel);
    }

    pub fn select_prev(&mut self) {
        if self.messages.is_empty() {
            return;
        }
        let new_sel = match self.sel_msg {
            Some(idx) if idx > 0 => idx - 1,
            None => self.messages.len().saturating_sub(1),
            _ => return,
        };
        self.sel_msg = Some(new_sel);
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn select_index(&mut self, idx: usize) {
        if idx < self.messages.len() {
            self.sel_msg = Some(idx);
            if self.auto_scroll && self.is_at_bottom() {
                self.scroll = usize::MAX;
            }
        }
    }

    fn total_rendered_lines(&self) -> usize {
        if self.messages.is_empty() {
            return 0;
        }
        let mut total = 0;
        for msg in &self.messages {
            total += self.estimate_msg_lines(msg);
        }
        total
    }

    fn is_at_bottom(&self) -> bool {
        if self.scroll == usize::MAX {
            return true;
        }
        let total = self.total_rendered_lines();
        if total == 0 {
            return true;
        }
        let max_scroll = total.saturating_sub(self.visible_height);
        self.scroll >= max_scroll
    }

    fn normalize_scroll(&mut self) {
        if self.scroll == usize::MAX {
            let total = self.total_rendered_lines();
            let max_scroll = total.saturating_sub(self.visible_height);
            self.scroll = max_scroll;
        }
    }

    pub fn scroll_up(&mut self) {
        self.normalize_scroll();
        if self.scroll > 0 {
            self.scroll -= 1;
        }
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self) {
        self.normalize_scroll();
        let total_lines = self.total_rendered_lines();
        let available = self.visible_height;
        let max_scroll = total_lines.saturating_sub(available);
        if self.scroll < max_scroll {
            self.scroll += 1;
        }
        self.auto_scroll = self.scroll >= max_scroll;
    }

    pub fn scroll_page_up(&mut self) {
        self.normalize_scroll();
        let total_lines = self.total_rendered_lines();
        let max_scroll = total_lines.saturating_sub(self.visible_height);
        let page = self.visible_height.saturating_sub(2).max(1);
        self.scroll = self.scroll.saturating_sub(page).min(max_scroll);
        self.auto_scroll = false;
    }

    pub fn scroll_page_down(&mut self) {
        self.normalize_scroll();
        let total_lines = self.total_rendered_lines();
        let max_scroll = total_lines.saturating_sub(self.visible_height);
        let page = self.visible_height.saturating_sub(2).max(1);
        self.scroll = (self.scroll + page).min(max_scroll);
        self.auto_scroll = self.scroll >= max_scroll;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
        self.auto_scroll = false;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = usize::MAX;
        self.auto_scroll = true;
    }

    pub fn scroll_left(&mut self) {}

    pub fn scroll_right(&mut self) {}

    pub fn add_streaming_token(&mut self, token: &str) {
        const MAX_STREAMING_TOKENS_SIZE: usize = 1024 * 1024;
        let was_at_bottom = self.is_at_bottom();

        // Ensure streaming text is visible even before first finalized line.
        let needs_placeholder = self
            .messages
            .last()
            .map(|m| m.role != MessageRole::Assistant)
            .unwrap_or(true);
        if needs_placeholder {
            self.messages.push(UIMessage {
                role: MessageRole::Assistant,
                parts: vec![],
                timestamp: Some(chrono::Local::now().timestamp()),
                is_plan_mode: None,
            });
            self.invalidate_layout_cache();
        }

        if self.streaming_tokens.len() + token.len() > MAX_STREAMING_TOKENS_SIZE {
            self.streaming_tokens
                .truncate(MAX_STREAMING_TOKENS_SIZE / 2);
        }
        self.streaming_tokens.push_str(token);

        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn finalize_streaming(&mut self) {
        if !self.streaming_tokens.is_empty() {
            self.add_assistant_text(self.streaming_tokens.clone());
            self.streaming_tokens.clear();
        }
    }

    pub fn clear_streaming(&mut self) {
        self.streaming_tokens.clear();
    }

    pub fn search(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_search();
            return;
        }
        self.search_query = Some(query.to_string());
        self.search_matches.clear();
        let lower_query = query.to_lowercase();
        let case_insensitive_query = lower_query.as_str();

        for (msg_idx, msg) in self.messages.iter().enumerate() {
            for (part_idx, part) in msg.parts.iter().enumerate() {
                let part_content = match part {
                    MsgPart::Text { content } => content.clone(),
                    MsgPart::Reasoning { content, .. } => content.clone(),
                    MsgPart::ToolCall {
                        name,
                        input,
                        output,
                        ..
                    } => {
                        format!("{}\n{}\n{}", name, input, output)
                    }
                    MsgPart::Image { alt_text, .. } => {
                        format!("[Image: {}]", alt_text)
                    }
                };

                let lower_content = part_content.to_lowercase();
                let mut start = 0;
                while let Some(pos) = lower_content[start..].find(case_insensitive_query) {
                    let abs_start = start + pos;
                    let abs_end = abs_start + query.len();
                    let line_in_msg = part_content[..abs_start].matches('\n').count();
                    self.search_matches.push(SearchMatch {
                        msg_idx,
                        part_idx,
                        line_in_msg,
                        start: abs_start,
                        end: abs_end,
                    });
                    start = abs_start + 1;
                }
            }
        }
        self.search_current = 0;
        if !self.search_matches.is_empty() {
            self.search_visible = true;
            self.sel_msg = Some(self.search_matches[0].msg_idx);
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_current = (self.search_current + 1) % self.search_matches.len();
        let current_match = &self.search_matches[self.search_current];
        self.sel_msg = Some(current_match.msg_idx);
        self.auto_scroll = true;
        let visible_lines = self.visible_height.saturating_sub(4);
        self.scroll = current_match.line_in_msg.saturating_sub(visible_lines / 2);
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        if self.search_current == 0 {
            self.search_current = self.search_matches.len() - 1;
        } else {
            self.search_current = self.search_current.saturating_sub(1);
        }
        let current_match = &self.search_matches[self.search_current];
        self.sel_msg = Some(current_match.msg_idx);
        self.auto_scroll = true;
        let visible_lines = self.visible_height.saturating_sub(4);
        self.scroll = current_match.line_in_msg.saturating_sub(visible_lines / 2);
    }

    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.search_matches.clear();
        self.search_current = 0;
        self.search_visible = false;
    }

    pub fn is_searching(&self) -> bool {
        self.search_visible && self.search_query.is_some()
    }

    fn find_match_for_msg(&self, msg_idx: usize) -> Option<&SearchMatch> {
        self.search_matches.iter().find(|m| m.msg_idx == msg_idx)
    }

    #[allow(dead_code)]
    fn get_message_content_for_search(&self, idx: usize) -> String {
        let mut content = String::new();
        if let Some(msg) = self.messages.get(idx) {
            for part in &msg.parts {
                match part {
                    MsgPart::Text { content: c } => {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(c);
                    }
                    MsgPart::Reasoning { content: c, .. } => {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(c);
                    }
                    MsgPart::ToolCall { name, output, .. } => {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(name);
                        content.push_str(output);
                    }
                    MsgPart::Image { alt_text, .. } => {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(&format!("[Image: {}]", alt_text));
                    }
                }
            }
        }
        content
    }
}

impl Default for MessagesWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

fn pad_line_to_width(line: &mut Line<'_>, target_width: u16, style: Style) {
    use unicode_width::UnicodeWidthStr;
    let current_width: usize = line.spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();
    let pad = target_width.saturating_sub(current_width as u16) as usize;
    if pad > 0 {
        line.spans.push(Span::styled(" ".repeat(pad), style));
    }
}

impl Widget for &MessagesWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        if self.messages.is_empty() {
            let text = Line::from(Span::styled(
                "No messages yet. Type a prompt to begin.",
                Style::default().fg(self.theme.muted),
            ));
            let paragraph = Paragraph::new(text)
                .alignment(ratatui::layout::Alignment::Center)
                .wrap(Wrap { trim: true });
            paragraph.render(area, buf);
            return;
        }

        let available = area.height as usize;

        let cache = self.get_layout_cache();
        let total_lines = cache.total_lines();
        let max_scroll = total_lines.saturating_sub(available);
        let scroll = if self.scroll == usize::MAX {
            max_scroll
        } else {
            self.scroll.min(max_scroll)
        };

        let visible_msg_range = {
            let (start, end) = cache.find_visible_range(scroll, available);
            start..end
        };

        let mut lines: Vec<Line<'_>> = Vec::new();
        for (idx, msg) in self.messages.iter().enumerate() {
            if idx < visible_msg_range.start {
                continue;
            }
            if idx >= visible_msg_range.end {
                break;
            }

            let current_match = self.find_match_for_msg(idx);
            let is_search_match = self.search_visible && current_match.is_some();
            let match_bg = if is_search_match {
                Some(self.theme.selection)
            } else {
                None
            };
            match &msg.role {
                MessageRole::User => {
                    if self.show_timestamps {
                        if let Some(ts) = msg.timestamp {
                            lines.push(Line::from(Span::styled(
                                format_time(ts),
                                Style::default().fg(self.theme.muted),
                            )));
                        }
                    }
                    let bar_color = if msg.is_plan_mode.unwrap_or(false) {
                        self.theme.warning
                    } else {
                        self.theme.primary
                    };
                    let user_bg = self.theme.input_bg;
                    let bar_style = if let Some(bg) = match_bg {
                        Style::default().fg(bar_color).bg(bg)
                    } else {
                        Style::default().fg(bar_color).bg(user_bg)
                    };
                    let text_style = if let Some(bg) = match_bg {
                        Style::default().fg(self.theme.primary).bg(bg)
                    } else {
                        Style::default().fg(self.theme.primary).bg(user_bg)
                    };
                    let highlight_style = Style::default()
                        .fg(self.theme.primary)
                        .bg(self.theme.selection)
                        .add_modifier(Modifier::REVERSED);
                    let pad_style = if let Some(bg) = match_bg {
                        Style::default().bg(bg)
                    } else {
                        Style::default().bg(user_bg)
                    };
                    let mut user_lines: Vec<Line> = Vec::new();
                    for (part_idx, part) in msg.parts.iter().enumerate() {
                        if let MsgPart::Text { content } = part {
                            for (line_idx, text_line) in content.lines().enumerate() {
                                let line_prefix = if line_idx == 0 {
                                    Some(Span::styled("│ ", bar_style))
                                } else {
                                    None
                                };
                                if let Some(m) = current_match {
                                    if m.part_idx == part_idx {
                                        let line_start = content.find(text_line).unwrap_or(0);
                                        let line_end = line_start + text_line.len();
                                        if m.start < line_end && m.end > line_start {
                                            let rel_start = m.start.saturating_sub(line_start);
                                            let rel_end =
                                                m.end.min(line_end).saturating_sub(line_start);
                                            let before = &text_line[..rel_start];
                                            let matched = &text_line[rel_start..rel_end];
                                            let after = &text_line[rel_end..];
                                            let mut spans = Vec::new();
                                            if let Some(p) = line_prefix {
                                                spans.push(p);
                                            }
                                            if !before.is_empty() {
                                                spans.push(Span::styled(before.to_string(), text_style));
                                            }
                                            spans.push(Span::styled(matched, highlight_style));
                                            if !after.is_empty() {
                                                spans.push(Span::styled(after.to_string(), text_style));
                                            }
                                            user_lines.push(Line::from(spans));
                                        } else if let Some(p) = line_prefix {
                                            user_lines.push(Line::from(vec![p, Span::styled(text_line.to_string(), text_style)]));
                                        } else {
                                            user_lines.push(Line::from(Span::styled(text_line.to_string(), text_style)));
                                        }
                                    } else if let Some(p) = line_prefix {
                                        user_lines.push(Line::from(vec![p, Span::styled(text_line.to_string(), text_style)]));
                                    } else {
                                        user_lines.push(Line::from(Span::styled(text_line.to_string(), text_style)));
                                    }
                                } else if let Some(p) = line_prefix {
                                    user_lines.push(Line::from(vec![p, Span::styled(text_line.to_string(), text_style)]));
                                } else {
                                    user_lines.push(Line::from(Span::styled(text_line.to_string(), text_style)));
                                }
                            }
                        }
                    }
                    for line in &mut user_lines {
                        pad_line_to_width(line, area.width, pad_style);
                    }
                    lines.extend(user_lines);
                }
                MessageRole::Assistant => {
                    if self.show_timestamps {
                        if let Some(ts) = msg.timestamp {
                            lines.push(Line::from(Span::styled(
                                format_time(ts),
                                Style::default().fg(self.theme.muted),
                            )));
                        }
                    }
                    let is_thinking = msg.is_thinking_first();
                    let bar_style = if is_thinking {
                        if let Some(bg) = match_bg {
                            Style::default().fg(self.theme.muted).bg(bg)
                        } else {
                            Style::default().fg(self.theme.muted)
                        }
                    } else {
                        if let Some(bg) = match_bg {
                            Style::default().fg(self.theme.secondary).bg(bg)
                        } else {
                            Style::default().fg(self.theme.secondary)
                        }
                    };
                    if !self.streaming_tokens.is_empty() {
                        let streaming_style = Style::default()
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::ITALIC);
                        lines.push(Line::from(vec![
                            Span::styled("│ ", bar_style),
                            Span::styled("Thinking...", streaming_style),
                        ]));
                        lines.push(Line::from(Span::styled(
                            format!("  {}", self.streaming_tokens),
                            streaming_style,
                        )));
                    }
                    let mut prev_was_reasoning = false;
                    for part in &msg.parts {
                        match part {
                            MsgPart::Text { content } => {
                                let rendered = render_markdown(content, &self.theme);
                                lines.extend(rendered);
                                prev_was_reasoning = false;
                            }
                            MsgPart::Reasoning { content, collapsed } => {
                                if !prev_was_reasoning {
                                    if *collapsed || !self.show_thinking {
                                        lines.push(Line::from(Span::styled(
                                            "Thinking",
                                            Style::default().fg(self.theme.primary),
                                        )));
                                    } else {
                                        lines.push(Line::from(Span::styled(
                                            "Thinking",
                                            Style::default()
                                                .fg(self.theme.primary)
                                                .add_modifier(Modifier::BOLD),
                                        )));
                                    }
                                }
                                if self.show_thinking && !*collapsed {
                                    let reasoning_style = Style::default().fg(self.theme.muted);
                                    for text_line in content.lines() {
                                        lines.push(Line::from(vec![
                                            Span::raw("  "),
                                            Span::styled(text_line, reasoning_style),
                                        ]));
                                    }
                                }
                                prev_was_reasoning = true;
                            }
                            MsgPart::ToolCall {
                                id: _,
                                name,
                                input,
                                status,
                                output: _,
                                duration_ms,
                                exit_code: _,
                                output_lines: _,
                            } => {
                                let target = extract_tool_target(name, input);
                                let display_name = if target.is_empty() {
                                    name.clone()
                                } else {
                                    format!("{} {}", name, target)
                                };
                                let spinner = match status {
                                    ToolStatus::Running => "⟳",
                                    ToolStatus::Pending => "○",
                                    ToolStatus::Completed => "✓",
                                    ToolStatus::Error => "✗",
                                };
                                let mut base_style = match status {
                                    ToolStatus::Pending => Style::default().fg(self.theme.muted),
                                    ToolStatus::Running => Style::default().fg(self.theme.warning),
                                    ToolStatus::Completed => {
                                        Style::default().fg(self.theme.success)
                                    }
                                    ToolStatus::Error => Style::default().fg(self.theme.muted),
                                };
                                if matches!(status, ToolStatus::Error) {
                                    base_style = base_style.add_modifier(Modifier::CROSSED_OUT);
                                }
                                let mut summary_parts: Vec<String> = Vec::new();
                                if let Some(ms) = duration_ms {
                                    if *ms < 1000 {
                                        summary_parts.push(format!("{}ms", ms));
                                    } else {
                                        summary_parts.push(format!("{:.1}s", *ms as f64 / 1000.0));
                                    }
                                }
                                let summary_str = if summary_parts.is_empty() {
                                    String::new()
                                } else {
                                    format!(" ({})", summary_parts.join(", "))
                                };
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        format!("{spinner} {display_name}{summary_str}"),
                                        base_style,
                                    ),
                                ]));
                            }
                            MsgPart::Image { alt_text, width, height, .. } => {
                                let img_text = format!("📷 Image ({}x{}): {}", width, height, alt_text);
                                lines.push(Line::from(Span::styled(
                                    img_text,
                                    Style::default().fg(self.theme.muted),
                                )));
                            }
                        }
                    }
                }
            }
        }

        let scroll_offset = scroll.saturating_sub(
            cache.get_offset(visible_msg_range.start).unwrap_or(0),
        );
        let visible_start = scroll_offset.min(lines.len().saturating_sub(1));
        let visible_end = (visible_start + available).min(lines.len());
        let visible: Vec<Line<'_>> = lines[visible_start..visible_end].to_vec();

        if self.search_visible {
            let match_count = self.search_matches.len();
            let current = if match_count > 0 {
                self.search_current + 1
            } else {
                0
            };
            let query_display = self.search_query.as_deref().unwrap_or("");
            let search_bar = if match_count > 0 {
                format!(
                    " Search: {} | {}/{} | n:next N:prev Esc:close ",
                    query_display, current, match_count
                )
            } else {
                format!(" Search: {} | no matches | Esc:close ", query_display)
            };
            let search_style = Style::default()
                .fg(self.theme.foreground)
                .bg(self.theme.selection);
            let mut display_lines = vec![Line::from(Span::styled(search_bar, search_style))];
            display_lines.push(Line::from(""));
            display_lines.extend(visible);
            let paragraph = Paragraph::new(display_lines).wrap(Wrap { trim: true });
            paragraph.render(area, buf);
        } else {
            let paragraph = Paragraph::new(visible).wrap(Wrap { trim: true });
            paragraph.render(area, buf);
        }
    }
}

fn render_markdown(text: &str, theme: &Arc<Theme>) -> Vec<Line<'static>> {
    if text.is_empty() {
        return vec![Line::from("")];
    }

    let has_code_block = text.contains("```");

    if has_code_block {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut in_code = false;
        let mut code_lang = String::new();
        let mut code_buf = String::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code {
                    let code_lines: Vec<&str> = code_buf.lines().collect();
                    let lang_upper = code_lang.to_uppercase();
                    let line_count = code_lines.len();
                    let needs_line_numbers = line_count > 5;
                    let highlighted = highlight_code(&code_buf, &code_lang, theme.code_theme());
                    let base_idx = lines.len();
                    if !lang_upper.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("  ┌─ {lang_upper} "),
                            Style::default()
                                .fg(theme.muted)
                                .add_modifier(Modifier::BOLD),
                        )));
                    }
                    lines.extend(highlighted.clone());
                    if needs_line_numbers {
                        for (i, highlighted_line) in highlighted.iter().enumerate() {
                            lines[base_idx + i] = Line::from(vec![
                                Span::styled(
                                    format!("{:4} │ ", i + 1),
                                    Style::default().fg(theme.muted),
                                ),
                                Span::raw(
                                    highlighted_line
                                        .spans
                                        .iter()
                                        .map(|s| s.content.as_ref())
                                        .collect::<String>(),
                                ),
                            ]);
                        }
                    }
                    code_buf.clear();
                    code_lang.clear();
                    in_code = false;
                } else {
                    code_lang = trimmed.trim_start_matches("```").trim().to_string();
                    in_code = true;
                }
                continue;
            }
            if in_code {
                code_buf.push_str(line);
                code_buf.push('\n');
            } else {
                let rendered = render_md_line(trimmed, theme);
                lines.extend(rendered);
            }
        }

        if in_code && !code_buf.is_empty() {
            let code_lines: Vec<&str> = code_buf.lines().collect();
            let lang_upper = code_lang.to_uppercase();
            let line_count = code_lines.len();
            let needs_line_numbers = line_count > 5;
            let highlighted = highlight_code(&code_buf, &code_lang, theme.code_theme());
            let base_idx = lines.len();
            if !lang_upper.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("  ┌─ {lang_upper} "),
                    Style::default()
                        .fg(theme.muted)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            lines.extend(highlighted.clone());
            if needs_line_numbers {
                for (i, highlighted_line) in highlighted.iter().enumerate() {
                    lines[base_idx + i] = Line::from(vec![
                        Span::styled(format!("{:4} │ ", i + 1), Style::default().fg(theme.muted)),
                        Span::raw(
                            highlighted_line
                                .spans
                                .iter()
                                .map(|s| s.content.as_ref())
                                .collect::<String>(),
                        ),
                    ]);
                }
            }
        }

        lines
    } else {
        let mut lines: Vec<Line<'static>> = Vec::new();
        for line in text.lines() {
            let rendered = render_md_line(line.trim(), theme);
            lines.extend(rendered);
        }
        lines
    }
}

fn parse_line_with_urls(line: &str, theme: &Arc<Theme>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last_end = 0;

    #[derive(Debug)]
    struct Match {
        start: usize,
        end: usize,
        text: String,
        is_url: bool,
    }

    let mut matches: Vec<Match> = Vec::new();

    for mat in URL_REGEX.find_iter(line) {
        matches.push(Match {
            start: mat.start(),
            end: mat.end(),
            text: mat.as_str().to_string(),
            is_url: true,
        });
    }

    for mat in FILE_PATH_REGEX.find_iter(line) {
        let path = mat.as_str().trim();
        matches.push(Match {
            start: mat.start() + (mat.as_str().len() - mat.as_str().trim_start().len()),
            end: mat.start() + (mat.as_str().len() - mat.as_str().trim_end().len()),
            text: path.to_string(),
            is_url: false,
        });
    }

    matches.sort_by_key(|m| m.start);

    for m in matches {
        if m.start > last_end {
            let before = &line[last_end..m.start];
            spans.extend(parse_plain_text(before, theme));
        }

        let link_text = if m.is_url {
            wrap_osc8(&m.text, &m.text)
        } else {
            let abs_path = if m.text.starts_with("~/") {
                if let Ok(home) = std::env::var("HOME") {
                    m.text.replace("~", &home)
                } else {
                    m.text.clone()
                }
            } else if m.text.starts_with("./") || m.text.starts_with("../") {
                std::path::Path::new(&m.text)
                    .canonicalize()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| m.text.clone())
            } else {
                m.text.clone()
            };
            wrap_osc8(&abs_path, &m.text)
        };

        spans.push(Span::styled(
            link_text,
            Style::default()
                .fg(theme.link)
                .add_modifier(Modifier::UNDERLINED),
        ));
        last_end = m.end;
    }

    if last_end < line.len() {
        let after = &line[last_end..];
        spans.extend(parse_plain_text(after, theme));
    }

    spans
}

fn parse_plain_text(text: &str, theme: &Arc<Theme>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut remaining = text.to_string();

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find('`') {
            if pos > 0 {
                let before = &remaining[..pos];
                spans.push(Span::raw(before.to_string()));
            }
            let rest = &remaining[pos + 1..];
            if let Some(end) = rest.find('`') {
                let code = &rest[..end];
                spans.push(Span::styled(
                    code.to_string(),
                    Style::default().fg(theme.primary).bg(theme.selection),
                ));
                remaining = rest[end + 1..].to_string();
            } else {
                spans.push(Span::raw(remaining[pos..].to_string()));
                remaining.clear();
            }
        } else if let Some(pos) = remaining.find("**") {
            if pos > 0 {
                let before = &remaining[..pos];
                spans.push(Span::raw(before.to_string()));
            }
            let rest = &remaining[pos + 2..];
            if let Some(end) = rest.find("**") {
                let bold = &rest[..end];
                spans.push(Span::styled(
                    bold.to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                remaining = rest[end + 2..].to_string();
            } else {
                spans.push(Span::raw(remaining[pos..].to_string()));
                remaining.clear();
            }
        } else if let Some(pos) = remaining.find('*') {
            if pos > 0 {
                let before = &remaining[..pos];
                spans.push(Span::raw(before.to_string()));
            }
            let rest = &remaining[pos + 1..];
            if let Some(end) = rest.find('*') {
                let italic = &rest[..end];
                spans.push(Span::styled(
                    italic.to_string(),
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
                remaining = rest[end + 1..].to_string();
            } else {
                spans.push(Span::raw(remaining[pos..].to_string()));
                remaining.clear();
            }
        } else {
            spans.push(Span::raw(remaining.clone()));
            remaining.clear();
        }
    }

    spans
}

fn render_md_line(line: &str, theme: &Arc<Theme>) -> Vec<Line<'static>> {
    if line.is_empty() {
        return vec![Line::from("")];
    }

    if line.starts_with("# ") {
        return vec![Line::from(Span::styled(
            line.trim_start_matches("# ").to_string(),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))];
    }
    if line.starts_with("## ") {
        return vec![Line::from(Span::styled(
            line.trim_start_matches("## ").to_string(),
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ))];
    }
    if line.starts_with("### ") {
        return vec![Line::from(Span::styled(
            line.trim_start_matches("### ").to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
    }
    if line.starts_with("- ") || line.starts_with("* ") {
        let content = line.trim_start_matches("- ").trim_start_matches("* ");
        return vec![Line::from(vec![
            Span::styled("• ", Style::default().fg(theme.primary)),
            Span::raw(content.to_string()),
        ])];
    }

    let spans = parse_line_with_urls(line, theme);
    if spans.is_empty() {
        vec![Line::from("")]
    } else {
        vec![Line::from(spans)]
    }
}

fn format_time(ts: i64) -> String {
    let dt = chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_default();
    format!("  {dt}")
}

pub fn highlight_code(code: &str, lang: &str, code_theme: &str) -> Vec<Line<'static>> {
    let syntax = SYNTAX_SET
        .find_syntax_by_token(lang)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = THEME_SET
        .themes
        .get(code_theme)
        .unwrap_or_else(|| &THEME_SET.themes["base16-ocean.dark"]);
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for code_line in code.lines() {
        let highlighted = highlighter
            .highlight_line(code_line, &SYNTAX_SET)
            .unwrap_or_default();
        let spans: Vec<Span<'static>> = highlighted
            .iter()
            .map(|(style, text)| {
                let fg = ratatui::style::Color::Rgb(
                    style.foreground.r,
                    style.foreground.g,
                    style.foreground.b,
                );
                Span::styled(text.to_string(), Style::default().fg(fg))
            })
            .collect();
        lines.push(Line::from(spans));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_down_to_bottom_reenables_auto_scroll() {
        let mut widget = MessagesWidget::default();
        widget.set_visible_height(3);
        for i in 0..12 {
            widget.add_user_message(format!("msg {i}"), None);
        }

        widget.scroll_to_top();
        assert!(!widget.auto_scroll);

        for _ in 0..64 {
            widget.scroll_down();
        }

        assert!(widget.is_at_bottom());
        assert!(widget.auto_scroll);
    }

    #[test]
    fn streaming_token_creates_assistant_placeholder_and_follows_bottom() {
        let mut widget = MessagesWidget::default();
        widget.set_visible_height(4);
        widget.add_user_message("hello".to_string(), None);
        widget.scroll_to_bottom();

        widget.add_streaming_token("partial");

        assert!(matches!(
            widget.messages.last().map(|m| &m.role),
            Some(MessageRole::Assistant)
        ));
        assert_eq!(widget.streaming_tokens, "partial");
        assert_eq!(widget.scroll, usize::MAX);
    }
}

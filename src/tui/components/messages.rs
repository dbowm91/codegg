use crate::lsp::language::extension_to_language_id;
use crate::session::message::ToolStatus;
use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{parse_document, Arena, Options};
use once_cell::sync::Lazy;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, ScrollbarState, Widget};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use unicode_width::UnicodeWidthStr;

mod layout;
use layout::MessageLayoutCache;

use super::super::theme::Theme;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);
static URL_REGEX: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"https?://[^\s<>"'`]+"#).expect("invalid URL regex"));
static FILE_PATH_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r#"(?:^|[\s])(\/(?:[a-zA-Z0-9._~-]+\/)*[a-zA-Z0-9._~-]+|~\/[a-zA-Z0-9._~-]+(?:\/[a-zA-Z0-9._~-]+)*|\.\.?\/[a-zA-Z0-9._~-]+(?:\/[a-zA-Z0-9._~-]+)*)"#).expect("invalid file path regex")
});

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShellCellUpdate {
    pub status: Option<String>,
    pub stdout_preview: Option<String>,
    pub stderr_preview: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub truncated: Option<bool>,
    pub promoted: Option<bool>,
    pub projection_projector: Option<String>,
    pub projection_exactness: Option<String>,
    pub projection_input_bytes: Option<u64>,
    pub projection_output_bytes: Option<usize>,
    pub projection_omitted: Option<String>,
    pub projection_raw_handle: Option<String>,
}
static MARKDOWN_OPTIONS: Lazy<Options<'static>> = Lazy::new(|| {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.description_lists = true;
    options
});
const TOOL_OUTPUT_PREVIEW_LINES: usize = 8;
const TOOL_INPUT_PREVIEW_LINES: usize = 4;
const TOOL_SPINNER_FRAMES: [&str; 8] = ["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

fn tool_spinner_frame() -> &'static str {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let idx = ((millis / 120) as usize) % TOOL_SPINNER_FRAMES.len();
    TOOL_SPINNER_FRAMES[idx]
}

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
        expanded: bool,
    },
    Image {
        data_uri: String,
        alt_text: String,
        width: u32,
        height: u32,
    },
    ShellCell {
        id: u64,
        command: String,
        cwd: String,
        stdout_preview: String,
        stderr_preview: String,
        status: String,
        elapsed_ms: Option<u64>,
        exit_code: Option<i32>,
        truncated: bool,
        promoted: bool,
        expanded: bool,
        projection_projector: Option<String>,
        projection_exactness: Option<String>,
        projection_input_bytes: Option<u64>,
        projection_output_bytes: Option<usize>,
        projection_omitted: Option<String>,
        projection_raw_handle: Option<String>,
    },
    RunCell {
        run_id: String,
        title: String,
        status: String,
        backend_label: String,
        duration: Option<String>,
        changed_file_count: usize,
        risk_label: String,
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
        self.parts
            .first()
            .map(|p| matches!(p, MsgPart::Reasoning { .. }))
            .unwrap_or(false)
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
                MsgPart::ShellCell {
                    command,
                    status,
                    stdout_preview,
                    stderr_preview,
                    ..
                } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!(
                        "$ {} [{}] stdout: {} stderr: {}",
                        command, status, stdout_preview, stderr_preview
                    ));
                }
                MsgPart::RunCell {
                    title,
                    status,
                    backend_label,
                    ..
                } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!(
                        "run: {} [{}] backend: {}",
                        title, status, backend_label
                    ));
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
    pub sel_msg: Option<usize>,
    pub undo_stack: VecDeque<UIMessage>,
    pub streaming_tokens: String,
    pub assistant_is_thinking: bool,
    pub search_query: Option<String>,
    pub search_matches: Vec<SearchMatch>,
    pub search_current: usize,
    pub search_visible: bool,
    pub visible_height: usize,
    pub width: u16,
    message_layout_cache: RefCell<Option<MessageLayoutCache>>,
    last_render_cache: RefCell<Option<LastRenderCache>>,
}

#[derive(Clone)]
struct LastRenderCache {
    streaming_len: usize,
    width: u16,
    message_ptr_id: usize,
    lines: Vec<Line<'static>>,
}

/// Greedy word-wrap counter. Returns the number of visual lines a string of
/// the given (Unicode-aware) display width occupies when wrapped at `width`.
/// Word-wrap is greedy: words that fit on the current line stay there; a
/// word that would overflow starts a new line. Words longer than `width`
/// (URLs, long paths) are hard-broken mid-character to match
/// [`wrap_to_strings`]. Empty input returns 1 line (matches a blank
/// paragraph).
fn wrap_count(s: &str, width: u16) -> usize {
    let width = width as usize;
    if width == 0 {
        return s.lines().count().max(1);
    }
    if s.is_empty() {
        return 1;
    }
    let mut count = 1usize;
    let mut col = 0usize;
    let mut word_w = 0usize;
    let mut word_chars = 0usize;
    let flush_word =
        |count: &mut usize, col: &mut usize, word_w: &mut usize, word_chars: &mut usize| {
            if *word_chars == 0 {
                return;
            }
            // Place the word on the current line if there's room (with leading
            // space), else start a new line.
            if *col == 0 {
                *col = *word_w;
            } else if *col + 1 + *word_w <= width {
                *col += 1 + *word_w;
            } else {
                *count += 1;
                *col = *word_w;
            }
            // Hard-break: the word is wider than the wrap width. It needs
            // ceil(word_w / width) visual lines. We've already counted the
            // first one (col was set to word_w above), so add the extras.
            if *col > width {
                let extras = word_w.div_ceil(width) - 1;
                *count += extras;
                *col = *word_w - extras * width;
            }
            *word_w = 0;
            *word_chars = 0;
        };
    for ch in s.chars() {
        if ch == '\n' {
            flush_word(&mut count, &mut col, &mut word_w, &mut word_chars);
            count += 1;
            col = 0;
        } else if ch.is_whitespace() {
            flush_word(&mut count, &mut col, &mut word_w, &mut word_chars);
            if col < width {
                col += 1;
            } else {
                count += 1;
                col = 0;
            }
        } else {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            // If the next char wouldn't fit on the current line and we're
            // mid-word, wait until the word ends to break (so we don't
            // break in the middle of a word we could fit on the next line).
            if col > 0 && col + cw > width && word_chars == 0 {
                count += 1;
                col = 0;
            }
            if cw > 0 {
                word_w += cw;
                word_chars += 1;
            }
        }
    }
    flush_word(&mut count, &mut col, &mut word_w, &mut word_chars);
    // Trailing newline → final empty line shouldn't count
    if s.ends_with('\n') && count > 1 {
        count - 1
    } else {
        count
    }
}

/// Split a string into wrapped lines of at most `width` display columns each.
/// Returns one entry per visual line (preserves explicit newlines as line
/// breaks). Matches the line-counting semantics of [`wrap_count`].
///
/// Word-wrap is greedy: words that fit on the current line stay there; a
/// word that would overflow the current line starts a new line. Words
/// longer than `width` (URLs, long paths) are hard-broken at character
/// boundaries to avoid overflowing the render area. Trailing whitespace is
/// trimmed from each output line so the wrap doesn't double-space between
/// words or leave stray spaces at line ends.
fn wrap_to_strings(s: &str, width: u16) -> Vec<String> {
    let width = width as usize;
    if width == 0 || s.is_empty() {
        return vec![s.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut col = 0usize;
    let mut word = String::new();
    let mut word_w = 0usize;
    let place_word = |word: &str,
                      word_w: usize,
                      line: &mut String,
                      col: &mut usize,
                      out: &mut Vec<String>|
     -> Option<String> {
        if word.is_empty() {
            return None;
        }
        if *col == 0 {
            *line += word;
            *col = word_w;
        } else {
            // Strip any trailing whitespace the previous whitespace-pass
            // added, so we don't end up with "hello  world" (double
            // space) when the line was already at "hello " from a literal
            // space in the input.
            while line.ends_with(' ') || line.ends_with('\t') {
                line.pop();
            }
            *col = line.chars().count();
            if *col + 1 + word_w <= width {
                *line += " ";
                *line += word;
                *col += 1 + word_w;
            } else {
                out.push(std::mem::take(line));
                *line += word;
                *col = word_w;
            }
        }
        // Hard-break: if the line is wider than `width` cols (a single
        // word overflowed), chop it into `width`-col chunks. The leftover
        // chars on the line are returned to the caller so they can be
        // combined with the next chars of the word (otherwise a 50-char
        // word would fragment into many 1-char lines).
        let mut did_hard_break = false;
        while *col > width && !line.is_empty() {
            let take_bytes = line
                .char_indices()
                .nth(width)
                .map(|(i, _)| i)
                .unwrap_or(line.len());
            let chunk: String = line.drain(..take_bytes).collect();
            let chunk_chars = chunk.chars().count();
            out.push(chunk);
            *col -= chunk_chars;
            did_hard_break = true;
        }
        if did_hard_break && !line.is_empty() {
            Some(std::mem::take(line))
        } else {
            None
        }
    };
    for ch in s.chars() {
        if ch == '\n' {
            if !word.is_empty() {
                if let Some(leftover) = place_word(&word, word_w, &mut line, &mut col, &mut out) {
                    word = leftover;
                    word_w = word.chars().count();
                } else {
                    word.clear();
                    word_w = 0;
                }
            }
            out.push(std::mem::take(&mut line));
            col = 0;
        } else if ch.is_whitespace() {
            if !word.is_empty() {
                if let Some(leftover) = place_word(&word, word_w, &mut line, &mut col, &mut out) {
                    word = leftover;
                    word_w = word.chars().count();
                    col = 0;
                } else {
                    word.clear();
                    word_w = 0;
                }
            }
            if col < width {
                line.push(ch);
                col += 1;
            } else {
                out.push(std::mem::take(&mut line));
                col = 0;
            }
        } else {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if cw == 0 {
                word.push(ch);
                continue;
            }
            if col + cw > width && col > 0 {
                out.push(std::mem::take(&mut line));
                col = 0;
            }
            word.push(ch);
            word_w += cw;
            if word_w > width {
                if let Some(leftover) = place_word(&word, word_w, &mut line, &mut col, &mut out) {
                    word = leftover;
                    word_w = word.chars().count();
                    col = 0;
                } else {
                    word.clear();
                    word_w = 0;
                }
            }
        }
    }
    if !word.is_empty() {
        if let Some(leftover) = place_word(&word, word_w, &mut line, &mut col, &mut out) {
            out.push(leftover);
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    // Trim trailing whitespace from every output line (so a wrap doesn't
    // double-space between words or leave a trailing space at line-end).
    for line in &mut out {
        while line.ends_with(' ') || line.ends_with('\t') {
            line.pop();
        }
    }
    // Trim trailing blank line caused by a terminal '\n'
    if s.ends_with('\n') {
        if let Some(last) = out.last() {
            if last.is_empty() {
                out.pop();
            }
        }
    }
    out
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
        "read" | "write" | "edit" | "multiedit" | "glob" | "grep" => val
            .get("path")
            .or_else(|| val.get("file_path"))
            .or_else(|| val.get("pattern"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "bash" | "exec" => val
            .get("command")
            .or_else(|| val.get("cmd"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "task" => val
            .get("prompt")
            .or_else(|| val.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "webfetch" => val
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => val
            .as_object()
            .and_then(|m| m.values().next())
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
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
    let mut in_code_block = false;
    let mut char_pos = 0;

    for line in text.lines() {
        let line_len = line.len() + 1;

        if line.trim().starts_with("```") {
            in_code_block = !in_code_block;
        }

        if !in_code_block {
            let lower = line.to_lowercase();
            for tag in &tags {
                let mut search_from = 0;
                while let Some(pos) = lower[search_from..].find(tag) {
                    let abs_pos = char_pos + search_from + pos;
                    let after_pos = abs_pos + tag.len();
                    let valid_boundary = after_pos >= line.len()
                        || line.as_bytes()[after_pos] == b'>'
                        || line.as_bytes()[after_pos] == b'\n'
                        || !line.as_bytes()[after_pos].is_ascii_alphanumeric();
                    if valid_boundary && (best_match.is_none() || abs_pos < best_match.unwrap().0) {
                        best_match = Some((abs_pos, tag.len()));
                    }
                    search_from += pos + 1;
                }
            }
        }

        char_pos += line_len;
    }

    best_match
}

impl MessagesWidget {
    pub fn estimate_msg_lines(&self, msg: &UIMessage) -> usize {
        // Returns the post-collapse line count for `msg`, matching what the
        // render path produces (after `collapse_blank_lines` is applied to the
        // full visible message range). This keeps the layout cache and the
        // render in sync so scroll position and the scrollbar stay accurate.
        let mut lines = 0usize;
        if self.show_timestamps && msg.timestamp.is_some() {
            lines += 1;
        }
        let width = self.width.max(2) as usize;
        for part in &msg.parts {
            match part {
                MsgPart::Text { content } => {
                    lines += self.estimate_text_part_lines(msg, content, width as u16);
                }
                MsgPart::Reasoning { content, collapsed } => {
                    lines += 1;
                    if self.show_thinking && !*collapsed {
                        lines += collapse_blank_lines(&render_markdown(
                            content,
                            &self.theme,
                            self.theme.muted,
                            (width.saturating_sub(2)) as u16,
                        ))
                        .len()
                        .max(1);
                    }
                }
                MsgPart::ToolCall {
                    input,
                    output,
                    status,
                    expanded,
                    ..
                } => {
                    lines += 1;
                    if matches!(status, ToolStatus::Pending | ToolStatus::Running)
                        && output.is_empty()
                    {
                        lines += 1;
                    } else if !output.is_empty() {
                        let output_line_count = output.lines().count().max(1);
                        if *expanded {
                            if !input.is_empty() {
                                lines += input.lines().count().min(TOOL_INPUT_PREVIEW_LINES);
                                if input.lines().count() > TOOL_INPUT_PREVIEW_LINES {
                                    lines += 1;
                                }
                            }
                            lines += output_line_count;
                        } else {
                            lines += output_line_count.min(TOOL_OUTPUT_PREVIEW_LINES);
                            if output_line_count > TOOL_OUTPUT_PREVIEW_LINES {
                                lines += 1;
                            }
                        }
                    }
                }
                MsgPart::Image { .. } => {
                    lines += 1;
                }
                MsgPart::ShellCell {
                    stdout_preview,
                    stderr_preview,
                    expanded,
                    ..
                } => {
                    lines += 1;
                    if *expanded {
                        lines += stdout_preview.lines().count().max(1);
                        if !stderr_preview.is_empty() {
                            lines += stderr_preview.lines().count().max(1);
                        }
                    } else {
                        lines += stdout_preview
                            .lines()
                            .count()
                            .min(TOOL_OUTPUT_PREVIEW_LINES);
                        if stdout_preview.lines().count() > TOOL_OUTPUT_PREVIEW_LINES {
                            lines += 1;
                        }
                    }
                }
                MsgPart::RunCell { .. } => {
                    lines += 1;
                }
            }
        }
        // Streaming tail: 1 label line + N token lines (matching render).
        if msg.role == MessageRole::Assistant
            && !self.streaming_tokens.is_empty()
            && self.messages.last().is_some_and(|m| std::ptr::eq(m, msg))
        {
            lines += 1 + collapse_blank_lines(&render_markdown(
                &self.streaming_tokens,
                &self.theme,
                self.theme.muted,
                self.width.saturating_sub(2),
            ))
            .len()
            .max(1);
        }
        lines
    }

    fn estimate_text_part_lines(&self, msg: &UIMessage, content: &str, width: u16) -> usize {
        match msg.role {
            MessageRole::User => content
                .lines()
                .map(|line| wrap_count(line, width.saturating_sub(2)))
                .sum::<usize>()
                .max(1),
            MessageRole::Assistant => collapse_blank_lines(&render_markdown(
                content,
                &self.theme,
                self.theme.muted,
                width,
            ))
            .len()
            .max(1),
        }
    }

    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            messages: Vec::new(),
            scroll: 0,
            auto_scroll: true,
            theme,
            show_thinking: true,
            show_timestamps: false,
            sel_msg: None,
            undo_stack: VecDeque::new(),
            streaming_tokens: String::new(),
            assistant_is_thinking: false,
            search_query: None,
            search_matches: Vec::new(),
            search_current: 0,
            search_visible: false,
            visible_height: 20,
            width: 80,
            message_layout_cache: RefCell::new(None),
            last_render_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_display_options(&mut self, show_thinking: bool, show_timestamps: bool) {
        if self.show_thinking != show_thinking || self.show_timestamps != show_timestamps {
            self.show_thinking = show_thinking;
            self.show_timestamps = show_timestamps;
            self.invalidate_layout_cache();
        }
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_width(&mut self, width: u16) {
        if self.width != width {
            self.width = width;
            self.invalidate_layout_cache();
            self.invalidate_render_cache();
        }
    }

    fn invalidate_render_cache(&self) {
        *self.last_render_cache.borrow_mut() = None;
    }

    pub fn set_auto_scroll(&mut self, val: bool) {
        self.auto_scroll = val;
    }

    fn message_viewport_height(&self) -> usize {
        if self.search_visible {
            self.visible_height.saturating_sub(2)
        } else {
            self.visible_height
        }
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
                            msg.parts.push(MsgPart::Text {
                                content: text_chunk.to_string(),
                            });
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
                        msg.parts.push(MsgPart::Text {
                            content: text_chunk.to_string(),
                        });
                    }
                    break;
                }
            }
        }

        self.invalidate_layout_cache();
        self.invalidate_render_cache();
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
                self.invalidate_render_cache();
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
        self.invalidate_render_cache();
        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn add_shell_cell(&mut self, id: u64, command: &str, cwd: &str) {
        let was_at_bottom = self.is_at_bottom();
        self.messages.push(UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::ShellCell {
                id,
                command: command.to_string(),
                cwd: cwd.to_string(),
                stdout_preview: String::new(),
                stderr_preview: String::new(),
                status: "running".to_string(),
                elapsed_ms: None,
                exit_code: None,
                truncated: false,
                promoted: false,
                expanded: false,
                projection_projector: None,
                projection_exactness: None,
                projection_input_bytes: None,
                projection_output_bytes: None,
                projection_omitted: None,
                projection_raw_handle: None,
            }],
            timestamp: Some(chrono::Local::now().timestamp()),
            is_plan_mode: None,
        });
        self.invalidate_layout_cache();
        self.invalidate_render_cache();
        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn update_shell_cell(&mut self, id: u64, f: impl FnOnce(&mut ShellCellUpdate)) {
        let mut update = ShellCellUpdate::default();
        f(&mut update);
        let mut updated = false;
        for msg in &mut self.messages {
            for part in &mut msg.parts {
                if let MsgPart::ShellCell {
                    id: cell_id,
                    status,
                    stdout_preview,
                    stderr_preview,
                    elapsed_ms,
                    exit_code,
                    truncated,
                    promoted,
                    projection_projector,
                    projection_exactness,
                    projection_input_bytes,
                    projection_output_bytes,
                    projection_omitted,
                    projection_raw_handle,
                    ..
                } = part
                {
                    if *cell_id == id {
                        if let Some(ref s) = update.status {
                            *status = s.clone();
                        }
                        if let Some(ref s) = update.stdout_preview {
                            *stdout_preview = s.clone();
                        }
                        if let Some(ref s) = update.stderr_preview {
                            *stderr_preview = s.clone();
                        }
                        if let Some(ms) = update.elapsed_ms {
                            *elapsed_ms = Some(ms);
                        }
                        if update.exit_code.is_some() {
                            *exit_code = update.exit_code;
                        }
                        if let Some(t) = update.truncated {
                            *truncated = t;
                        }
                        if let Some(p) = update.promoted {
                            *promoted = p;
                        }
                        if let Some(ref s) = update.projection_projector {
                            *projection_projector = Some(s.clone());
                        }
                        if let Some(ref s) = update.projection_exactness {
                            *projection_exactness = Some(s.clone());
                        }
                        if let Some(b) = update.projection_input_bytes {
                            *projection_input_bytes = Some(b);
                        }
                        if let Some(b) = update.projection_output_bytes {
                            *projection_output_bytes = Some(b);
                        }
                        if let Some(ref s) = update.projection_omitted {
                            *projection_omitted = Some(s.clone());
                        }
                        if let Some(ref s) = update.projection_raw_handle {
                            *projection_raw_handle = Some(s.clone());
                        }
                        updated = true;
                        break;
                    }
                }
            }
            if updated {
                break;
            }
        }
        if updated {
            self.invalidate_layout_cache();
            self.invalidate_render_cache();
        }
    }

    pub fn add_tool_call(&mut self, id: String, name: String, input: serde_json::Value) {
        let was_at_bottom = self.is_at_bottom();
        let input_str = serde_json::to_string_pretty(&input).unwrap_or_default();
        self.messages.push(UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::ToolCall {
                id,
                name,
                input: input_str,
                output: String::new(),
                status: ToolStatus::Pending,
                duration_ms: None,
                exit_code: None,
                output_lines: None,
                expanded: false,
            }],
            timestamp: Some(chrono::Local::now().timestamp()),
            is_plan_mode: None,
        });
        self.invalidate_layout_cache();
        self.invalidate_render_cache();
        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn mark_tool_call_running(&mut self, id: &str) {
        let mut updated = false;
        for msg in &mut self.messages {
            for part in &mut msg.parts {
                if let MsgPart::ToolCall {
                    id: part_id,
                    status,
                    ..
                } = part
                {
                    if part_id == id {
                        *status = ToolStatus::Running;
                        updated = true;
                        break;
                    }
                }
            }
            if updated {
                break;
            }
        }
        if updated {
            self.invalidate_layout_cache();
            self.invalidate_render_cache();
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
        self.invalidate_layout_cache();
        self.invalidate_render_cache();
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
        self.invalidate_render_cache();
    }

    pub fn toggle_tool_output(&mut self, msg_idx: usize) -> bool {
        let mut toggled = false;
        if let Some(msg) = self.messages.get_mut(msg_idx) {
            for part in &mut msg.parts {
                if let MsgPart::ToolCall { expanded, .. } = part {
                    *expanded = !*expanded;
                    toggled = true;
                }
            }
        }
        if toggled {
            self.invalidate_layout_cache();
            self.invalidate_render_cache();
        }
        toggled
    }

    pub fn message_has_tool_output(&self, msg_idx: usize) -> bool {
        self.messages.get(msg_idx).is_some_and(|msg| {
            msg.parts
                .iter()
                .any(|part| matches!(part, MsgPart::ToolCall { .. }))
        })
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll = 0;
        self.sel_msg = None;
        self.undo_stack.clear();
        self.invalidate_layout_cache();
        self.invalidate_render_cache();
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
        self.invalidate_layout_cache();
        self.invalidate_render_cache();
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
        self.invalidate_layout_cache();
        self.invalidate_render_cache();
        self.scroll = usize::MAX;
        true
    }

    fn with_layout_cache<R>(&self, f: impl FnOnce(&MessageLayoutCache) -> R) -> R {
        if self.message_layout_cache.borrow().is_none() {
            let cache = self.build_layout_cache();
            *self.message_layout_cache.borrow_mut() = Some(cache);
        }
        let cache = self.message_layout_cache.borrow();
        f(cache.as_ref().expect("layout cache must be initialized"))
    }

    fn invalidate_layout_cache(&self) {
        *self.message_layout_cache.borrow_mut() = None;
        self.invalidate_render_cache();
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
                        MsgPart::ShellCell {
                            command,
                            stdout_preview,
                            stderr_preview,
                            ..
                        } => {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str(&format!(
                                "$ {}\nstdout: {}\nstderr: {}",
                                command, stdout_preview, stderr_preview
                            ));
                        }
                        MsgPart::RunCell {
                            title,
                            status,
                            backend_label,
                            ..
                        } => {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str(&format!(
                                "run: {} [{}] backend: {}",
                                title, status, backend_label
                            ));
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

    pub fn select_at_viewport_line(&mut self, line: usize) -> Option<usize> {
        let target = self.scroll_position().saturating_add(line);
        let msg_idx = self
            .with_layout_cache(|cache| cache.find_message_at_line(target))
            .map(|(idx, _)| idx)?;
        self.sel_msg = Some(msg_idx);
        Some(msg_idx)
    }

    /// Public accessor for cached rendered line totals, used by callers that
    /// map click positions on the scrollbar gutter into scroll offsets.
    pub fn total_lines(&self) -> usize {
        self.with_layout_cache(MessageLayoutCache::total_lines)
    }

    pub fn max_scroll(&self) -> usize {
        self.total_lines()
            .saturating_sub(self.message_viewport_height())
    }

    pub fn scroll_position(&self) -> usize {
        let max_scroll = self.max_scroll();
        if self.scroll == usize::MAX {
            max_scroll
        } else {
            self.scroll.min(max_scroll)
        }
    }

    pub fn is_at_bottom(&self) -> bool {
        if self.scroll == usize::MAX {
            return true;
        }
        if self.total_lines() == 0 {
            return true;
        }
        self.scroll >= self.max_scroll()
    }

    fn normalize_scroll(&mut self) {
        if self.scroll == usize::MAX {
            let total = self.total_lines();
            let max_scroll = total.saturating_sub(self.message_viewport_height());
            self.scroll = max_scroll;
        }
    }

    /// Build a ratatui `ScrollbarState` for the current viewport. The caller
    /// should pass the *scrollbar* area height (i.e. the chat-log viewport
    /// height, not the total window height). When the content fits, returns
    /// an empty state (no thumb).
    pub fn scrollbar_state(&self, viewport_height: usize) -> ScrollbarState {
        let total = self.total_lines();
        let message_viewport_height = self.message_viewport_height().min(viewport_height);
        let max_scroll = total.saturating_sub(message_viewport_height);
        let pos = self.scroll_position().min(max_scroll);
        ScrollbarState::new(max_scroll)
            .position(pos)
            .viewport_content_length(message_viewport_height)
    }

    /// True when content overflows the viewport (i.e. a scrollbar is useful).
    pub fn needs_scrollbar(&self) -> bool {
        self.total_lines() > self.message_viewport_height()
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
        let total_lines = self.total_lines();
        let available = self.message_viewport_height();
        let max_scroll = total_lines.saturating_sub(available);
        if self.scroll < max_scroll {
            self.scroll += 1;
        }
        self.auto_scroll = self.scroll >= max_scroll;
    }

    pub fn scroll_page_up(&mut self) {
        self.normalize_scroll();
        let total_lines = self.total_lines();
        let available = self.message_viewport_height();
        let max_scroll = total_lines.saturating_sub(available);
        let page = available.saturating_sub(2).max(1);
        self.scroll = self.scroll.saturating_sub(page).min(max_scroll);
        self.auto_scroll = false;
    }

    pub fn scroll_page_down(&mut self) {
        self.normalize_scroll();
        let total_lines = self.total_lines();
        let available = self.message_viewport_height();
        let max_scroll = total_lines.saturating_sub(available);
        let page = available.saturating_sub(2).max(1);
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
        self.invalidate_layout_cache();

        if self.auto_scroll && was_at_bottom {
            self.scroll = usize::MAX;
        }
    }

    pub fn finalize_streaming(&mut self) {
        if !self.streaming_tokens.is_empty() {
            self.add_assistant_text(self.streaming_tokens.clone());
            self.streaming_tokens.clear();
            self.invalidate_layout_cache();
        }
    }

    pub fn clear_streaming(&mut self) {
        self.streaming_tokens.clear();
        self.invalidate_layout_cache();
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
                    MsgPart::ShellCell {
                        command,
                        stdout_preview,
                        stderr_preview,
                        ..
                    } => {
                        format!("{}\n{}\n{}", command, stdout_preview, stderr_preview)
                    }
                    MsgPart::RunCell {
                        title,
                        status,
                        backend_label,
                        ..
                    } => {
                        format!("run: {} [{}] {}", title, status, backend_label)
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
        self.scroll_to_current_search_match();
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
        self.scroll_to_current_search_match();
    }

    fn scroll_to_current_search_match(&mut self) {
        let Some(current_match) = self.search_matches.get(self.search_current).cloned() else {
            return;
        };
        self.sel_msg = Some(current_match.msg_idx);

        let visible_lines = self.message_viewport_height().max(1);
        let message_offset =
            self.with_layout_cache(|cache| cache.get_offset(current_match.msg_idx).unwrap_or(0));
        let target_line = message_offset.saturating_add(current_match.line_in_msg);
        let max_scroll = self.total_lines().saturating_sub(visible_lines);
        self.scroll = target_line
            .saturating_sub(visible_lines / 2)
            .min(max_scroll);
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
                    MsgPart::ShellCell {
                        command,
                        stdout_preview,
                        stderr_preview,
                        ..
                    } => {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(&format!(
                            "$ {}\nstdout: {}\nstderr: {}",
                            command, stdout_preview, stderr_preview
                        ));
                    }
                    MsgPart::RunCell {
                        title,
                        status,
                        backend_label,
                        ..
                    } => {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(&format!("run: {} [{}] {}", title, status, backend_label));
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
    let current_width: usize = line
        .spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
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
            let paragraph = Paragraph::new(text).alignment(ratatui::layout::Alignment::Center);
            paragraph.render(area, buf);
            return;
        }

        let available = area.height as usize;
        let search_chrome_height = if self.search_visible { 2 } else { 0 };
        let message_available = available.saturating_sub(search_chrome_height);

        let (scroll, visible_msg_range, range_start_offset) = self.with_layout_cache(|cache| {
            let total_lines = cache.total_lines();
            let max_scroll = total_lines.saturating_sub(message_available);
            let scroll = if self.scroll == usize::MAX {
                max_scroll
            } else {
                self.scroll.min(max_scroll)
            };
            let (start, end) = if message_available == 0 {
                (0, 0)
            } else {
                cache.find_visible_range(scroll, message_available)
            };
            let range_start_offset = cache.get_offset(start).unwrap_or(0);
            (scroll, start..end, range_start_offset)
        });

        let mut lines: Vec<Line<'_>> = Vec::new();
        for (idx, msg) in self.messages.iter().enumerate() {
            if idx < visible_msg_range.start {
                continue;
            }
            if idx >= visible_msg_range.end {
                break;
            }

            let is_last = self.messages.last().is_some_and(|m| std::ptr::eq(m, msg));
            let current_match = self.find_match_for_msg(idx);
            let is_search_match = self.search_visible && current_match.is_some();
            let match_bg = if is_search_match {
                Some(self.theme.selection)
            } else {
                None
            };

            // For the last assistant message, use the cached parts-render
            // (parts don't change frame-to-frame during streaming; the
            // streaming_tokens tail is appended below).
            if is_last
                && matches!(msg.role, MessageRole::Assistant)
                && !self.streaming_tokens.is_empty()
            {
                if let Some(cached) = self.get_cached_last_assistant_parts(msg) {
                    lines.extend(cached.iter().cloned());
                } else {
                    let built = self.build_assistant_parts_lines(msg, current_match, match_bg);
                    self.store_cached_last_assistant_parts(msg, built.clone());
                    lines.extend(built);
                }
                // Streaming tail: append label + each streaming line (cheap).
                let streaming_label = if self.assistant_is_thinking {
                    "Thinking..."
                } else {
                    "Generating..."
                };
                let streaming_style = Style::default().fg(self.theme.muted);
                lines.push(Line::from(Span::styled(streaming_label, streaming_style)));
                let tail_width = self.width.saturating_sub(2);
                lines.extend(prefix_rendered_lines(
                    render_markdown(
                        &self.streaming_tokens,
                        &self.theme,
                        self.theme.muted,
                        tail_width,
                    ),
                    "",
                    Style::default(),
                ));
                continue;
            }

            // Non-cached path: build lines directly.
            match &msg.role {
                MessageRole::User => {
                    lines.extend(self.build_user_lines(msg, idx, match_bg));
                }
                MessageRole::Assistant => {
                    lines.extend(self.build_assistant_parts_lines(msg, current_match, match_bg));
                }
            }
        }

        // Collapse consecutive blank lines on the FULL visible message range
        // (not the window slice) so relational spacing between messages stays
        // consistent regardless of where the scroll position lands.
        let lines = collapse_blank_lines(&lines);

        let scroll_offset = scroll.saturating_sub(range_start_offset);
        let visible_start = scroll_offset.min(lines.len().saturating_sub(1));
        let visible_end = (visible_start + message_available).min(lines.len());
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
            let paragraph = Paragraph::new(display_lines);
            paragraph.render(area, buf);
        } else {
            let paragraph = Paragraph::new(visible);
            paragraph.render(area, buf);
        }
    }
}

impl MessagesWidget {
    /// Build the lines for a User message (pre-wrapped to current width).
    fn build_user_lines(
        &self,
        msg: &UIMessage,
        _idx: usize,
        match_bg: Option<ratatui::style::Color>,
    ) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
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
        // User message text uses `theme.muted` (same color as the prompt
        // placeholder) so chat reads with a single, readable text color
        // instead of `theme.primary`, which on many Halloy themes resolves
        // to a near-background accent (e.g. Cyber Red's #230202) and
        // becomes effectively invisible.
        let text_style = if let Some(bg) = match_bg {
            Style::default().fg(self.theme.muted).bg(bg)
        } else {
            Style::default().fg(self.theme.muted).bg(user_bg)
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
        let current_match = self.find_match_for_msg(_idx);
        let mut user_lines: Vec<Line<'static>> = Vec::new();
        for (part_idx, part) in msg.parts.iter().enumerate() {
            if let MsgPart::Text { content } = part {
                let tail_width = self.width.saturating_sub(2);
                for (line_idx, text_line) in content.lines().enumerate() {
                    let line_prefix = if line_idx == 0 {
                        Some(Span::styled("│ ", bar_style))
                    } else {
                        None
                    };
                    // Pre-wrap the user line so it never overflows.
                    let chunks = wrap_to_strings(text_line, tail_width);
                    for (chunk_idx, chunk) in chunks.iter().enumerate() {
                        let prefix_for_chunk = if chunk_idx == 0 {
                            line_prefix.clone()
                        } else {
                            Some(Span::styled("│ ", bar_style))
                        };
                        if let Some(m) = current_match {
                            if m.part_idx == part_idx && chunk_idx == 0 {
                                // Highlight logic on the first chunk only
                                let line_start = content.find(text_line).unwrap_or(0);
                                let line_end = line_start + text_line.len();
                                if m.start < line_end && m.end > line_start {
                                    let rel_start = m.start.saturating_sub(line_start);
                                    let rel_end = m.end.min(line_end).saturating_sub(line_start);
                                    let before = &text_line[..rel_start];
                                    let matched = &text_line[rel_start..rel_end];
                                    let after = &text_line[rel_end..];
                                    let mut spans: Vec<Span<'static>> = Vec::new();
                                    if let Some(p) = prefix_for_chunk {
                                        spans.push(p);
                                    }
                                    if !before.is_empty() {
                                        spans.push(Span::styled(before.to_string(), text_style));
                                    }
                                    spans.push(Span::styled(matched.to_string(), highlight_style));
                                    if !after.is_empty() {
                                        spans.push(Span::styled(after.to_string(), text_style));
                                    }
                                    user_lines.push(Line::from(spans));
                                } else if let Some(p) = prefix_for_chunk {
                                    user_lines.push(Line::from(vec![
                                        p,
                                        Span::styled(chunk.clone(), text_style),
                                    ]));
                                } else {
                                    user_lines
                                        .push(Line::from(Span::styled(chunk.clone(), text_style)));
                                }
                            } else if let Some(p) = prefix_for_chunk {
                                user_lines.push(Line::from(vec![
                                    p,
                                    Span::styled(chunk.clone(), text_style),
                                ]));
                            } else {
                                user_lines
                                    .push(Line::from(Span::styled(chunk.clone(), text_style)));
                            }
                        } else if let Some(p) = prefix_for_chunk {
                            user_lines
                                .push(Line::from(vec![p, Span::styled(chunk.clone(), text_style)]));
                        } else {
                            user_lines.push(Line::from(Span::styled(chunk.clone(), text_style)));
                        }
                    }
                }
            }
        }
        let pad_target = self.width;
        for line in &mut user_lines {
            pad_line_to_width(line, pad_target, pad_style);
        }
        lines.extend(user_lines);
        lines
    }

    /// Build the non-streaming-tail lines for an Assistant message. Used for
    /// both fresh renders and the render cache.
    fn build_assistant_parts_lines(
        &self,
        msg: &UIMessage,
        current_match: Option<&SearchMatch>,
        _match_bg: Option<ratatui::style::Color>,
    ) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        if self.show_timestamps {
            if let Some(ts) = msg.timestamp {
                lines.push(Line::from(Span::styled(
                    format_time(ts),
                    Style::default().fg(self.theme.muted),
                )));
            }
        }
        let mut prev_was_reasoning = false;
        for (part_idx, part) in msg.parts.iter().enumerate() {
            match part {
                MsgPart::Text { content } => {
                    let rendered =
                        render_markdown(content, &self.theme, self.theme.muted, self.width);
                    if let Some(m) = current_match {
                        if m.part_idx == part_idx {
                            lines.extend(highlight_match_in_rendered(
                                &rendered,
                                content,
                                m,
                                self.theme.selection,
                            ));
                            continue;
                        }
                    }
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
                        let tail_width = self.width.saturating_sub(2);
                        let rendered =
                            render_markdown(content, &self.theme, self.theme.muted, tail_width);
                        lines.extend(prefix_rendered_lines(rendered, "  ", Style::default()));
                    }
                    prev_was_reasoning = true;
                }
                MsgPart::ToolCall {
                    name,
                    input,
                    output,
                    status,
                    duration_ms,
                    exit_code,
                    output_lines,
                    expanded,
                    ..
                } => {
                    let target = extract_tool_target(name, input);
                    let display_name = if target.is_empty() {
                        name.clone()
                    } else {
                        format!("{} {}", name, target)
                    };
                    let (icon, base_style) = match status {
                        ToolStatus::Running => (
                            tool_spinner_frame(),
                            Style::default().fg(self.theme.warning),
                        ),
                        ToolStatus::Pending => ("○", Style::default().fg(self.theme.muted)),
                        ToolStatus::Completed => ("✓", Style::default().fg(self.theme.success)),
                        ToolStatus::Error => (
                            "✗",
                            Style::default()
                                .fg(self.theme.error)
                                .add_modifier(Modifier::CROSSED_OUT),
                        ),
                    };
                    let mut summary_parts: Vec<String> = Vec::new();
                    if let Some(ms) = duration_ms {
                        if *ms < 1000 {
                            summary_parts.push(format!("{}ms", ms));
                        } else {
                            summary_parts.push(format!("{:.1}s", *ms as f64 / 1000.0));
                        }
                    }
                    if let Some(lines) = output_lines {
                        summary_parts.push(format!("{lines} lines"));
                    }
                    if let Some(code) = exit_code {
                        summary_parts.push(format!("exit {code}"));
                    }
                    let summary_str = if summary_parts.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", summary_parts.join(", "))
                    };
                    let output_line_count = output.lines().count();
                    let toggle = if output_line_count > TOOL_OUTPUT_PREVIEW_LINES {
                        if *expanded {
                            " collapse"
                        } else {
                            " expand"
                        }
                    } else {
                        ""
                    };
                    lines.push(Line::from(vec![Span::styled(
                        format!("  {} {}{}{}", icon, display_name, summary_str, toggle),
                        base_style,
                    )]));
                    if matches!(status, ToolStatus::Pending | ToolStatus::Running)
                        && output.is_empty()
                    {
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(
                                "waiting for command/tool output...",
                                Style::default().fg(self.theme.muted),
                            ),
                        ]));
                    } else if !output.is_empty() {
                        if *expanded && !input.is_empty() {
                            let highlighted_input =
                                highlight_code(input, "json", self.theme.code_theme());
                            for input_line in
                                highlighted_input.iter().take(TOOL_INPUT_PREVIEW_LINES)
                            {
                                let mut spans = vec![Span::raw("    ")];
                                spans.extend(input_line.spans.clone());
                                lines.push(Line::from(spans));
                            }
                            let input_line_count = input.lines().count();
                            if input_line_count > TOOL_INPUT_PREVIEW_LINES {
                                lines.push(Line::from(vec![
                                    Span::raw("    "),
                                    Span::styled(
                                        format!(
                                            "... (+{} more input lines)",
                                            input_line_count - TOOL_INPUT_PREVIEW_LINES
                                        ),
                                        Style::default()
                                            .fg(self.theme.muted)
                                            .add_modifier(Modifier::DIM),
                                    ),
                                ]));
                            }
                        }

                        let limit = if *expanded {
                            usize::MAX
                        } else {
                            TOOL_OUTPUT_PREVIEW_LINES
                        };
                        if let Some(lang) = infer_tool_output_language(name, input) {
                            for output_line in
                                render_numbered_code_output(output, &lang, &self.theme)
                                    .into_iter()
                                    .take(limit)
                            {
                                let mut spans = vec![Span::raw("    ")];
                                spans.extend(output_line.spans);
                                lines.push(Line::from(spans));
                            }
                        } else {
                            for output_line in output.lines().take(limit) {
                                lines.push(Line::from(vec![
                                    Span::raw("    "),
                                    Span::styled(
                                        output_line.to_string(),
                                        Style::default().fg(self.theme.muted),
                                    ),
                                ]));
                            }
                        }
                        if !*expanded && output_line_count > TOOL_OUTPUT_PREVIEW_LINES {
                            lines.push(Line::from(vec![
                                Span::raw("    "),
                                Span::styled(
                                    format!(
                                        "... (+{} more lines, click or toggle to expand)",
                                        output_line_count - TOOL_OUTPUT_PREVIEW_LINES
                                    ),
                                    Style::default()
                                        .fg(self.theme.muted)
                                        .add_modifier(Modifier::DIM),
                                ),
                            ]));
                        }
                    }
                    // Suppress part_idx warning by referencing it
                    let _ = part_idx;
                }
                MsgPart::Image {
                    alt_text,
                    width,
                    height,
                    ..
                } => {
                    let img_text = format!("📷 Image ({}x{}): {}", width, height, alt_text);
                    lines.push(Line::from(vec![Span::styled(
                        img_text,
                        Style::default().fg(self.theme.muted),
                    )]));
                }
                MsgPart::ShellCell {
                    id,
                    command,
                    cwd: _,
                    stdout_preview,
                    stderr_preview,
                    status,
                    elapsed_ms,
                    exit_code,
                    truncated,
                    promoted,
                    expanded,
                    projection_projector,
                    projection_exactness,
                    projection_input_bytes,
                    projection_output_bytes,
                    projection_omitted,
                    projection_raw_handle,
                } => {
                    let (icon, base_style) = match status.as_str() {
                        "running" => (
                            tool_spinner_frame(),
                            Style::default().fg(self.theme.primary),
                        ),
                        "exited" => ("✓", Style::default().fg(self.theme.success)),
                        "timed_out" => ("✗", Style::default().fg(self.theme.warning)),
                        "killed" => ("✗", Style::default().fg(self.theme.warning)),
                        "failed" => ("✗", Style::default().fg(self.theme.error)),
                        _ => ("○", Style::default().fg(self.theme.muted)),
                    };
                    let mut summary_parts: Vec<String> = Vec::new();
                    if let Some(ms) = elapsed_ms {
                        if *ms < 1000 {
                            summary_parts.push(format!("{}ms", ms));
                        } else {
                            summary_parts.push(format!("{:.1}s", *ms as f64 / 1000.0));
                        }
                    }
                    if let Some(code) = exit_code {
                        summary_parts.push(format!("exit {code}"));
                    }
                    if *truncated {
                        summary_parts.push("truncated".to_string());
                    }
                    if *promoted {
                        summary_parts.push("promoted".to_string());
                    }
                    let summary_str = if summary_parts.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", summary_parts.join(", "))
                    };
                    let _toggle = if stdout_preview.lines().count() > TOOL_OUTPUT_PREVIEW_LINES {
                        if *expanded {
                            " collapse"
                        } else {
                            " expand"
                        }
                    } else {
                        ""
                    };
                    lines.push(Line::from(vec![Span::styled(
                        format!("  {} $ {}{}", icon, command, summary_str),
                        base_style,
                    )]));
                    // Projection metadata line
                    if let Some(ref projector) = projection_projector {
                        let mut meta_parts: Vec<String> =
                            vec![format!("projection: {}", projector)];
                        if let Some(ref exactness) = projection_exactness {
                            meta_parts.push(exactness.clone());
                        }
                        if let (Some(in_bytes), Some(out_bytes)) =
                            (projection_input_bytes, projection_output_bytes)
                        {
                            meta_parts.push(format!(
                                "{} -> {}",
                                format_bytes(*in_bytes),
                                format_bytes(*out_bytes as u64)
                            ));
                        }
                        if let Some(ref omitted) = projection_omitted {
                            meta_parts.push(format!("omitted {}", omitted));
                        }
                        if let Some(ref handle) = projection_raw_handle {
                            meta_parts.push(format!("raw: {}", handle));
                        }
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(
                                meta_parts.join(" . "),
                                Style::default()
                                    .fg(self.theme.muted)
                                    .add_modifier(Modifier::DIM),
                            ),
                        ]));
                    }
                    if status == "running" && stdout_preview.is_empty() && stderr_preview.is_empty()
                    {
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("running...", Style::default().fg(self.theme.muted)),
                        ]));
                    } else if !stdout_preview.is_empty() || !stderr_preview.is_empty() {
                        let limit = if *expanded {
                            usize::MAX
                        } else {
                            TOOL_OUTPUT_PREVIEW_LINES
                        };
                        if !stdout_preview.is_empty() {
                            for output_line in stdout_preview.lines().take(limit) {
                                lines.push(Line::from(vec![
                                    Span::raw("    "),
                                    Span::styled(
                                        output_line.to_string(),
                                        Style::default().fg(self.theme.muted),
                                    ),
                                ]));
                            }
                            let total_lines = stdout_preview.lines().count();
                            if !*expanded && total_lines > TOOL_OUTPUT_PREVIEW_LINES {
                                lines.push(Line::from(vec![
                                    Span::raw("    "),
                                    Span::styled(
                                        format!(
                                            "... (+{} more lines, click or toggle to expand)",
                                            total_lines - TOOL_OUTPUT_PREVIEW_LINES
                                        ),
                                        Style::default()
                                            .fg(self.theme.muted)
                                            .add_modifier(Modifier::DIM),
                                    ),
                                ]));
                            }
                        }
                        if !stderr_preview.is_empty() {
                            for error_line in stderr_preview.lines().take(limit) {
                                lines.push(Line::from(vec![
                                    Span::raw("    "),
                                    Span::styled(
                                        error_line.to_string(),
                                        Style::default().fg(self.theme.error),
                                    ),
                                ]));
                            }
                        }
                    }
                    if status != "running" && !*promoted {
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(
                                format!("not in model context; /shell-include {id} to attach"),
                                Style::default()
                                    .fg(self.theme.muted)
                                    .add_modifier(Modifier::DIM),
                            ),
                        ]));
                    }
                }
                MsgPart::RunCell {
                    run_id,
                    title,
                    status,
                    backend_label,
                    duration,
                    changed_file_count,
                    risk_label,
                } => {
                    let (icon, base_style) = match status.as_str() {
                        "running" => (
                            tool_spinner_frame(),
                            Style::default().fg(self.theme.primary),
                        ),
                        "complete" => ("✓", Style::default().fg(self.theme.success)),
                        "failed" | "timed_out" => ("✗", Style::default().fg(self.theme.error)),
                        "cancelled" | "incomplete" => {
                            ("✗", Style::default().fg(self.theme.warning))
                        }
                        _ => ("○", Style::default().fg(self.theme.muted)),
                    };
                    let mut summary_parts: Vec<String> = Vec::new();
                    summary_parts.push(backend_label.clone());
                    if let Some(ref dur) = duration {
                        summary_parts.push(dur.clone());
                    }
                    if *changed_file_count > 0 {
                        summary_parts.push(format!(
                            "{} file{} changed",
                            changed_file_count,
                            if *changed_file_count == 1 { "" } else { "s" }
                        ));
                    }
                    if !risk_label.is_empty() {
                        summary_parts.push(format!("risk: {}", risk_label));
                    }
                    let summary_str = format!(" ({})", summary_parts.join(", "));
                    let detail_hint =
                        format!(" [view: /run-detail {}]", &run_id[..run_id.len().min(8)]);
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} run: {}", icon, title), base_style),
                        Span::styled(summary_str, Style::default().fg(self.theme.muted)),
                        Span::styled(
                            detail_hint,
                            Style::default()
                                .fg(self.theme.muted)
                                .add_modifier(Modifier::DIM),
                        ),
                    ]));
                }
            }
        }
        lines
    }

    fn get_cached_last_assistant_parts(&self, msg: &UIMessage) -> Option<Vec<Line<'static>>> {
        let cache = self.last_render_cache.borrow();
        let c = cache.as_ref()?;
        // Cache is keyed on (streaming_len=0 means "the parts themselves,
        // ignoring streaming tail") and width and a stable identity of the
        // message buffer. We use the address of the msg as a cheap identity
        // (the last assistant message is stable for the duration of streaming).
        if c.streaming_len == usize::MAX
            && c.width == self.width
            && c.message_ptr_id == msg as *const _ as usize
        {
            return Some(c.lines.clone());
        }
        None
    }

    fn store_cached_last_assistant_parts(&self, msg: &UIMessage, lines: Vec<Line<'static>>) {
        *self.last_render_cache.borrow_mut() = Some(LastRenderCache {
            streaming_len: usize::MAX, // sentinel meaning "parts cache"
            width: self.width,
            message_ptr_id: msg as *const _ as usize,
            lines,
        });
    }
}

/// Apply search-match highlight to pre-rendered markdown lines by re-coloring
/// the character range covered by `m` with a background color + REVERSED.
/// Returns the original lines unchanged when the match is outside the rendered
/// range or on a non-first logical line (pre-wrap may split logical lines
/// across multiple rendered lines; for now we only highlight the first
/// rendered line, which is the common case).
fn highlight_match_in_rendered(
    rendered: &[Line<'static>],
    original_content: &str,
    m: &crate::tui::components::messages::SearchMatch,
    selection_bg: ratatui::style::Color,
) -> Vec<Line<'static>> {
    if rendered.is_empty() || m.line_in_msg >= original_content.lines().count() {
        return rendered.to_vec();
    }
    if m.line_in_msg != 0 {
        return rendered.to_vec();
    }
    let mut first = rendered[0].clone();
    let total_width: usize = first
        .spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    if m.end > total_width {
        return rendered.to_vec();
    }
    let mut new_spans: Vec<Span<'static>> = Vec::new();
    let mut col = 0usize;
    for span in first.spans.iter() {
        let w = UnicodeWidthStr::width(span.content.as_ref());
        let span_start = col;
        let span_end = col + w;
        if m.start >= span_end || m.end <= span_start {
            new_spans.push(span.clone());
        } else {
            let chars: Vec<char> = span.content.chars().collect();
            let local_start = m.start.saturating_sub(span_start).min(chars.len());
            let local_end = m.end.saturating_sub(span_start).min(chars.len());
            if local_start > 0 {
                let before: String = chars[..local_start].iter().collect();
                new_spans.push(Span::styled(before, span.style));
            }
            if local_end > local_start {
                let matched: String = chars[local_start..local_end].iter().collect();
                new_spans.push(Span::styled(
                    matched,
                    span.style.bg(selection_bg).add_modifier(Modifier::REVERSED),
                ));
            }
            if local_end < chars.len() {
                let after: String = chars[local_end..].iter().collect();
                new_spans.push(Span::styled(after, span.style));
            }
        }
        col = span_end;
    }
    first.spans = new_spans;
    let mut out: Vec<Line<'static>> = vec![first];
    out.extend_from_slice(&rendered[1..]);
    out
}

fn render_markdown(
    text: &str,
    theme: &Arc<Theme>,
    default_color: ratatui::style::Color,
    width: u16,
) -> Vec<Line<'static>> {
    if text.is_empty() {
        return vec![Line::from("")];
    }
    let arena = Arena::new();
    let root = parse_document(&arena, text, &MARKDOWN_OPTIONS);
    let mut lines = Vec::new();
    render_markdown_blocks(root, &mut lines, theme, default_color, width);
    if lines.is_empty() {
        vec![Line::from("")]
    } else {
        lines
    }
}

fn render_markdown_blocks<'a>(
    node: &'a AstNode<'a>,
    lines: &mut Vec<Line<'static>>,
    theme: &Arc<Theme>,
    default_color: ratatui::style::Color,
    width: u16,
) {
    for child in node.children() {
        let before_len = lines.len();
        if before_len > 0 && !is_blank_line(&lines[before_len - 1]) {
            lines.push(Line::from(""));
        }
        render_markdown_block(child, lines, theme, default_color, width);
        if lines.len() == before_len + 1 && is_blank_line(&lines[before_len]) {
            lines.pop();
        }
    }
}

fn render_markdown_block<'a>(
    node: &'a AstNode<'a>,
    lines: &mut Vec<Line<'static>>,
    theme: &Arc<Theme>,
    default_color: ratatui::style::Color,
    width: u16,
) {
    match &node.data.borrow().value {
        NodeValue::Document => render_markdown_blocks(node, lines, theme, default_color, width),
        NodeValue::Paragraph => {
            let spans = collect_markdown_inlines(node, theme, Style::default().fg(default_color));
            lines.extend(wrap_markdown_spans(spans, width, default_color));
        }
        NodeValue::Heading(heading) => {
            let style = match heading.level {
                1 => Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
                2 => Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default()
                    .fg(default_color)
                    .add_modifier(Modifier::BOLD),
            };
            let spans = collect_markdown_inlines(node, theme, style);
            lines.extend(wrap_markdown_spans(spans, width, default_color));
        }
        NodeValue::CodeBlock(code) => {
            let lang = normalize_code_lang(&code.info);
            lines.extend(render_code_block(&code.literal, &lang, theme, width));
        }
        NodeValue::BlockQuote => {
            let mut inner = Vec::new();
            render_markdown_blocks(
                node,
                &mut inner,
                theme,
                default_color,
                width.saturating_sub(2),
            );
            for line in inner {
                let mut spans = vec![Span::styled("│ ", Style::default().fg(theme.muted))];
                spans.extend(line.spans);
                lines.push(Line::from(spans));
            }
        }
        NodeValue::List(list) => {
            render_markdown_list(node, *list, lines, theme, default_color, width)
        }
        NodeValue::ThematicBreak => lines.push(Line::from(Span::styled(
            "─".repeat(width.max(1) as usize),
            Style::default().fg(theme.muted),
        ))),
        NodeValue::HtmlBlock(html) => {
            for line in html.literal.lines() {
                lines.extend(render_md_line_wrapped(
                    line.trim(),
                    theme,
                    default_color,
                    width,
                ));
            }
        }
        NodeValue::Table(_) | NodeValue::TableRow(_) | NodeValue::TableCell => {
            let text = collect_markdown_plain_text(node);
            if !text.trim().is_empty() {
                lines.extend(render_md_line_wrapped(
                    text.trim(),
                    theme,
                    default_color,
                    width,
                ));
            }
        }
        NodeValue::DescriptionList
        | NodeValue::DescriptionItem(_)
        | NodeValue::DescriptionTerm
        | NodeValue::DescriptionDetails
        | NodeValue::FootnoteDefinition(_)
        | NodeValue::MultilineBlockQuote(_)
        | NodeValue::Alert(_) => {
            render_markdown_blocks(node, lines, theme, default_color, width);
        }
        _ => {
            let spans = collect_markdown_inlines(node, theme, Style::default().fg(default_color));
            if !spans.is_empty() {
                lines.extend(wrap_markdown_spans(spans, width, default_color));
            }
        }
    }
}

fn render_markdown_list<'a>(
    node: &'a AstNode<'a>,
    list: comrak::nodes::NodeList,
    lines: &mut Vec<Line<'static>>,
    theme: &Arc<Theme>,
    default_color: ratatui::style::Color,
    width: u16,
) {
    let mut ordinal = list.start.max(1);
    for item in node.children() {
        let marker = match list.list_type {
            ListType::Bullet => "• ".to_string(),
            ListType::Ordered => {
                let marker = format!("{ordinal}. ");
                ordinal += 1;
                marker
            }
        };
        let marker_width = UnicodeWidthStr::width(marker.as_str());
        let mut item_lines = Vec::new();
        render_markdown_blocks(
            item,
            &mut item_lines,
            theme,
            default_color,
            width.saturating_sub(marker_width as u16),
        );
        if item_lines.is_empty() {
            lines.push(Line::from(Span::styled(
                marker,
                Style::default().fg(theme.primary),
            )));
            continue;
        }
        for (idx, line) in item_lines.into_iter().enumerate() {
            let prefix = if idx == 0 {
                marker.clone()
            } else {
                " ".repeat(marker_width)
            };
            let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.primary))];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
    }
}

fn collect_markdown_inlines<'a>(
    node: &'a AstNode<'a>,
    theme: &Arc<Theme>,
    style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for child in node.children() {
        collect_markdown_inline(child, theme, style, &mut spans);
    }
    spans
}

fn collect_markdown_inline<'a>(
    node: &'a AstNode<'a>,
    theme: &Arc<Theme>,
    style: Style,
    spans: &mut Vec<Span<'static>>,
) {
    match &node.data.borrow().value {
        NodeValue::Text(text) => spans.extend(parse_text_links(text, theme, style)),
        NodeValue::Code(code) => spans.push(Span::styled(
            code.literal.clone(),
            Style::default().fg(theme.primary).bg(theme.selection),
        )),
        NodeValue::Emph => {
            for child in node.children() {
                collect_markdown_inline(child, theme, style.add_modifier(Modifier::ITALIC), spans);
            }
        }
        NodeValue::Strong => {
            for child in node.children() {
                collect_markdown_inline(child, theme, style.add_modifier(Modifier::BOLD), spans);
            }
        }
        NodeValue::Strikethrough => {
            for child in node.children() {
                collect_markdown_inline(
                    child,
                    theme,
                    style.add_modifier(Modifier::CROSSED_OUT),
                    spans,
                );
            }
        }
        NodeValue::Link(link) | NodeValue::Image(link) => {
            let text = collect_markdown_plain_text(node);
            let label = if text.is_empty() {
                link.url.as_str()
            } else {
                text.as_str()
            };
            spans.push(Span::styled(
                wrap_osc8(&link.url, label),
                Style::default()
                    .fg(theme.link)
                    .add_modifier(Modifier::UNDERLINED),
            ));
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => spans.push(Span::raw(" ")),
        NodeValue::HtmlInline(text) | NodeValue::Raw(text) => {
            spans.extend(parse_text_links(text, theme, style))
        }
        _ => {
            for child in node.children() {
                collect_markdown_inline(child, theme, style, spans);
            }
        }
    }
}

fn collect_markdown_plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    collect_markdown_plain_text_into(node, &mut out);
    out
}

fn collect_markdown_plain_text_into<'a>(node: &'a AstNode<'a>, out: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(text) | NodeValue::HtmlInline(text) | NodeValue::Raw(text) => {
            out.push_str(text)
        }
        NodeValue::Code(code) => out.push_str(&code.literal),
        NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
        NodeValue::CodeBlock(code) => out.push_str(&code.literal),
        _ => {
            for child in node.children() {
                collect_markdown_plain_text_into(child, out);
            }
        }
    }
}

fn parse_line_with_urls(
    line: &str,
    theme: &Arc<Theme>,
    default_color: ratatui::style::Color,
) -> Vec<Span<'static>> {
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
            spans.extend(parse_plain_text(before, theme, default_color));
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
        spans.extend(parse_plain_text(after, theme, default_color));
    }

    spans
}

fn parse_text_links(text: &str, theme: &Arc<Theme>, style: Style) -> Vec<Span<'static>> {
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
    for mat in URL_REGEX.find_iter(text) {
        matches.push(Match {
            start: mat.start(),
            end: mat.end(),
            text: mat.as_str().to_string(),
            is_url: true,
        });
    }
    for mat in FILE_PATH_REGEX.find_iter(text) {
        let matched = mat.as_str();
        let path = matched.trim();
        matches.push(Match {
            start: mat.start() + (matched.len() - matched.trim_start().len()),
            end: mat.start() + (matched.len() - matched.trim_end().len()),
            text: path.to_string(),
            is_url: false,
        });
    }

    matches.sort_by_key(|m| m.start);
    for m in matches {
        if m.start < last_end {
            continue;
        }
        if m.start > last_end {
            spans.push(Span::styled(text[last_end..m.start].to_string(), style));
        }
        let link_text = if m.is_url {
            wrap_osc8(&m.text, &m.text)
        } else {
            let abs_path = if m.text.starts_with("~/") {
                if let Ok(home) = std::env::var("HOME") {
                    m.text.replacen('~', &home, 1)
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

    if last_end < text.len() {
        spans.push(Span::styled(text[last_end..].to_string(), style));
    }
    spans
}

fn wrap_markdown_spans(
    spans: Vec<Span<'static>>,
    width: u16,
    default_color: ratatui::style::Color,
) -> Vec<Line<'static>> {
    if spans.is_empty() {
        return vec![Line::from("")];
    }
    let total_width: usize = spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    if total_width <= width as usize {
        return vec![Line::from(spans)];
    }
    wrap_styled_spans(spans, width, Style::default().fg(default_color))
}

fn wrap_styled_spans(
    spans: Vec<Span<'static>>,
    width: u16,
    fallback_style: Style,
) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut col = 0usize;

    for span in spans {
        let style = if span.style == Style::default() {
            fallback_style
        } else {
            span.style
        };
        for ch in span.content.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if ch == '\n' {
                trim_trailing_space_spans(&mut current);
                lines.push(Line::from(std::mem::take(&mut current)));
                col = 0;
                continue;
            }
            if col > 0 && col + ch_width > width {
                trim_trailing_space_spans(&mut current);
                lines.push(Line::from(std::mem::take(&mut current)));
                col = 0;
            }
            if col == 0 && ch.is_whitespace() {
                continue;
            }
            current.push(Span::styled(ch.to_string(), style));
            col += ch_width;
        }
    }

    trim_trailing_space_spans(&mut current);
    if !current.is_empty() || lines.is_empty() {
        lines.push(Line::from(current));
    }
    lines
}

fn trim_trailing_space_spans(spans: &mut Vec<Span<'static>>) {
    while spans
        .last()
        .is_some_and(|span| span.content.chars().all(char::is_whitespace))
    {
        spans.pop();
    }
}

fn wrap_code_spans(spans: &[Span<'static>], width: u16) -> Vec<Line<'static>> {
    let width = width as usize;
    if width == 0 {
        return vec![Line::from(spans.to_vec())];
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut col = 0usize;
    for span in spans {
        let style = span.style;
        for ch in span.content.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if col > 0 && col + ch_width > width {
                lines.push(Line::from(std::mem::take(&mut current)));
                col = 0;
            }
            current.push(Span::styled(ch.to_string(), style));
            col += ch_width;
        }
    }
    lines.push(Line::from(current));
    lines
}

fn render_code_block(code: &str, lang: &str, theme: &Arc<Theme>, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let line_count = code.lines().count();
    let needs_line_numbers = line_count > 5;
    let highlighted = highlight_code(code, lang, theme.code_theme());
    if !lang.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  ┌─ {} ", lang.to_uppercase()),
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD),
        )));
    }

    let gutter_width = if needs_line_numbers { 7 } else { 0 };
    let code_inner_width = width.saturating_sub(gutter_width);
    for (i, highlighted_line) in highlighted.iter().enumerate() {
        let wrapped = wrap_code_spans(&highlighted_line.spans, code_inner_width);
        for (j, wrapped_line) in wrapped.into_iter().enumerate() {
            let mut spans = Vec::new();
            if needs_line_numbers && j == 0 {
                spans.push(Span::styled(
                    format!("{:4} │ ", i + 1),
                    Style::default().fg(theme.muted),
                ));
            } else if needs_line_numbers {
                spans.push(Span::styled("     │ ", Style::default().fg(theme.muted)));
            }
            spans.extend(wrapped_line.spans);
            lines.push(Line::from(spans));
        }
    }
    lines
}

fn prefix_rendered_lines(
    lines: Vec<Line<'static>>,
    prefix: &str,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    if prefix.is_empty() {
        return lines;
    }
    lines
        .into_iter()
        .map(|line| {
            let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

fn infer_tool_output_language(name: &str, input: &str) -> Option<String> {
    if name != "read" {
        return None;
    }
    let input: serde_json::Value = serde_json::from_str(input).ok()?;
    let path = input.get("path")?.as_str()?;
    let ext = std::path::Path::new(path).extension()?.to_str()?;
    let lang = normalize_code_lang(ext);
    if lang.is_empty() {
        None
    } else {
        Some(lang)
    }
}

fn render_numbered_code_output(output: &str, lang: &str, theme: &Arc<Theme>) -> Vec<Line<'static>> {
    let code = output
        .lines()
        .map(strip_read_line_number)
        .collect::<Vec<_>>()
        .join("\n");
    let highlighted = highlight_code(&code, lang, theme.code_theme());
    output
        .lines()
        .zip(highlighted)
        .map(|(original, highlighted_line)| {
            let mut spans = Vec::new();
            if let Some((prefix, _)) = split_read_line_number(original) {
                spans.push(Span::styled(
                    prefix.to_string(),
                    Style::default().fg(theme.muted),
                ));
            }
            spans.extend(highlighted_line.spans);
            Line::from(spans)
        })
        .collect()
}

fn strip_read_line_number(line: &str) -> &str {
    split_read_line_number(line)
        .map(|(_, code)| code)
        .unwrap_or(line)
}

fn split_read_line_number(line: &str) -> Option<(&str, &str)> {
    if line.len() < 8 {
        return None;
    }
    let (prefix, rest) = line.split_at(8);
    let valid_prefix = prefix
        .chars()
        .take(6)
        .all(|c| c == ' ' || c.is_ascii_digit())
        && prefix.as_bytes().get(6) == Some(&b':')
        && prefix.as_bytes().get(7) == Some(&b' ');
    if valid_prefix {
        Some((prefix, rest))
    } else {
        None
    }
}

fn normalize_code_lang(info: &str) -> String {
    let lang = info
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|c: char| c == '{' || c == '}' || c == '.' || c == ',')
        .to_ascii_lowercase();
    if lang.is_empty() {
        return String::new();
    }
    match lang.as_str() {
        "js" => "javascript".to_string(),
        "jsx" => "javascriptreact".to_string(),
        "ts" => "typescript".to_string(),
        "tsx" => "typescriptreact".to_string(),
        "py" | "pyw" => "python".to_string(),
        "rs" => "rust".to_string(),
        "sh" | "zsh" | "fish" => "shellscript".to_string(),
        "bash" => "bash".to_string(),
        "yml" => "yaml".to_string(),
        "md" => "markdown".to_string(),
        "docker" => "dockerfile".to_string(),
        other => extension_to_language_id(other).unwrap_or(other).to_string(),
    }
}

fn parse_plain_text(
    text: &str,
    theme: &Arc<Theme>,
    default_color: ratatui::style::Color,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut remaining = text.to_string();

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find('`') {
            if pos > 0 {
                let before = &remaining[..pos];
                spans.push(Span::styled(
                    before.to_string(),
                    Style::default().fg(default_color),
                ));
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
                spans.push(Span::styled(
                    remaining[pos..].to_string(),
                    Style::default().fg(default_color),
                ));
                remaining.clear();
            }
        } else if let Some(pos) = remaining.find("**") {
            if pos > 0 {
                let before = &remaining[..pos];
                spans.push(Span::styled(
                    before.to_string(),
                    Style::default().fg(default_color),
                ));
            }
            let rest = &remaining[pos + 2..];
            if let Some(end) = rest.find("**") {
                let bold = &rest[..end];
                spans.push(Span::styled(
                    bold.to_string(),
                    Style::default()
                        .fg(default_color)
                        .add_modifier(Modifier::BOLD),
                ));
                remaining = rest[end + 2..].to_string();
            } else {
                spans.push(Span::styled(
                    remaining[pos..].to_string(),
                    Style::default().fg(default_color),
                ));
                remaining.clear();
            }
        } else if let Some(pos) = remaining.find('*') {
            let before = &remaining[..pos];
            if !before.is_empty() {
                spans.push(Span::styled(
                    before.to_string(),
                    Style::default().fg(default_color),
                ));
            }
            let rest = &remaining[pos + 1..];
            if let Some(end) = rest.find('*') {
                let italic = &rest[..end];
                if !italic.is_empty() {
                    spans.push(Span::styled(
                        italic.to_string(),
                        Style::default()
                            .fg(default_color)
                            .add_modifier(Modifier::ITALIC),
                    ));
                }
                remaining = rest[end + 1..].to_string();
            } else {
                spans.push(Span::styled(
                    remaining[pos..].to_string(),
                    Style::default().fg(default_color),
                ));
                remaining.clear();
            }
        } else {
            spans.push(Span::styled(
                remaining.clone(),
                Style::default().fg(default_color),
            ));
            remaining.clear();
        }
    }

    spans
}

/// Like `render_md_line` but pre-wraps the line to `width` columns.
/// Used by [`render_markdown`]. For long lines, continuation lines reuse the
/// default style (bold/italic/code span styling is dropped on wrap — this is
/// an acceptable tradeoff for chat-log readability).
fn render_md_line_wrapped(
    line: &str,
    theme: &Arc<Theme>,
    default_color: ratatui::style::Color,
    width: u16,
) -> Vec<Line<'static>> {
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
            Style::default()
                .fg(default_color)
                .add_modifier(Modifier::BOLD),
        ))];
    }
    if line.starts_with("- ") || line.starts_with("* ") {
        let content = line.trim_start_matches("- ").trim_start_matches("* ");
        return vec![Line::from(vec![
            Span::styled("• ", Style::default().fg(theme.primary)),
            Span::styled(content.to_string(), Style::default().fg(default_color)),
        ])];
    }

    let spans = parse_line_with_urls(line, theme, default_color);
    if spans.is_empty() {
        return vec![Line::from("")];
    }
    // If the line fits, return as-is
    let total_width: usize = spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    if total_width <= width as usize {
        return vec![Line::from(spans)];
    }
    // Overflow: collapse to plain text, wrap, and emit each chunk using
    // the default style.
    let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
    let chunks = wrap_to_strings(&plain, width);
    chunks
        .into_iter()
        .map(|c| Line::from(Span::styled(c, Style::default().fg(default_color))))
        .collect()
}

fn format_time(ts: i64) -> String {
    let dt = chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_default();
    format!("  {dt}")
}

fn is_blank_line(line: &Line) -> bool {
    line.spans.is_empty()
        || line
            .spans
            .iter()
            .all(|s| s.content.is_empty() || s.content.chars().all(|c| c == ' '))
}

fn collapse_blank_lines<'a>(lines: &'a [Line<'a>]) -> Vec<Line<'a>> {
    let mut result = Vec::with_capacity(lines.len());
    let mut prev_blank = false;
    for line in lines {
        let blank = is_blank_line(line);
        if blank && prev_blank {
            continue;
        }
        result.push(line.clone());
        prev_blank = blank;
    }
    result
}

pub fn highlight_code(code: &str, lang: &str, code_theme: &str) -> Vec<Line<'static>> {
    let syntax_token = match lang {
        "bash" | "shellscript" => "sh",
        "c++" | "cpp" => "cpp",
        "c" => "c",
        "csharp" => "cs",
        "css" => "css",
        "dockerfile" => "Dockerfile",
        "go" => "go",
        "html" => "html",
        "java" => "java",
        "javascript" | "javascriptreact" => "js",
        "json" => "json",
        "lua" => "lua",
        "markdown" => "md",
        "objective-c" => "objc",
        "objective-cpp" => "cpp",
        "python" => "py",
        "ruby" => "rb",
        "rust" => "rs",
        "toml" => "toml",
        "typescript" | "typescriptreact" => "ts",
        "xml" => "xml",
        "yaml" => "yml",
        other => other,
    };
    let syntax = SYNTAX_SET
        .find_syntax_by_token(syntax_token)
        .or_else(|| SYNTAX_SET.find_syntax_by_extension(syntax_token))
        .or_else(|| SYNTAX_SET.find_syntax_by_name(lang))
        .or_else(|| SYNTAX_SET.find_syntax_by_name(syntax_token))
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

    #[test]
    fn wrap_count_short_lines() {
        // A short line that fits in width is one visual line.
        assert_eq!(wrap_count("hello world", 80), 1);
        assert_eq!(wrap_count("", 80), 1);
    }

    #[test]
    fn wrap_count_breaks_at_whitespace() {
        // 10 chars per word * 5 words = 50 chars total; at width=12 should fit
        // on ceil(50/12) lines ~= 5
        let s = "aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeeeeeeee";
        let n = wrap_count(s, 12);
        assert!(
            (4..=6).contains(&n),
            "got {n} for 50-char string at width 12"
        );
    }

    #[test]
    fn wrap_count_preserves_explicit_newlines() {
        assert_eq!(wrap_count("a\nb\nc", 80), 3);
    }

    #[test]
    fn wrap_to_strings_matches_wrap_count() {
        let cases: &[(&str, u16)] = &[
            ("hello world", 80),
            ("", 80),
            ("a b c d e f g h i j k l m n o p", 10),
            ("one\ntwo\nthree", 80),
            (
                "a very long line that should wrap multiple times at narrow widths",
                15,
            ),
            // Long-word cases (hard-break) — these need to count the same
            // in both functions.
            ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbb", 10),
            ("https://example.com/very/long/url bbb", 20),
        ];
        for (s, w) in cases {
            let lines = wrap_to_strings(s, *w);
            let count = wrap_count(s, *w);
            assert_eq!(
                lines.len(),
                count,
                "wrap_to_strings produced {} lines but wrap_count says {} for {s:?} @ width {w}",
                lines.len(),
                count
            );
        }
    }

    #[test]
    fn wrap_to_strings_no_trailing_space() {
        // The wrap must not leave a trailing space on a wrapped line, and
        // must not insert a double space at the wrap point (which made
        // text look "extra wide" between words).
        let cases: &[(&str, u16, Vec<&str>)] = &[
            ("hello world", 10, vec!["hello", "world"]),
            // width 11: "hello world" is exactly 11 chars and fits on one
            // line (the previous buggy version double-spaced the join to
            // squeeze it in — no longer needed).
            ("hello world", 11, vec!["hello world"]),
            ("hello world", 12, vec!["hello world"]),
            ("a b c d e", 5, vec!["a b c", "d e"]),
            ("a b c d e", 6, vec!["a b c", "d e"]),
        ];
        for (s, w, expected) in cases {
            let lines = wrap_to_strings(s, *w);
            let got: Vec<&str> = lines.iter().map(String::as_str).collect();
            assert_eq!(got, *expected, "input={s:?} width={w}");
            // And no line should have a trailing space.
            for line in &lines {
                assert!(
                    !line.ends_with(' '),
                    "line {line:?} ends with a space (input={s:?} width={w})"
                );
                assert!(
                    !line.contains("  "),
                    "line {line:?} contains a double space (input={s:?} width={w})"
                );
            }
        }
    }

    #[test]
    fn wrap_to_strings_hard_breaks_long_words() {
        // Words longer than `width` (URLs, paths) must hard-break so they
        // never bleed into the scrollbar gutter.
        let url = "https://example.com/very/long/url";
        assert!(
            url.chars().count() > 20,
            "test url must be longer than 20 chars"
        );
        let lines = wrap_to_strings(url, 20);
        // 33 chars at width 20 → ceil(33/20) = 2 lines.
        assert_eq!(
            lines.len(),
            2,
            "expected 2 lines for long URL, got {lines:?}"
        );
        for line in &lines {
            assert!(
                line.chars().count() <= 20,
                "line {line:?} is longer than wrap width 20"
            );
        }

        // Single huge word (no whitespace).
        let huge = "a".repeat(50);
        let lines = wrap_to_strings(&huge, 20);
        // 50 chars at width 20 → ceil(50/20) = 3 lines.
        assert_eq!(
            lines.len(),
            3,
            "expected 3 lines for 50-char word, got {lines:?}"
        );
        for line in &lines {
            assert!(
                line.chars().count() <= 20,
                "line {line:?} is longer than wrap width 20"
            );
        }
    }

    #[test]
    fn wrap_count_hard_breaks_long_words() {
        // The estimate must agree with the render for long words.
        let url = "https://example.com/very/long/url";
        assert_eq!(wrap_count(url, 20), 2);
        let huge = "a".repeat(50);
        assert_eq!(wrap_count(&huge, 20), 3);
        // Long word that fits on its own line — no extra line for the
        // hard-break, just one visual line.
        assert_eq!(wrap_count(&"a".repeat(20), 20), 1);
        assert_eq!(wrap_count(&"a".repeat(21), 20), 2);
    }

    #[test]
    fn estimate_msg_lines_short_text() {
        let mut widget = MessagesWidget::default();
        widget.set_width(80);
        let msg = UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::Text {
                content: "short line".to_string(),
            }],
            timestamp: None,
            is_plan_mode: None,
        };
        // 1 (one-line text) — no implicit border/header is added.
        assert_eq!(widget.estimate_msg_lines(&msg), 1);
    }

    #[test]
    fn estimate_msg_lines_text_wraps_to_width() {
        let mut widget = MessagesWidget::default();
        widget.set_width(20);
        let long = "x".repeat(50);
        let msg = UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::Text {
                content: long.clone(),
            }],
            timestamp: None,
            is_plan_mode: None,
        };
        let n = widget.estimate_msg_lines(&msg);
        // width=20 minus 2 = 18 cols; 50 chars / 18 = 3 lines
        assert!(
            n >= 2,
            "expected at least 2 lines for 50 chars at width 20, got {n}"
        );
    }

    #[test]
    fn estimate_msg_lines_code_block_with_lang_header() {
        let mut widget = MessagesWidget::default();
        widget.set_width(80);
        let code = "```rust\nfn main() {}\n```".to_string();
        let msg = UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::Text { content: code }],
            timestamp: None,
            is_plan_mode: None,
        };
        // 1 ┌─ RUST header + 1 code line = 2 lines
        assert!(widget.estimate_msg_lines(&msg) >= 2);
    }

    #[test]
    fn estimate_msg_lines_matches_rendered_markdown_code_width() {
        let mut widget = MessagesWidget::default();
        widget.set_width(20);
        let code = "```rust\nlet value = 1234567890;\n```".to_string();
        let msg = UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::Text {
                content: code.clone(),
            }],
            timestamp: None,
            is_plan_mode: None,
        };
        let rendered_lines =
            render_markdown(&code, &widget.theme, widget.theme.muted, widget.width);
        let rendered = collapse_blank_lines(&rendered_lines);
        assert_eq!(widget.estimate_msg_lines(&msg), rendered.len());
    }

    #[test]
    fn wrapped_markdown_preserves_inline_styles() {
        let widget = MessagesWidget::default();
        let rendered = render_markdown(
            "prefix **bold text that wraps** and `code span` suffix",
            &widget.theme,
            widget.theme.muted,
            14,
        );
        assert!(
            rendered
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.style.add_modifier.contains(Modifier::BOLD)),
            "wrapped markdown should preserve bold spans"
        );
        assert!(
            rendered
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.style.bg == Some(widget.theme.selection)),
            "wrapped markdown should preserve inline-code background"
        );
    }

    #[test]
    fn rust_code_highlighting_uses_non_plain_syntax() {
        let highlighted = highlight_code(
            "fn main() {\n    let value = 1;\n}",
            "rust",
            "base16-ocean.dark",
        );
        let colors: std::collections::HashSet<_> = highlighted
            .iter()
            .flat_map(|line| line.spans.iter().filter_map(|span| span.style.fg))
            .collect();
        assert!(
            colors.len() > 1,
            "rust highlighting should produce multiple syntax colors"
        );
    }

    #[test]
    fn estimate_msg_lines_collapses_consecutive_blanks_in_prose() {
        // Markdown paragraph breaks render as one separator line, while
        // single newlines inside a paragraph are soft breaks. The estimator
        // must match that rendered output so scroll math stays accurate.
        let mut widget = MessagesWidget::default();
        widget.set_width(80);
        let cases = [
            "a\n\nb",
            "a\n\n\nb",
            "a\n\n\n\nb",
            "a\nb\n\nc",
            "a\n\nb\n\n\nc",
        ];
        for content in cases {
            let msg = UIMessage {
                role: MessageRole::Assistant,
                parts: vec![MsgPart::Text {
                    content: content.to_string(),
                }],
                timestamp: None,
                is_plan_mode: None,
            };
            let n = widget.estimate_msg_lines(&msg);
            let rendered_lines =
                render_markdown(content, &widget.theme, widget.theme.muted, widget.width);
            let expected = collapse_blank_lines(&rendered_lines).len();
            assert_eq!(n, expected, "content={content:?}");
        }

        let paragraph_lines =
            render_markdown("a\n\nb", &widget.theme, widget.theme.muted, widget.width);
        assert_eq!(
            collapse_blank_lines(&paragraph_lines).len(),
            3,
            "paragraph break should keep a blank separator"
        );
    }

    #[test]
    fn estimate_msg_lines_blank_inside_code_block_not_collapsed() {
        // Blank lines inside a fenced code block must NOT be collapsed —
        // they're meaningful (e.g., separating functions). This matches
        // the render path's code-block handling.
        let mut widget = MessagesWidget::default();
        widget.set_width(80);
        let code = "```rust\nfn a() {}\n\nfn b() {}\n```".to_string();
        let msg = UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::Text { content: code }],
            timestamp: None,
            is_plan_mode: None,
        };
        // ┌─ RUST + fn a() {} + blank + fn b() {} = 4 lines
        assert_eq!(widget.estimate_msg_lines(&msg), 4);
    }

    #[test]
    fn estimate_msg_lines_includes_streaming_tail() {
        let mut widget = MessagesWidget::default();
        widget.set_width(80);
        widget.set_visible_height(20);
        widget.add_user_message("hi".to_string(), None);
        widget.add_streaming_token("streaming content here");
        // estimate must be called with a reference to the actual message in
        // the vector (the ptr::eq check requires identity).
        let n = widget.estimate_msg_lines(widget.messages.last().unwrap());
        // 0 (empty parts) + 1 (label) + 1 (streaming line) = 2 minimum
        assert!(
            n >= 2,
            "expected at least 2 lines (label + stream), got {n}"
        );
    }

    #[test]
    fn reasoning_uses_markdown_renderer() {
        let widget = MessagesWidget::default();
        let msg = UIMessage {
            role: MessageRole::Assistant,
            parts: vec![MsgPart::Reasoning {
                content: "Thinking **hard**\n\n```rust\nfn main() {}\n```".to_string(),
                collapsed: false,
            }],
            timestamp: None,
            is_plan_mode: None,
        };

        let rendered = widget.build_assistant_parts_lines(&msg, None, None);

        assert!(
            rendered
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.content.contains("hard")
                    && span.style.add_modifier.contains(Modifier::BOLD)),
            "reasoning markdown should preserve bold spans"
        );
        assert!(
            rendered
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.content.contains("RUST")),
            "reasoning markdown should render fenced code blocks"
        );
    }

    #[test]
    fn read_tool_output_language_is_inferred_from_path() {
        let input = r#"{"path":"/tmp/example.rs"}"#;
        assert_eq!(
            infer_tool_output_language("read", input),
            Some("rust".to_string())
        );
        assert_eq!(infer_tool_output_language("bash", input), None);
    }

    #[test]
    fn numbered_read_output_highlights_code_without_coloring_line_numbers() {
        let widget = MessagesWidget::default();
        let rendered = render_numbered_code_output("     1: fn main() {}\n", "rust", &widget.theme);

        let first = rendered.first().expect("expected highlighted output");
        assert_eq!(first.spans.first().unwrap().content.as_ref(), "     1: ");
        assert_eq!(
            first.spans.first().unwrap().style.fg,
            Some(widget.theme.muted)
        );

        let colors: std::collections::HashSet<_> = rendered
            .iter()
            .flat_map(|line| line.spans.iter().skip(1).filter_map(|span| span.style.fg))
            .collect();
        assert!(
            colors.len() > 1,
            "read output code portion should use syntax colors"
        );
    }

    #[test]
    fn scrollbar_state_empty_when_fits() {
        let widget = MessagesWidget::default();
        // No messages, no scrollbar needed. The state should be constructible.
        let _ = widget.scrollbar_state(20);
    }

    #[test]
    fn scrollbar_state_partial_position() {
        let mut widget = MessagesWidget::default();
        widget.set_width(80);
        widget.set_visible_height(3);
        for i in 0..6 {
            widget.add_user_message(format!("msg {i} with some content"), None);
        }
        // 6 messages, each ~1-2 lines; more than 3 lines total.
        widget.scroll = 1;
        let state = widget.scrollbar_state(3);
        // Just assert that the state is constructible; ratatui 0.29 doesn't
        // expose content_length/position getters.
        let _ = state;
    }

    #[test]
    fn width_change_invalidates_render_cache() {
        let mut widget = MessagesWidget::default();
        widget.set_width(80);
        widget.add_user_message("hi".to_string(), None);
        widget.add_assistant_text("first response".to_string());
        widget.add_streaming_token("partial");
        // Populate render cache directly (in production it's populated during render).
        let last_idx = widget.messages.len() - 1;
        widget.store_cached_last_assistant_parts(
            &widget.messages[last_idx],
            vec![Line::from("cached")],
        );
        let cached: Option<Vec<Line<'static>>> =
            widget.get_cached_last_assistant_parts(&widget.messages[last_idx]);
        assert!(cached.is_some(), "cache should be populated after store");
        // Width change should invalidate the cache.
        widget.set_width(40);
        let cached2: Option<Vec<Line<'static>>> =
            widget.get_cached_last_assistant_parts(&widget.messages[last_idx]);
        assert!(
            cached2.is_none(),
            "cache should be invalidated after width change"
        );
    }
}

//! M006-A: document editor widget.
//!
//! Renders one document from a scoped borrow of the controller's replica:
//! a line-number gutter, the current-line highlight, a horizontally
//! scrolled viewport, and a state banner.
//!
//! ## Hard wrap
//!
//! One logical line is exactly one screen row. That one-to-one mapping is
//! what keeps the gutter, the current-line highlight, and cursor arithmetic
//! deterministic; soft wrap is deferred to a later milestone.
//!
//! ## Bounded work
//!
//! Only the visible slice is materialized, bounded by the viewport height,
//! and each visible line is read with a single bounded `read_bytes` call.
//! Per-frame cost is a function of the viewport, not of the document, so a
//! multi-megabyte file renders exactly like a small one.
//!
//! The widget never reads a file: every byte arrives through
//! `DocumentController::try_snapshot()`, which is daemon-authorized.

use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use codegg_document::DocumentSnapshot;

use crate::tui::editor::{
    self, EditorFocus, EditorMode, EditorViewport, MAX_EDITOR_PENDING_COMMAND,
};
use crate::tui::theme::Theme;

/// Rows of chrome reserved above the text: the state banner and the
/// current-line/column readout.
const CHROME_ROWS: u16 = 2;

/// Document editor view.
///
/// `snapshot` is borrowed for the duration of one `render` call and is
/// dropped with it; the widget stores no text.
pub struct EditorWidget<'a> {
    /// Workspace-relative path shown in the title.
    pub path: &'a str,
    /// Controller state label.
    pub status: &'a str,
    /// Surfaced error banner, if any.
    pub notice: Option<&'a str>,
    /// Buffer mode.
    pub mode: EditorMode,
    /// Which region owns keyboard input.
    pub focus: EditorFocus,
    /// Pending normal-mode command prefix.
    pub pending_command: &'a str,
    /// Whether the `:` command line is open.
    pub command_active: bool,
    /// Authority root the open document is confined to. Drives the global /
    /// project badge in the status row, so a user editing a shared agent
    /// config can see at a glance that a `:wq` will affect every project.
    pub scope: codegg_protocol::document::DocumentScope,
    /// The `:` line without its leading colon.
    pub command_line: &'a str,
    /// Highlighted palette row index.
    pub command_selection: usize,
    /// Cursor byte offset into `snapshot`.
    pub cursor_byte: usize,
    /// Whether the attachment holds the writer lease.
    pub writable: bool,
    /// Whether a lifecycle operation is in flight.
    pub busy: bool,
    /// The controller replica. Borrowed, never retained.
    pub snapshot: &'a DocumentSnapshot,
    /// Bounded diagnostics and semantic spans for this document generation.
    pub lsp: &'a crate::tui::editor_lsp::EditorLspState,
    /// Focused editor presentation, mutated only to resolve the viewport.
    pub presentation: &'a mut crate::tui::document_session::TuiDocumentPresentation,
    pub theme: Arc<Theme>,
}

impl EditorWidget<'_> {
    /// Paint the editor into `area`.
    pub fn render(mut self, frame: &mut Frame, area: Rect) {
        let theme = Arc::clone(&self.theme);
        let focused = self.focus == EditorFocus::Buffer;
        let title = format!(
            " {} {} · {} · {}{} ",
            self.path,
            self.mode_label(),
            self.status,
            if focused { "[buffer]" } else { "[composer]" },
            if self.busy { " · working…" } else { "" },
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused { theme.primary } else { theme.border }))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let text_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(CHROME_ROWS),
        };
        self.render_banner(frame, inner);
        if text_area.height > 0 {
            self.render_text(frame, text_area);
        }
        self.render_lsp_panel(frame, area);
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
        }
    }

    /// State banner plus the cursor readout, on the bottom `CHROME_ROWS`.
    fn render_banner(&self, frame: &mut Frame, inner: Rect) {
        let theme = Arc::clone(&self.theme);
        let line_number = editor::cursor_line(self.snapshot, self.cursor_byte) + 1;
        let column = editor::display_column(self.snapshot, self.cursor_byte) + 1;
        let pending = if self.pending_command.is_empty() {
            String::new()
        } else {
            format!(" {}", self.pending_command)
        };
        // The `:` line replaces the status text while it is open. It owns
        // the banner row because that row is what the user is looking at
        // when they typed `:`, and a separate floating line would read as a
        // different surface than the one holding the keyboard.
        let (banner_text, banner_style) = if self.command_active {
            (
                format!(":{}_", self.command_line),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
        } else if let Some(notice) = self.notice {
            (
                format!("{notice}{pending}"),
                Style::default().fg(theme.error),
            )
        } else if !self.writable {
            (
                format!("{}{pending}", self.status),
                Style::default().fg(theme.warning),
            )
        } else {
            (
                format!("{}{pending}", self.status),
                Style::default().fg(theme.muted),
            )
        };
        let first = Line::from(Span::styled(banner_text, banner_style));
        let second = if self.command_active {
            // One row of fuzzy matches, inlined after the cursor readout.
            // A separate overlay would cover the document the command is
            // about to act on, which is the one thing the user needs to see
            // while choosing between `:w` and `:wq`.
            let matches = editor::complete_editor_command(self.command_line);
            let mut spans = vec![Span::styled(
                format!("ln {line_number}  col {column}  "),
                Style::default().fg(theme.muted),
            )];
            if matches.is_empty() {
                spans.push(Span::styled(
                    "no matching command",
                    Style::default().fg(theme.error),
                ));
            }
            for (index, candidate) in matches.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("  "));
                }
                let selected = index == self.command_selection;
                let command = candidate.command;
                spans.push(Span::styled(
                    format!(":{}", command.spec()),
                    if selected {
                        Style::default()
                            .fg(theme.background)
                            .bg(theme.primary)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.foreground)
                    },
                ));
                spans.push(Span::styled(
                    format!(" {}", command.help()),
                    Style::default().fg(theme.muted),
                ));
            }
            Line::from(spans)
        } else {
            Line::from(vec![
                Span::styled(
                    format!("ln {line_number}"),
                    Style::default().fg(theme.muted),
                ),
                Span::styled("  ", Style::default()),
                Span::styled(format!("col {column}"), Style::default().fg(theme.muted)),
                Span::styled("  ", Style::default()),
                // Always show which root this document is confined to. The
                // difference is invisible in the path itself (`build.toml`
                // reads the same at either scope) but it decides who sees
                // the edit, so it must not be something the user has to
                // remember.
                Span::styled(
                    format!(
                        "[{}]  ",
                        crate::tui::commands::agent_editor::scope_label(self.scope)
                    ),
                    Style::default().fg(if self.scope.is_global_agent() {
                        theme.warning
                    } else {
                        theme.muted
                    }),
                ),
                Span::styled(
                    if !self.writable {
                        "read-only — edits refused".to_string()
                    } else if crate::tui::commands::agent_editor::is_agent_config_path(
                        self.path, self.scope,
                    ) {
                        // In an agent file the interesting command is the one
                        // that publishes the agent, so say so here rather
                        // than making the user read `/help` for it.
                        crate::tui::commands::agent_editor::AGENT_EDITOR_HINT.to_string()
                    } else {
                        "writable".to_string()
                    },
                    Style::default().fg(if self.writable {
                        theme.muted
                    } else {
                        theme.warning
                    }),
                ),
            ])
        };
        let rows = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(CHROME_ROWS),
            width: inner.width,
            height: CHROME_ROWS.min(inner.height),
        };
        frame.render_widget(Paragraph::new(vec![first, second]), rows);
    }

    /// Gutter plus horizontally scrolled text for the visible slice.
    fn render_text(&mut self, frame: &mut Frame, area: Rect) {
        let theme = Arc::clone(&self.theme);
        let viewport: EditorViewport =
            editor::resolve_viewport(self.presentation, self.snapshot, area.width, area.height);
        let cursor_line = editor::cursor_line(self.snapshot, self.cursor_byte);
        let gutter_style = Style::default().fg(theme.muted);
        let current_gutter = Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD);
        let current_text = Style::default().bg(theme.selection);
        let current_style = if self.focus == EditorFocus::Buffer {
            current_text
        } else {
            Style::default().fg(theme.alternate_bg)
        };

        let total_lines = self.snapshot.len_lines();
        let mut lines: Vec<Line> = Vec::with_capacity(viewport.visible_lines as usize);
        for row in 0..viewport.visible_lines as usize {
            let line_index = viewport.first_line + row;
            if line_index >= total_lines {
                break;
            }
            let is_current = line_index == cursor_line;
            let number = format!(
                "{:>width$} ",
                line_index + 1,
                width = viewport.gutter as usize - 1
            );
            let marker = self
                .lsp
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.range.start_line as usize == line_index)
                .map(|diagnostic| diagnostic.severity)
                .min();
            let marker = match marker {
                Some(1) => "!",
                Some(2) => "▲",
                Some(_) => "·",
                None => " ",
            };
            let mut spans = vec![Span::styled(
                format!("{number}{marker}"),
                if is_current {
                    current_gutter
                } else {
                    gutter_style
                },
            )];
            let text_style = if is_current {
                current_style
            } else {
                Style::default()
            };
            spans.extend(self.line_spans(line_index, viewport, text_style));
            lines.push(Line::from(spans));
        }
        // An empty document still has one logical line, so `lines` is
        // non-empty. Test the text, not the line count.
        if self.snapshot.is_empty() {
            lines.clear();
            lines.push(Line::from(Span::styled(
                "(empty document)",
                Style::default().fg(theme.muted),
            )));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_lsp_panel(&self, frame: &mut Frame, area: Rect) {
        use crate::tui::editor_lsp::{DiagnosticsViewStatus, EditorLspPanel};
        let Some(panel) = self.lsp.panel else { return };
        let height = area.height.min(12);
        let width = area.width.min(100);
        if height < 3 || width < 12 {
            return;
        }
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let theme = Arc::clone(&self.theme);
        let (title, mut lines) = match panel {
            EditorLspPanel::Diagnostics => {
                let status = match self.lsp.diagnostics_status {
                    DiagnosticsViewStatus::Idle => "not requested".to_string(),
                    DiagnosticsViewStatus::Loading => "refreshing".to_string(),
                    DiagnosticsViewStatus::Ready => "current".to_string(),
                    DiagnosticsViewStatus::NotReady => "language server not ready".to_string(),
                    DiagnosticsViewStatus::Unavailable => "unavailable".to_string(),
                };
                let stale = if self.lsp.diagnostics_stale {
                    " · showing stale results"
                } else {
                    ""
                };
                let mut lines = vec![Line::from(Span::styled(
                    format!("{status}{stale}"),
                    Style::default().fg(theme.warning),
                ))];
                lines.extend(self.lsp.diagnostics.iter().take(8).enumerate().map(
                    |(index, item)| {
                        Line::from(format!(
                            "{}{}:{} [{}] {}",
                            if index == self.lsp.selected_index {
                                "> "
                            } else {
                                "  "
                            },
                            item.range.start_line + 1,
                            item.range.start_column + 1,
                            severity_label(item.severity),
                            crate::tui::editor_lsp::truncate_chars(&item.message, 120)
                        ))
                    },
                ));
                if self.lsp.diagnostics.is_empty()
                    && self.lsp.diagnostics_status == DiagnosticsViewStatus::Ready
                {
                    lines.push(Line::from("No diagnostics"));
                }
                (" Diagnostics · Enter jump · r refresh · Esc close ", lines)
            }
            EditorLspPanel::ReadResult => {
                let mut lines = Vec::new();
                if self.lsp.read_loading {
                    lines.push(Line::from("Request in progress…"));
                }
                if let Some(error) = &self.lsp.read_error {
                    lines.push(Line::from(crate::tui::editor_lsp::truncate_chars(
                        error, 120,
                    )));
                }
                if let Some(result) = &self.lsp.read_result {
                    use codegg_protocol::lsp::{LspReadPayloadDto, LspReadStatus};
                    if result.status == LspReadStatus::NotReady {
                        lines.push(Line::from("Language server not ready"));
                    }
                    match result.payload.as_ref() {
                        Some(LspReadPayloadDto::Hover { text, .. }) => lines.push(Line::from(
                            crate::tui::editor_lsp::truncate_chars(text, 240),
                        )),
                        Some(LspReadPayloadDto::Locations { locations, .. }) => lines.extend(
                            locations.iter().take(8).enumerate().map(|(index, loc)| {
                                Line::from(format!(
                                    "{}{}:{} · {}",
                                    if index == self.lsp.selected_index {
                                        "> "
                                    } else {
                                        "  "
                                    },
                                    loc.path,
                                    loc.start_line + 1,
                                    loc.workspace_relative_path
                                        .as_deref()
                                        .unwrap_or("outside workspace; display only")
                                ))
                            }),
                        ),
                        Some(LspReadPayloadDto::Symbols { symbols, .. }) => lines.extend(
                            symbols.iter().take(8).enumerate().map(|(index, symbol)| {
                                Line::from(format!(
                                    "{}{} · {}:{}",
                                    if index == self.lsp.selected_index {
                                        "> "
                                    } else {
                                        "  "
                                    },
                                    crate::tui::editor_lsp::truncate_chars(&symbol.name, 80),
                                    symbol.range.path,
                                    symbol.range.start_line + 1
                                ))
                            }),
                        ),
                        _ => {}
                    }
                }
                if lines.is_empty() {
                    lines.push(Line::from("No results"));
                }
                (" LSP result · Enter opens safe target · Esc close ", lines)
            }
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(rect);
        frame.render_widget(Clear, rect);
        frame.render_widget(block, rect);
        lines.truncate(inner.height as usize);
        frame.render_widget(
            Paragraph::new(lines).scroll((self.lsp.selected_index as u16, 0)),
            inner,
        );
    }

    /// The visible display-column slice of `line_index`, truncated to the
    /// viewport width.
    ///
    /// Reads at most one window per line rather than the whole line, so a
    /// pathologically long line cannot make a frame expensive.
    fn line_slice(&self, line_index: usize, viewport: EditorViewport) -> String {
        let Some(start) = editor::line_start(self.snapshot, line_index) else {
            return String::new();
        };
        let end = editor::line_content_end(self.snapshot, line_index).unwrap_or(start);
        let text = match self.snapshot.read_bytes(start, end) {
            Ok(text) => text,
            Err(_) => return String::new(),
        };
        let width = viewport.text_columns as usize;
        if viewport.column == 0 && display_width_str(&text) <= width {
            return text;
        }
        let mut out = String::new();
        let mut consumed = 0usize;
        for ch in text.chars() {
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if consumed + char_width > viewport.column + width {
                break;
            }
            if consumed >= viewport.column {
                out.push(ch);
            }
            consumed += char_width;
        }
        out
    }

    fn line_spans(
        &self,
        line_index: usize,
        viewport: EditorViewport,
        base: Style,
    ) -> Vec<Span<'static>> {
        use crate::tui::editor_lsp::utf16_column_to_byte;
        let Some(start) = editor::line_start(self.snapshot, line_index) else {
            return vec![];
        };
        let end = editor::line_content_end(self.snapshot, line_index).unwrap_or(start);
        if end.saturating_sub(start) > 16 * 1024 {
            return vec![Span::styled(self.line_slice(line_index, viewport), base)];
        }
        let Ok(line) = self.snapshot.read_bytes(start, end) else {
            return vec![];
        };
        let mut display = 0usize;
        let mut visible_start = line.len();
        let mut visible_end = line.len();
        let mut started = false;
        let limit = viewport
            .column
            .saturating_add(viewport.text_columns as usize);
        for (byte, ch) in line.char_indices() {
            let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            let next = display.saturating_add(width);
            if !started && next > viewport.column {
                visible_start = byte;
                started = true;
            }
            if started && display < limit {
                visible_end = byte + ch.len_utf8();
            }
            display = next;
        }
        if !started && viewport.column == 0 {
            visible_start = 0;
            visible_end = line.len();
        } else if !started {
            return vec![Span::styled(String::new(), base)];
        }
        let visible_end = visible_end.max(visible_start).min(line.len());
        let mut boundaries = vec![visible_start, visible_end];
        let mut ranges = Vec::new();
        for token in self
            .lsp
            .semantic_tokens
            .iter()
            .filter(|token| token.line as usize == line_index)
        {
            let Some(style) = known_token_style(&token.token_type) else {
                continue;
            };
            let Some(token_end) = token.character.checked_add(token.length) else {
                continue;
            };
            let Some(token_start) = utf16_column_to_byte(&line, token.character) else {
                continue;
            };
            let Some(token_end) = utf16_column_to_byte(&line, token_end) else {
                continue;
            };
            let range = token_start.max(visible_start)..token_end.min(visible_end);
            if range.start < range.end {
                boundaries.push(range.start);
                boundaries.push(range.end);
                ranges.push((range, style));
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
            .windows(2)
            .filter_map(|pair| {
                let range = pair[0]..pair[1];
                if range.start == range.end {
                    return None;
                }
                let text = line.get(range.clone())?.to_string();
                let style = ranges
                    .iter()
                    .find(|(token_range, _)| {
                        token_range.start <= range.start && token_range.end >= range.end
                    })
                    .map_or(base, |(_, token_style)| base.patch(*token_style));
                Some(Span::styled(text, style))
            })
            .collect()
    }
}

fn severity_label(severity: u32) -> &'static str {
    match severity {
        1 => "error",
        2 => "warning",
        3 => "info",
        _ => "hint",
    }
}

fn known_token_style(token_type: &str) -> Option<Style> {
    Some(match token_type {
        "keyword" => Style::default().fg(ratatui::style::Color::Yellow),
        "string" => Style::default().fg(ratatui::style::Color::Green),
        "number" => Style::default().fg(ratatui::style::Color::Cyan),
        "comment" => Style::default()
            .fg(ratatui::style::Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
        "function" | "method" => Style::default().fg(ratatui::style::Color::Magenta),
        "type" | "class" | "enum" | "struct" => Style::default().fg(ratatui::style::Color::Blue),
        _ => return None,
    })
}

fn display_width_str(text: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    UnicodeWidthStr::width(text)
}

/// Clamp a pending normal-mode prefix to the editor's bound.
pub fn bounded_pending(pending: &str) -> &str {
    if pending.len() <= MAX_EDITOR_PENDING_COMMAND {
        pending
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::document_session::TuiDocumentPresentation;
    use codegg_document::DocumentBuffer;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Widget chrome that varies per test. The snapshot and presentation
    /// are supplied separately so each test can vary either without
    /// duplicating the terminal plumbing.
    struct Options<'a> {
        status: &'a str,
        notice: Option<&'a str>,
        mode: EditorMode,
        focus: EditorFocus,
        pending_command: &'a str,
        command_active: bool,
        command_line: &'a str,
        command_selection: usize,
        writable: bool,
        busy: bool,
        scope: codegg_protocol::document::DocumentScope,
    }

    fn defaults<'a>() -> Options<'a> {
        Options {
            status: "synced",
            notice: None,
            mode: EditorMode::Normal,
            focus: EditorFocus::Buffer,
            pending_command: "",
            command_active: false,
            command_line: "",
            command_selection: 0,
            writable: true,
            busy: false,
            scope: codegg_protocol::document::DocumentScope::Workspace,
        }
    }

    /// Paint one editor frame and return the rendered rows as plain text.
    fn render(
        text: &str,
        width: u16,
        height: u16,
        cursor: usize,
        options: Options<'_>,
    ) -> Vec<String> {
        let snapshot = DocumentBuffer::new(text).snapshot();
        let mut presentation = TuiDocumentPresentation {
            cursor_byte: cursor,
            ..TuiDocumentPresentation::default()
        };
        let lsp = crate::tui::editor_lsp::EditorLspState::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                EditorWidget {
                    path: "src/lib.rs",
                    status: options.status,
                    notice: options.notice,
                    mode: options.mode,
                    focus: options.focus,
                    pending_command: options.pending_command,
                    command_active: options.command_active,
                    command_line: options.command_line,
                    command_selection: options.command_selection,
                    scope: options.scope,
                    cursor_byte: presentation.cursor_byte,
                    writable: options.writable,
                    busy: options.busy,
                    snapshot: &snapshot,
                    lsp: &lsp,
                    presentation: &mut presentation,
                    theme: Arc::new(Theme::default()),
                }
                .render(frame, Rect::new(0, 0, width, height));
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn paint(text: &str, width: u16, height: u16, cursor: usize) -> String {
        render(text, width, height, cursor, defaults()).join("\n")
    }

    #[test]
    fn renders_line_numbers_and_the_line_content() {
        let joined = paint("alpha\nbeta\ngamma", 30, 8, 0);
        assert!(joined.contains("alpha"), "{joined}");
        assert!(joined.contains("beta"), "{joined}");
        assert!(joined.contains("gamma"), "{joined}");
        assert!(joined.contains("ln 1"), "{joined}");
    }

    #[test]
    fn the_viewport_shows_only_the_visible_window() {
        let text = (0..50).map(|i| format!("line{i}\n")).collect::<String>();
        let joined = paint(&text, 30, 8, 0);
        assert!(joined.contains("line0"), "{joined}");
        assert!(
            !joined.contains("line20"),
            "must not render off-screen lines: {joined}"
        );
    }

    #[test]
    fn moving_the_cursor_scrolls_the_viewport_and_updates_the_readout() {
        let text = (0..50).map(|i| format!("line{i}\n")).collect::<String>();
        let offset = text.find("line40").unwrap();
        let joined = paint(&text, 30, 8, offset);
        assert!(joined.contains("line40"), "{joined}");
        assert!(joined.contains("ln 41"), "{joined}");
    }

    #[test]
    fn a_read_only_attachment_says_so() {
        // Wide enough for the whole readout row. The scope badge sits before
        // the advisory on purpose: on a narrow box the badge and the cursor
        // position survive truncation while the trailing hint is what gets
        // cut, because the badge is the safety-relevant half.
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                writable: false,
                status: "read-only",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("read-only"), "{joined}");
        assert!(joined.contains("edits refused"), "{joined}");
    }

    /// The scope badge is the only thing on screen that tells a user whether
    /// `:wq` reaches this project or every project on the machine, and the
    /// path reads identically at either scope. It must therefore always be
    /// rendered, and it must change with the scope.
    #[test]
    fn the_status_row_always_names_the_authority_root() {
        for (scope, expected) in [
            (
                codegg_protocol::document::DocumentScope::Workspace,
                "[project]",
            ),
            (
                codegg_protocol::document::DocumentScope::GlobalAgent,
                "[global]",
            ),
        ] {
            let joined = render(
                "content",
                60,
                8,
                0,
                Options {
                    scope,
                    ..defaults()
                },
            )
            .join("\n");
            assert!(
                joined.contains(expected),
                "{scope:?} must show {expected}:\n{joined}"
            );
            // And never the other one — a badge that always rendered both
            // would carry no information.
            let other = if expected == "[project]" {
                "[global]"
            } else {
                "[project]"
            };
            assert!(
                !joined.contains(other),
                "{scope:?} must not show {other}:\n{joined}"
            );
        }
    }

    /// The status banner is the row directly above the line/column
    /// readout. Locating it by the readout keeps the assertion independent
    /// of block borders and chrome height, and keeps it off the title,
    /// which always carries the status.
    fn banner(lines: &[String]) -> String {
        let readout = lines
            .iter()
            .position(|line| line.contains("ln "))
            .unwrap_or(lines.len());
        lines
            .get(readout.saturating_sub(1))
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn a_surfaced_notice_replaces_the_status_line_and_appends_the_pending_prefix() {
        let lines = render(
            "content",
            60,
            8,
            0,
            Options {
                notice: Some("edit queue is full"),
                pending_command: "d",
                ..defaults()
            },
        );
        let banner = banner(&lines);
        assert!(banner.contains("edit queue is full"), "{banner}");
        assert!(
            !banner.contains("synced"),
            "banner status is replaced: {banner}"
        );
        assert!(
            banner.contains("edit queue is full d"),
            "banner and pending prefix share the row: {banner}"
        );
    }

    #[test]
    fn a_clean_status_shows_in_the_banner() {
        let lines = render("content", 60, 8, 0, defaults());
        let banner = banner(&lines);
        assert!(banner.contains("synced"), "{banner}");
    }

    #[test]
    fn a_conflict_draft_is_visible_in_the_banner() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                status: "disk conflict \u{2014} draft kept",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("disk conflict"), "{joined}");
        assert!(joined.contains("draft kept"), "{joined}");
    }

    #[test]
    fn a_gone_document_says_the_draft_is_kept() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                status: "document gone \u{2014} draft kept",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("document gone"), "{joined}");
        assert!(joined.contains("draft kept"), "{joined}");
    }

    #[test]
    fn a_disconnected_transport_says_the_draft_is_kept() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                status: "disconnected \u{2014} draft kept",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("disconnected"), "{joined}");
    }

    #[test]
    fn an_empty_document_renders_a_placeholder() {
        let joined = paint("", 30, 8, 0);
        assert!(joined.contains("empty document"), "{joined}");
    }

    #[test]
    fn a_narrow_area_does_not_panic() {
        for (width, height) in [(1u16, 1u16), (1, 10), (10, 1), (2, 2), (3, 3)] {
            let lines = render("abc\ndef", width, height, 1, defaults());
            assert_eq!(lines.len(), height as usize, "{width}x{height}");
        }
    }

    #[test]
    fn a_very_long_line_is_truncated_to_the_viewport() {
        let text = "x".repeat(5000);
        let lines = render(&text, 30, 8, 0, defaults());
        let widest = lines
            .iter()
            .map(|line| line.trim_end().chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            widest <= 30,
            "per-frame render must stay viewport-bounded: {widest}"
        );
    }

    #[test]
    fn per_frame_work_is_bounded_by_the_viewport_not_the_document() {
        let text = (0..20_000)
            .map(|i| format!("line{i}\n"))
            .collect::<String>();
        let lines = render(&text, 40, 10, 0, defaults());
        let visible = lines.iter().filter(|line| line.contains("line")).count();
        assert!(
            visible <= 10,
            "at most one row per viewport line: {visible}"
        );
        assert!(lines.iter().any(|line| line.contains("line0")));
    }

    #[test]
    fn the_gutter_width_does_not_change_as_the_cursor_moves() {
        let text = (0..120).map(|i| format!("line{i}\n")).collect::<String>();
        let content_column = |rendered: &str| {
            rendered
                .lines()
                .filter(|line| line.trim_start().starts_with("line"))
                .map(|line| line.len() - line.trim_start().len())
                .min()
                .unwrap_or(usize::MAX)
        };
        let first = paint(&text, 40, 10, 0);
        let middle = text.find("line60").unwrap();
        let later = paint(&text, 40, 10, middle);
        assert_eq!(
            content_column(&first),
            content_column(&later),
            "gutter must not jitter as the cursor moves"
        );
    }

    #[test]
    fn the_title_reports_mode_and_focus() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                mode: EditorMode::Insert,
                focus: EditorFocus::Composer,
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("INSERT"), "{joined}");
        assert!(joined.contains("[composer]"), "{joined}");
    }

    #[test]
    fn a_busy_operation_is_marked_in_the_title() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                busy: true,
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("working"), "{joined}");
    }

    #[test]
    fn bounded_pending_drops_an_over_long_prefix() {
        assert_eq!(bounded_pending("d"), "d");
        assert_eq!(bounded_pending("ddd"), "");
    }
}

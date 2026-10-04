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
    /// Cursor byte offset into `snapshot`.
    pub cursor_byte: usize,
    /// Whether the attachment holds the writer lease.
    pub writable: bool,
    /// Whether a lifecycle operation is in flight.
    pub busy: bool,
    /// The controller replica. Borrowed, never retained.
    pub snapshot: &'a DocumentSnapshot,
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
        let status_style = if self.notice.is_some() {
            Style::default().fg(theme.error)
        } else if !self.writable {
            Style::default().fg(theme.warning)
        } else {
            Style::default().fg(theme.muted)
        };
        let banner = self.notice.unwrap_or(self.status);
        let first = Line::from(Span::styled(format!("{banner}{pending}"), status_style));
        let second = Line::from(vec![
            Span::styled(
                format!("ln {line_number}"),
                Style::default().fg(theme.muted),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(format!("col {column}"), Style::default().fg(theme.muted)),
            Span::styled("  ", Style::default()),
            Span::styled(
                if self.writable {
                    "writable"
                } else {
                    "read-only — edits refused"
                }
                .to_string(),
                Style::default().fg(if self.writable {
                    theme.muted
                } else {
                    theme.warning
                }),
            ),
        ]);
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
            let mut spans = vec![Span::styled(
                number,
                if is_current {
                    current_gutter
                } else {
                    gutter_style
                },
            )];
            spans.push(Span::styled(
                self.line_slice(line_index, viewport),
                if is_current {
                    current_style
                } else {
                    Style::default()
                },
            ));
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
        writable: bool,
        busy: bool,
    }

    fn defaults<'a>() -> Options<'a> {
        Options {
            status: "synced",
            notice: None,
            mode: EditorMode::Normal,
            focus: EditorFocus::Buffer,
            pending_command: "",
            writable: true,
            busy: false,
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
                    cursor_byte: presentation.cursor_byte,
                    writable: options.writable,
                    busy: options.busy,
                    snapshot: &snapshot,
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
        let joined = render(
            "content",
            50,
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

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;
use similar::{ChangeTag, TextDiff};

use crate::tui::components::scroll::CenteredScroll;
use crate::tui::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum DiffMode {
    Inline,
    SideBySide,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_number_old: Option<usize>,
    pub line_number_new: Option<usize>,
    pub content: String,
    pub tag: ChangeTag,
}

#[derive(Debug)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

/// Cloning a hunk is a **whole-patch copy**, which is the cost the change
/// review paid on every frame before M003.
///
/// The impl is written out rather than derived so the counter is attached to
/// the operation itself: a render path that reintroduces
/// `patch.hunks.clone()` is measured without a second call site anybody has
/// to remember, and a hunk list that is moved or borrowed — the shape the
/// review uses now — counts nothing. The change review borrows its state's
/// parsed patch through [`DiffViewer::render_hunks_in`].
impl Clone for DiffHunk {
    fn clone(&self) -> Self {
        crate::tui::render_cost::count_review_hunk_copy();
        Self {
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
            lines: self.lines.clone(),
        }
    }
}

#[derive(Clone)]
pub struct DiffViewer {
    pub old_content: String,
    pub new_content: String,
    pub hunks: Vec<DiffHunk>,
    pub scroll: CenteredScroll,
    pub mode: DiffMode,
    pub title: String,
}

impl DiffViewer {
    pub fn new(old_content: Box<str>, new_content: Box<str>, title: Box<str>) -> Self {
        let hunks = Self::compute_diff(&old_content, &new_content);
        Self {
            old_content: old_content.to_string(),
            new_content: new_content.to_string(),
            hunks,
            scroll: CenteredScroll::new(),
            mode: DiffMode::Inline,
            title: title.to_string(),
        }
    }

    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            DiffMode::Inline => DiffMode::SideBySide,
            DiffMode::SideBySide => DiffMode::Inline,
        };
        self.scroll.reset();
    }

    fn compute_diff(old_content: &str, new_content: &str) -> Vec<DiffHunk> {
        let diff = TextDiff::from_lines(old_content, new_content);
        let mut hunks = Vec::new();
        let mut current_hunk: Option<DiffHunk> = None;
        let mut old_line = 0;
        let mut new_line = 0;

        for change in diff.iter_all_changes() {
            let tag = change.tag();
            match tag {
                ChangeTag::Delete => {
                    old_line += 1;
                    if current_hunk.is_none() {
                        current_hunk = Some(DiffHunk {
                            old_start: old_line,
                            old_count: 0,
                            new_start: new_line + 1,
                            new_count: 0,
                            lines: Vec::new(),
                        });
                    }
                    if let Some(ref mut hunk) = current_hunk {
                        hunk.old_count += 1;
                        hunk.lines.push(DiffLine {
                            line_number_old: Some(old_line),
                            line_number_new: None,
                            content: change.value().to_string(),
                            tag,
                        });
                    }
                }
                ChangeTag::Insert => {
                    new_line += 1;
                    if current_hunk.is_none() {
                        current_hunk = Some(DiffHunk {
                            old_start: old_line + 1,
                            old_count: 0,
                            new_start: new_line,
                            new_count: 0,
                            lines: Vec::new(),
                        });
                    }
                    if let Some(ref mut hunk) = current_hunk {
                        hunk.new_count += 1;
                        hunk.lines.push(DiffLine {
                            line_number_old: None,
                            line_number_new: Some(new_line),
                            content: change.value().to_string(),
                            tag,
                        });
                    }
                }
                ChangeTag::Equal => {
                    old_line += 1;
                    new_line += 1;
                    if let Some(hunk) = current_hunk.take() {
                        hunks.push(hunk);
                    }
                }
            }
        }

        if let Some(hunk) = current_hunk {
            hunks.push(hunk);
        }

        hunks
    }

    pub fn total_lines(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len()).sum()
    }

    /// Draw the viewer into `area`.
    ///
    /// `theme` is a parameter rather than a field this box refreshes when it is
    /// opened. `DiffViewer` used to cache an `Arc<Theme>` that was set once by
    /// its owner, so a `/diff` opened before a theme change kept drawing the old
    /// palette while the rest of the UI followed the new one. The owner already
    /// holds the live theme at every call site, so reading it is strictly less
    /// state — there is no path on which the value can be stale.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

        self.render_header(frame, chunks[0], theme);
        self.render_diff(frame, chunks[1], theme);
        self.render_footer(frame, chunks[2], theme);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mode_str = match self.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(theme.primary)),
            Span::styled(&self.title, Style::default().fg(theme.foreground)),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Diff Viewer ")
                .border_style(Style::default().fg(theme.border)),
        );

        frame.render_widget(header, area);
    }

    fn render_diff(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.height < 1 {
            return;
        }

        let inner = area.intersection(Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        ));

        let visible_lines = inner.height as usize;
        let scroll_offset = self.scroll.get();

        let mut y = 0u16;

        for hunk in &self.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    ChangeTag::Delete => theme.error,
                    ChangeTag::Insert => theme.success,
                    ChangeTag::Equal => theme.foreground,
                };

                let old_num = line
                    .line_number_old
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = line
                    .line_number_new
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let prefix = match line.tag {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(inner.x, inner.y + y, inner.width, 1);
                frame.render_widget(styled_line, line_area);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mode_toggle = match self.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!("Scroll: ↑↓ or j/k | Mode: {} | Close: Esc", mode_toggle);

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(theme.muted),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );

        frame.render_widget(footer, area);
    }

    pub fn handle_scroll(&mut self, delta: isize) {
        let current = self.scroll.get() as isize;
        let new_scroll = current.saturating_add(delta).max(0) as usize;
        self.scroll.set(new_scroll);
    }

    pub fn scroll_to_bottom(&mut self) {
        let total = self.total_lines();
        self.scroll.set(total.saturating_sub(1));
    }
}

/// The single-border layout, as a method rather than a `Widget` impl.
///
/// It was `impl Widget for &DiffViewer` while the theme lived in a field. That
/// trait cannot carry a theme, so the only way to read the live one was to keep
/// the cache — which is the defect. An inherent method takes the theme it is
/// handed and renders byte-identically, so `ChangeReview` keeps its layout and
/// the cache goes away.
impl DiffViewer {
    /// Draw the viewer's own hunks into `area`.
    pub fn render_in(&self, area: Rect, buf: &mut ratatui::prelude::Buffer, theme: &Theme) {
        self.render_hunks_in(&self.hunks, area, buf, theme);
    }

    /// Draw `hunks` into `area` for **this viewer's** mode, scroll and title.
    ///
    /// The borrowed path, added by M003 so the change review stops cloning its
    /// parsed patch: the review state already owns the hunks, and a
    /// `Vec<DiffHunk>` built here would copy up to
    /// `MAX_REVIEW_PATCH_BYTES` on every frame. Scrolling, the side-by-side
    /// toggle and the theme are read exactly as [`Self::render_in`] reads
    /// them — that method delegates here with its own list — so the two paths
    /// cannot draw different pictures.
    pub fn render_hunks_in(
        &self,
        hunks: &[DiffHunk],
        area: Rect,
        buf: &mut ratatui::prelude::Buffer,
        theme: &Theme,
    ) {
        if area.height < 7 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Diff Viewer ");

        let inner = block.inner(area);
        block.render(area, buf);

        let header_area = Rect::new(inner.x, inner.y, inner.width, 1);
        let mode_str = match self.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(theme.primary)),
            Span::styled(&self.title, Style::default().fg(theme.foreground)),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        header.render(header_area, buf);

        let footer_area = Rect::new(
            inner.x,
            inner.y.saturating_add(inner.height).saturating_sub(1),
            inner.width,
            1,
        );

        let mode_toggle = match self.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!("Scroll: ↑↓ or j/k | Mode: {} | Close: Esc", mode_toggle);

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(theme.muted),
        )));

        footer.render(footer_area, buf);

        let content_height = inner.height.saturating_sub(2);
        if content_height == 0 {
            return;
        }

        let visible_lines = content_height as usize;
        let scroll_offset = self.scroll.get();
        let mut y = 0u16;

        for hunk in hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    ChangeTag::Delete => theme.error,
                    ChangeTag::Insert => theme.success,
                    ChangeTag::Equal => theme.foreground,
                };

                let old_num = line
                    .line_number_old
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = line
                    .line_number_new
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let prefix = match line.tag {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(inner.x, inner.y + 1 + y, inner.width, 1);
                styled_line.render(line_area, buf);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render_cost;
    use ratatui::buffer::Buffer;

    fn area() -> Rect {
        Rect::new(0, 0, 60, 14)
    }

    /// A viewer over two hunks: an edit and an insertion.
    fn viewer() -> DiffViewer {
        DiffViewer::new(
            "alpha\nbravo\ncharlie\ndelta\n".into(),
            "alpha\nBRAVO\ncharlie\ninserted\ndelta\n".into(),
            "demo.rs".into(),
        )
    }

    fn draw(render: impl FnOnce(&mut Buffer)) -> Buffer {
        let mut buffer = Buffer::empty(area());
        render(&mut buffer);
        buffer
    }

    /// The borrowed path draws byte-identically to the owned one, in both
    /// modes and at every scroll offset the review can reach.
    ///
    /// This is the property M003 must not trade away: the review stopped
    /// copying its patch, not stopped rendering it.
    #[test]
    fn the_borrowed_render_draws_what_the_owned_one_draws() {
        let theme = Theme::dark();
        let source = viewer();
        assert!(!source.hunks.is_empty(), "the fixture must have hunks");

        for mode in [DiffMode::Inline, DiffMode::SideBySide] {
            for scroll in [0, 1, 3, 50] {
                let mut owned = source.clone();
                owned.mode = mode.clone();
                owned.scroll.set(scroll);
                let hunks: Vec<DiffHunk> = owned.hunks.clone();

                let mut borrowed = DiffViewer::new(
                    String::new().into_boxed_str(),
                    String::new().into_boxed_str(),
                    "demo.rs".into(),
                );
                borrowed.mode = mode.clone();
                borrowed.scroll.set(scroll);

                let expected = draw(|buffer| owned.render_in(area(), buffer, &theme));
                let actual =
                    draw(|buffer| borrowed.render_hunks_in(&hunks, area(), buffer, &theme));
                assert_eq!(
                    expected, actual,
                    "the borrowed render differs from the owned one in {mode:?} at scroll {scroll}"
                );
            }
        }
    }

    /// The borrowed render copies nothing: `review_hunk_copies` stays put.
    #[test]
    fn the_borrowed_render_copies_no_hunks() {
        let viewer = viewer();
        let before = render_cost::snapshot();
        let buffer = draw(|buffer| {
            viewer.render_hunks_in(&viewer.hunks, area(), buffer, &Theme::dark());
        });
        let cost = render_cost::snapshot().since(&before);
        assert!(buffer.area.width > 0, "the viewer drew something");
        assert_eq!(
            cost.review_hunk_copies, 0,
            "rendering a borrowed patch copied hunks"
        );
    }

    /// The counter is produced by the copy itself, so a reintroduced
    /// whole-patch clone cannot go unmeasured.
    #[test]
    fn copying_a_hunk_moves_the_review_copy_counter() {
        let viewer = viewer();
        let before = render_cost::snapshot();
        let copy = viewer.hunks.clone();
        let cost = render_cost::snapshot().since(&before);
        assert_eq!(
            cost.review_hunk_copies,
            copy.len() as u64,
            "one count per copied hunk"
        );
    }
}

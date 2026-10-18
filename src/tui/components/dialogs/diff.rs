use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::prelude::Widget;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::tui::components::diff::{DiffMode, DiffViewer};

/// Who owns this render's chrome.
///
/// The two paths differ only in that. [`Chrome::Owned`] is the modal box, which
/// draws its own border, title and keymap footer; [`Chrome::Hosted`] is a
/// column in a side panel, where the host has already painted all three into
/// the same rect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The verbs this box implements, for the panel host to reuse verbatim.
///
/// `handle_key` below is the implementation and this is the advertisement.
/// `↑/↓` and `Esc` are deliberately absent even though `handle_key` answers
/// them: a focused panel never sees them, because `SidePanelHost::handle_key`
/// answers the arrow keys as panel directions and claims `Esc`/`q` as its
/// universal exits before consulting the widget. `Enter` is absent for a second
/// reason — it returns `TuiMsg::CloseDialog`, and docking popped this box off
/// the modal stack before the panel adopted it, so that close has an empty
/// stack to run against and does nothing. **A `/diff` in a column therefore has
/// no close verb at all**, which is why this line is the whole keymap of the
/// docked box and why the old footer's `Esc close` was a lie in it. `Home`,
/// `End` and `^d`/`^u` survive: none of them is a panel direction.
const HINT: &str = "j/k scroll · ^d/^u page · s side-by-side · Home/End jump";

#[derive(Clone)]
pub struct DiffDialog {
    pub viewer: DiffViewer,
}

impl DiffDialog {
    pub fn new(old_content: Box<str>, new_content: Box<str>, title: Box<str>) -> Self {
        Self {
            viewer: DiffViewer::new(old_content, new_content, title),
        }
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                if key.modifiers.is_empty() =>
            {
                self.viewer.handle_scroll(-1);
                true
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                if key.modifiers.is_empty() =>
            {
                self.viewer.handle_scroll(1);
                true
            }
            crossterm::event::KeyCode::Char('s') if key.modifiers.is_empty() => {
                self.viewer.toggle_mode();
                true
            }
            crossterm::event::KeyCode::PageUp => {
                self.viewer.handle_scroll(-10);
                true
            }
            crossterm::event::KeyCode::PageDown => {
                self.viewer.handle_scroll(10);
                true
            }
            crossterm::event::KeyCode::Home => {
                self.viewer.scroll.reset();
                true
            }
            crossterm::event::KeyCode::End => {
                self.viewer.scroll_to_bottom();
                true
            }
            _ => false,
        }
    }

    /// A third, chrome-owning render path, kept exactly as it was.
    ///
    /// Nothing in this crate calls it — `src/tui/app/modal.rs` hands the dialog
    /// to the focus manager as a `Box<dyn Component>`, so the modal and the
    /// panel both go through [`Component::render`] / [`Component::render_in_panel`]
    /// and therefore both go through [`Self::render_body`]. It is left alone
    /// rather than folded in, because folding paths is de-duplication and this
    /// milestone only owns the chrome split; whoever does that work will find
    /// it here and delete it.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.height < 7 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Diff Viewer ");

        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        let content_height = inner.height.saturating_sub(2);
        if content_height == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(content_height),
            Constraint::Length(1),
        ])
        .split(inner);

        let mode_str = match self.viewer.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(theme.primary)),
            Span::styled(&self.viewer.title, Style::default().fg(theme.foreground)),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        frame.render_widget(header, chunks[0]);

        let visible_lines = content_height as usize;
        let scroll_offset = self.viewer.scroll.get();
        let mut y = 0u16;

        for hunk in &self.viewer.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    similar::ChangeTag::Delete => theme.error,
                    similar::ChangeTag::Insert => theme.success,
                    similar::ChangeTag::Equal => theme.foreground,
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
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(chunks[1].x, chunks[1].y + y, chunks[1].width, 1);
                frame.render_widget(styled_line, line_area);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }

        let mode_toggle = match self.viewer.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!(
            "j/k/↑/↓ scroll  |  PgUp/PgDn  |  {}  |  Esc close",
            mode_toggle
        );

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(theme.muted),
        )));

        frame.render_widget(footer, chunks[2]);
    }

    /// One diff routine for both render paths.
    ///
    /// `visible_lines` below is derived from the rect this routine is handed
    /// and `scroll_offset` from the same `CenteredScroll` the key arms move, so
    /// the window the pass paints and the window `j`/`k`/`^d`/`^u`/`Home`/`End`
    /// clamp against cannot disagree. That is why this is one routine and not
    /// two: a hosted path computing its own viewport from a different rect is
    /// how a docked box ends up with an unreachable tail and no error anywhere.
    ///
    /// Only the chrome differs. The header line — `Diff: <file> | <mode>` — is
    /// content rather than chrome, and it stays in both: the host's panel title
    /// names the panel, not the file or the mode, so a hosted column that
    /// dropped it would say neither which file it is nor which way it is laid
    /// out.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Theme, chrome: Chrome) {
        if area.height < 7 {
            return;
        }

        let inner = match chrome {
            Chrome::Owned => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(" Diff Viewer ");
                let inner = block.inner(area);
                block.render(area, frame.buffer_mut());
                inner
            }
            // The host owns the border, the title and the `●` in this rect
            // already, and `render_in_panel` is handed the full rect rather
            // than the interior, so a second block would overwrite all three
            // instead of stacking on them.
            Chrome::Hosted => area,
        };

        // The footer is a keymap, and this box's modal keymap is a lie in a
        // column: `↑/↓` and `Esc close` are the host's to answer, and `Enter`'s
        // `CloseDialog` has an empty modal stack to run against. So the hosted
        // path spends none of its height on it, and the verbs move to
        // `keymap_hint` instead, where the host reads them once for the whole
        // panel rather than burning one row of a narrow column per box.
        let footer_rows = match chrome {
            Chrome::Owned => 1,
            Chrome::Hosted => 0,
        };
        let content_height = inner.height.saturating_sub(footer_rows + 1);
        if content_height == 0 {
            return;
        }

        let chunks = match chrome {
            Chrome::Owned => Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(content_height),
                Constraint::Length(1),
            ])
            .split(inner),
            Chrome::Hosted => {
                Layout::vertical([Constraint::Length(1), Constraint::Min(content_height)])
                    .split(inner)
            }
        };

        let mode_str = match self.viewer.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(theme.primary)),
            Span::styled(&self.viewer.title, Style::default().fg(theme.foreground)),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        frame.render_widget(header, chunks[0]);

        let visible_lines = content_height as usize;
        let scroll_offset = self.viewer.scroll.get();
        let mut y = 0u16;

        for hunk in &self.viewer.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    similar::ChangeTag::Delete => theme.error,
                    similar::ChangeTag::Insert => theme.success,
                    similar::ChangeTag::Equal => theme.foreground,
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
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(chunks[1].x, chunks[1].y + y, chunks[1].width, 1);
                frame.render_widget(styled_line, line_area);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }

        if chrome == Chrome::Owned {
            let mode_toggle = match self.viewer.mode {
                DiffMode::Inline => "Press 's' for Side-by-Side",
                DiffMode::SideBySide => "Press 's' for Inline",
            };

            let info = format!(
                "j/k/↑/↓ scroll  |  PgUp/PgDn  |  {}  |  Esc close",
                mode_toggle
            );

            let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                info,
                Style::default().fg(theme.muted),
            )));

            frame.render_widget(footer, chunks[2]);
        }
    }
}

/// The legacy single-border layout, kept byte-for-byte.
///
/// Converted from `impl Widget for &DiffDialog` because a trait cannot carry a
/// theme: with the viewer's cache gone there is no other way for this path to
/// reach one. It is still dead code (nothing renders a `DiffDialog` as a bare
/// `Widget`), and deleting it belongs to the separate legacy-path cleanup — so
/// it is given a parameter rather than removed.
impl DiffDialog {
    pub fn render_legacy(&self, area: Rect, buf: &mut ratatui::prelude::Buffer, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Diff Viewer ");

        let inner = block.inner(area);
        block.render(area, buf);

        let content_height = inner.height.saturating_sub(2);
        if content_height == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(content_height),
            Constraint::Length(1),
        ])
        .split(inner);

        let mode_str = match self.viewer.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(theme.primary)),
            Span::styled(&self.viewer.title, Style::default().fg(theme.foreground)),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        header.render(chunks[0], buf);

        let visible_lines = content_height as usize;
        let scroll_offset = self.viewer.scroll.get();
        let mut y = 0u16;

        for hunk in &self.viewer.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    similar::ChangeTag::Delete => theme.error,
                    similar::ChangeTag::Insert => theme.success,
                    similar::ChangeTag::Equal => theme.foreground,
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
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(chunks[1].x, chunks[1].y + y, chunks[1].width, 1);
                styled_line.render(line_area, buf);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }

        let mode_toggle = match self.viewer.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!(
            "j/k/↑/↓ scroll  |  PgUp/PgDn  |  {}  |  Esc close",
            mode_toggle
        );

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(theme.muted),
        )));

        footer.render(chunks[2], buf);
    }
}

impl Component for DiffDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                self.viewer.handle_scroll(-1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                self.viewer.handle_scroll(1);
                None
            }
            KeyCode::Char('s') if key.modifiers.is_empty() => {
                self.viewer.toggle_mode();
                None
            }
            KeyCode::PageUp => {
                self.viewer.handle_scroll(-10);
                None
            }
            KeyCode::PageDown => {
                self.viewer.handle_scroll(10);
                None
            }
            KeyCode::Home => {
                self.viewer.scroll.reset();
                None
            }
            KeyCode::End => {
                self.viewer.scroll_to_bottom();
                None
            }
            KeyCode::Enter | KeyCode::Esc => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    /// The verbs this box owns, spelled the way this box spells them.
    fn keymap_hint(&self) -> Option<String> {
        Some(HINT.to_string())
    }

    /// The hunks alone: no border, no title, no footer.
    ///
    /// The host has already painted a border, a ` ● Diff ` title and the `●`
    /// focus marker into this same rect, and `render_in_panel` is handed the
    /// full rect rather than the interior, so a second block does not stack —
    /// it overwrites all three, and a focused column renders exactly like an
    /// unfocused one. The box's own ` Diff Viewer ` title goes with it for the
    /// same reason; the header line inside the body still names the file and
    /// the mode, which the host's title does not. `area` is the host's full
    /// rect, so inset by its border first.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    /// The modal box: border, title and footer, all of which are correct here
    /// and only here.
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn dialog() -> DiffDialog {
        DiffDialog::new(
            "one\ntwo\nthree\nfour\nfive\n".into(),
            "one\ntwo changed\nthree\nfour\nfive\n".into(),
            "src/main.rs".into(),
        )
    }

    /// Render through the panel host: host chrome first, then
    /// `render_in_panel` over the *same* rect, exactly as `render_hosted` does.
    fn hosted_rows(dialog: &mut DiffDialog, width: u16, height: u16) -> Vec<String> {
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Diff ● ")
                        .border_style(Style::default().fg(theme.border)),
                    frame.area(),
                );
                dialog.render_in_panel(frame, frame.area(), &theme);
            })
            .expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect::<Vec<_>>()
            .chunks(width as usize)
            .map(|row| row.concat())
            .collect()
    }

    fn modal_rows(dialog: &mut DiffDialog, width: u16, height: u16) -> Vec<String> {
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| Component::render(dialog, frame, frame.area(), &theme))
            .expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect::<Vec<_>>()
            .chunks(width as usize)
            .map(|row| row.concat())
            .collect()
    }

    /// `/diff` has three entry points into the same body — `render_in_panel`,
    /// `Component::render`, and the public `render` kept for its modal callers —
    /// and its body has two more layout branches on top of those. Missing one
    /// is the most likely way this milestone ships half-done, so this sweeps
    /// the sizes that make each branch take a different shape: tall, short
    /// enough to fail `height < 7`, and wide.
    #[test]
    fn no_hosted_size_lets_the_body_paint_its_own_chrome() {
        for (width, height) in [(40u16, 16u16), (40, 8), (80, 24), (20, 7), (12, 6)] {
            let mut d = dialog();
            let rows = hosted_rows(&mut d, width, height);
            assert!(
                rows[0].contains("Diff ●"),
                "{width}x{height}: the host's title and focus marker were \
                 overwritten — row 0 is {:?}",
                rows[0]
            );
            let h = rows.len();
            for (y, row) in rows.iter().enumerate().take(h.saturating_sub(1)).skip(1) {
                let interior: String = row
                    .trim_start_matches('│')
                    .trim_end_matches('│')
                    .to_string();
                for corner in ['┌', '┐', '└', '┘'] {
                    assert!(
                        !interior.contains(corner),
                        "{width}x{height}: a second box appeared at row {y} — {row:?}"
                    );
                }
            }
        }
    }

    /// The other direction, and what makes the test above non-vacuous: the
    /// modal really does keep a titled border and its keymap footer, which is
    /// precisely why the hosted path needs its own override.
    #[test]
    fn the_modal_keeps_its_border_and_its_footer() {
        let mut d = dialog();
        let rows = modal_rows(&mut d, 40, 16);
        assert!(
            rows[0].contains("Diff Viewer"),
            "the modal keeps its own titled border — row 0 is {:?}",
            rows[0]
        );
        // The footer is one row wide and wraps, so only its first visual line is
        // on screen — `Esc close` lives past the cut. What is being checked is
        // that a keymap is painted at all, and `scroll` is its first word.
        assert!(
            rows.join("\n").contains("scroll"),
            "the modal keeps its own keymap footer"
        );
    }

    /// The modal footer is a keymap, and in a column it is false: `↑/↓` and
    /// `Esc` are the host's to answer and `Enter`'s `CloseDialog` runs against
    /// an empty modal stack. So the hosted path must not repeat any of it.
    #[test]
    fn the_hosted_path_advertises_no_key_the_host_answers() {
        let mut d = dialog();
        let text = hosted_rows(&mut d, 40, 16).join("\n");
        for dead in ["Esc close", "↑/↓"] {
            assert!(
                !text.contains(dead),
                "the hosted path still advertises {dead:?}, which it cannot \
                 deliver from a column — {text}"
            );
        }
    }

    /// A column with no interior renders nothing rather than panicking on a
    /// zero-sized rect; the host's chrome is already painted, so the panel shows
    /// an empty column.
    #[test]
    fn a_column_with_no_interior_paints_nothing_and_does_not_panic() {
        let mut d = dialog();
        let rows = hosted_rows(&mut d, 2, 2);
        // At width 2 the host's border has no room for a rule, so these rows
        // are the corners themselves. That is the whole claim: the widget
        // returned before painting, so the column is the host's and nothing else.
        assert_eq!(
            rows,
            vec!["┌┐".to_string(), "└┘".to_string()],
            "a 2x2 column has no interior, so only the host's border should remain"
        );
    }

    #[test]
    fn keymap_hint_names_no_key_the_host_answers_first() {
        let d = dialog();
        let hint = d.keymap_hint().expect("diff owns real verbs");
        for dead in ["esc", "Esc", "↑", "↓", "←", "→", "enter", "Enter"] {
            assert!(!hint.contains(dead), "the hint teaches {dead:?}: {hint:?}");
        }
        assert_eq!(d.keymap_hint().as_deref(), Some(HINT));
    }
}

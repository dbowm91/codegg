use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum InfoType {
    Context,
    Cost,
    Usage,
    ShellShow,
    TerminalShow,
    Stats,
    TaskList,
    WorktreeList,
    GoalShow,
    MemoryResults,
    DoctorReport,
    Agents,
    Collaborators,
    Team,
    Control,
    ProjectChat,
    /// `/logs`: recent daemon log tail plus the in-memory toast history.
    Logs,
    ProjectInit,
}

#[derive(Clone)]
pub struct InfoDialog {
    info_type: InfoType,
    lines: Vec<String>,
    theme: Arc<Theme>,
    scroll: usize,
    custom_footer: Option<String>,
}

impl InfoDialog {
    pub fn new(theme: Arc<Theme>, info_type: InfoType, lines: Vec<String>) -> Self {
        Self {
            info_type,
            lines,
            theme,
            scroll: 0,
            custom_footer: None,
        }
    }

    pub fn set_content(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.scroll = 0;
    }

    pub fn set_info_type(&mut self, info_type: InfoType) {
        self.info_type = info_type;
        self.scroll = 0;
    }

    fn title(&self) -> &'static str {
        match self.info_type {
            InfoType::Context => " Context ",
            InfoType::Cost => " Cost ",
            InfoType::Usage => " Usage ",
            InfoType::ShellShow => " Shell Command ",
            InfoType::TerminalShow => " Interactive Terminal ",
            InfoType::Stats => " TUI Stats ",
            InfoType::TaskList => " Tasks ",
            InfoType::WorktreeList => " Worktrees ",
            InfoType::GoalShow => " Goal ",
            InfoType::MemoryResults => " Memory ",
            InfoType::DoctorReport => " Doctor ",
            InfoType::Agents => " Agents ",
            InfoType::Collaborators => " Collaborators ",
            InfoType::Team => " Team ",
            InfoType::Control => " Control ",
            InfoType::ProjectChat => " Project Chat ",
            InfoType::Logs => " Logs ",
            InfoType::ProjectInit => " Initialize Project ",
        }
    }

    pub fn dialog_type_for_info_type(&self) -> DialogType {
        match self.info_type {
            InfoType::Context => DialogType::Context,
            InfoType::Cost => DialogType::Cost,
            InfoType::Usage => DialogType::Usage,
            InfoType::ShellShow => DialogType::ShellShow,
            InfoType::TerminalShow => DialogType::Terminal,
            InfoType::Stats => DialogType::Stats,
            InfoType::TaskList => DialogType::TaskList,
            InfoType::WorktreeList => DialogType::WorktreeList,
            InfoType::GoalShow => DialogType::GoalShow,
            InfoType::MemoryResults => DialogType::MemoryResults,
            InfoType::DoctorReport => DialogType::DoctorReport,
            InfoType::Agents => DialogType::Agent,
            InfoType::Collaborators => DialogType::Collaborators,
            InfoType::Team => DialogType::Team,
            // M004 shares the Team dialog slot (single InfoDialog
            // instance; last writer wins). Control output never
            // carries secrets, so sharing the slot is safe.
            InfoType::Control => DialogType::Team,
            InfoType::ProjectChat => DialogType::ProjectChat,
            InfoType::Logs => DialogType::Logs,
            InfoType::ProjectInit => DialogType::Team,
        }
    }

    /// Scroll to the last content line. Used by append-only windows
    /// (`/logs`) whose newest entry is at the bottom.
    pub fn scroll_to_end(&mut self) {
        // Scroll offsets are counted in *wrapped rows*, so the end position is
        // not derivable from `lines.len()` without the render width. The
        // render pass clamps `scroll` to the real maximum, so the sentinel is
        // the correct way to express "as far down as possible"; clamping
        // against the logical line count here left the final line off-screen
        // whenever an earlier line wrapped.
        self.scroll = usize::MAX;
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_custom_footer(&mut self, footer: String) {
        self.custom_footer = Some(footer);
    }

    pub fn content_lines(&self) -> &[String] {
        &self.lines
    }

    pub fn info_type(&self) -> InfoType {
        self.info_type.clone()
    }
}

impl Component for InfoDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                // No logical-line bound here: a line that wraps occupies
                // several rows, so the reachable maximum is larger than
                // `lines.len()` and clamping against it made the tail of
                // wrapped content unreachable. The render pass clamps
                // `scroll` to the true maximum, and `saturating_add` keeps
                // the `usize::MAX` sentinel from wrapping on repeat.
                self.scroll = self.scroll.saturating_add(1);
                None
            }
            crossterm::event::KeyCode::Char('a') if self.info_type == InfoType::ProjectInit => {
                Some(TuiMsg::ProjectInitApprove)
            }
            crossterm::event::KeyCode::Enter => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

        let title_block = Block::default()
            .borders(Borders::ALL)
            .title(self.title())
            .border_style(Style::default().fg(theme.border));

        // The content paragraph has its own two-row border. Derive the
        // viewport from the actual content chunk so scrolling reaches the
        // last line instead of overestimating the available rows.
        let visible_lines = (chunks[1].height as usize).saturating_sub(2);
        // Long log lines must wrap rather than run off the right edge. The
        // rows are produced here instead of by `Paragraph::wrap` because
        // `line_count` is behind ratatui's unstable `rendered-line-info`
        // feature, so the scroll offset has to be counted against the same
        // wrapping this code performs — otherwise a wrapped line occupies
        // several rows while the offset advances by one.
        let inner_width = chunks[1].width.saturating_sub(2);
        let display_lines: Vec<Line> = self
            .lines
            .iter()
            .flat_map(|s| crate::tui::wrap::wrap_to_strings(s, inner_width))
            .map(|row| Line::from(Span::styled(row, Style::default().fg(theme.foreground))))
            .collect();

        let total_lines = display_lines.len();
        let max_scroll = total_lines.saturating_sub(visible_lines);

        let start_idx = self.scroll.min(max_scroll);
        let end_idx = (start_idx + visible_lines).min(total_lines);

        let visible: Vec<Line> = display_lines[start_idx..end_idx].to_vec();

        let content_para = Paragraph::new(visible).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );

        let scroll_indicator = if total_lines > visible_lines {
            format!("Showing {}-{} of {}", start_idx + 1, end_idx, total_lines)
        } else {
            String::new()
        };

        let footer_text = if let Some(ref custom) = self.custom_footer {
            if scroll_indicator.is_empty() {
                custom.clone()
            } else {
                format!(" {}  |  {}", scroll_indicator, custom)
            }
        } else if scroll_indicator.is_empty() {
            " j/k scroll  |  Esc/Enter close ".to_string()
        } else {
            format!(" {}  |  j/k scroll  |  Esc/Enter close ", scroll_indicator)
        };

        let footer_block = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(theme.secondary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );

        frame.render_widget(title_block, chunks[0]);
        frame.render_widget(content_para, chunks[1]);
        frame.render_widget(footer_block, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        self.dialog_type_for_info_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_lines(dialog: &InfoDialog, width: u16, height: u16) -> Vec<String> {
        let theme = Arc::new(Theme::dark());
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                let mut d = dialog.clone();
                d.render(frame, frame.area(), &theme);
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

    #[test]
    fn long_log_line_wraps_instead_of_running_off_the_right_edge() {
        // A single unbroken token wider than the dialog is the worst case:
        // without wrapping it is clipped at the border and every row stops
        // short of the right edge.
        let dialog = InfoDialog::new(
            Arc::new(Theme::dark()),
            InfoType::Logs,
            vec!["x".repeat(200)],
        );
        let (w, h) = (40u16, 12u16);
        let rows = render_lines(&dialog, w, h);

        // Geometry: title(3) + content + footer(3); the content box spends two
        // more rows on its border and two more columns.
        let inner_width = (w - 2) as usize;
        let visible_rows = (h - 3 - 3 - 2) as usize;

        // Wrapping, not clipping: every visible content row is filled edge to
        // edge. Without wrapping only the first row would carry text.
        let filled: usize = rows
            .iter()
            .map(|r| r.chars().filter(|c| *c == 'x').count())
            .sum();
        assert_eq!(
            filled,
            inner_width * visible_rows,
            "content did not fill every visible row to the wrap width"
        );

        // No row may overflow the dialog.
        for row in &rows {
            assert!(
                row.chars().count() <= w as usize,
                "row overflowed the dialog: {row:?}"
            );
        }
    }

    #[test]
    fn scrolling_reaches_the_tail_after_wrapping() {
        let lines = vec!["first".to_string(), "y".repeat(200), "last".to_string()];
        let mut dialog = InfoDialog::new(Arc::new(Theme::dark()), InfoType::Logs, lines);
        dialog.scroll = usize::MAX;
        let rows = render_lines(&dialog, 40, 10);
        assert!(
            rows.concat().contains("last"),
            "scrolled view must end at the last logical line"
        );
    }

    #[test]
    fn project_init_dialog_requires_explicit_approval_and_escape_cancels() {
        let theme = Arc::new(Theme::default());
        let mut dialog = InfoDialog::new(theme, InfoType::ProjectInit, vec!["candidate".into()]);
        let approve = dialog.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(approve, Some(TuiMsg::ProjectInitApprove));
        let cancel = dialog.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(cancel, Some(TuiMsg::CloseDialog));
    }
}

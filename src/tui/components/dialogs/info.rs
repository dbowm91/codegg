use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// The verbs every `InfoType` dispatches.
const SCROLL_VERBS: &str = "j/k scroll";

/// The one verb that is conditional on the content type.
const PUBLISH_VERB: &str = "a publish";

/// Whether this widget frames its own body, or the panel host frames it.
///
/// Standalone — as a modal — the widget is the whole surface, so it draws its
/// own title, content box and footer. In a side column the host has **already**
/// painted the border, the panel's name and the `●` focus marker into the same
/// rect, so painting a second set of chrome over them does not layer, it
/// overwrites: all three, which is why a focused column and an unfocused one
/// rendered identically and the one fact the title carried was the thing
/// destroyed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    /// Modal: three stacked bordered boxes.
    Owned,
    /// Hosted: the host's frame, this box's body inset inside it.
    Hosted,
}

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
    /// `/logs`: recent daemon log tail plus the in-memory toast history.
    Logs,
    ProjectInit,
}

#[derive(Clone)]
pub struct InfoDialog {
    info_type: InfoType,
    lines: Vec<String>,
    styled_lines: Option<Vec<Line<'static>>>,
    theme: Arc<Theme>,
    scroll: usize,
    custom_footer: Option<String>,
}

impl InfoDialog {
    pub fn new(theme: Arc<Theme>, info_type: InfoType, lines: Vec<String>) -> Self {
        Self {
            info_type,
            lines,
            styled_lines: None,
            theme,
            scroll: 0,
            custom_footer: None,
        }
    }

    pub fn set_content(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.styled_lines = None;
        self.scroll = 0;
    }

    /// Set trusted, already interpreted presentation lines such as the local
    /// VT screen. This accepts ratatui styles, never raw terminal escapes.
    /// Replace the content without discarding the reader's position.
    ///
    /// The scroll offset is deliberately preserved and re-clamped rather than
    /// reset. This dialog is refreshed on every completion and on every
    /// forwarded keystroke, so a reset here means any scrolling a reader did
    /// is silently undone by whatever arrives next — the offset the reader set
    /// never survives to be used.
    pub fn set_styled_content_preserving_scroll(&mut self, lines: Vec<Line<'static>>) {
        let scroll = self.scroll;
        self.set_styled_content(lines);
        // No clamp here: `max_scroll` depends on the render width, which this
        // widget cannot see, and the render pass already clamps `scroll` to
        // the true maximum. Preserving the raw offset keeps `usize::MAX`, the
        // "as far down as possible" sentinel, meaningful.
        self.scroll = scroll;
    }

    pub fn set_styled_content(&mut self, lines: Vec<Line<'static>>) {
        self.lines.clear();
        self.styled_lines = Some(lines);
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
            InfoType::Control => DialogType::Control,
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

    /// Draw the body, with or without the chrome around it.
    ///
    /// This is the **only** place the viewport is computed, and both render
    /// paths go through it. That is structural rather than tidy, because the
    /// viewport cannot be derived twice without being derived differently:
    ///
    /// - `handle_key`'s `Down`/`j` arm deliberately does not bound `scroll`,
    ///   because a line that wraps occupies several rows and the reachable
    ///   maximum is larger than `lines.len()`.
    /// - `scroll_to_end` therefore stores a `usize::MAX` sentinel instead of an
    ///   offset, for the same reason.
    ///
    /// Both hand the clamp to this pass. Two clamps — one derived from the
    /// modal rect and one from a column inset by a different amount — reach a
    /// different last wrapped row, and nothing anywhere reports it: the panel
    /// simply stops scrolling early, with no error and nothing to see in a
    /// screenshot. Deriving `visible_lines` and the wrap width from one
    /// `text_area` means the number the clamp uses and the number the
    /// paragraph paints into are the same value by construction.
    ///
    /// The clamp is applied to the start index, never written back to
    /// `self.scroll`: the sentinel has to outlive this call, because
    /// `set_styled_content_preserving_scroll` restores it on every refresh.
    fn render_body(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        // `para_area` is what the paragraph is handed; `text_area` is what it
        // paints text into. They differ by the content box's own border when
        // this widget owns the frame, and are the same rect when it does not.
        let (para_area, text_area, status_area) = match chrome {
            Chrome::Owned => {
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
                frame.render_widget(title_block, chunks[0]);

                // The two rows and two columns the content box's border
                // occupies, subtracted per dimension so a chunk thinner than
                // its own border degrades to zero rather than to `Rect::ZERO`'s
                // other axis.
                let text = Rect {
                    x: chunks[1].x.saturating_add(1),
                    y: chunks[1].y.saturating_add(1),
                    width: chunks[1].width.saturating_sub(2),
                    height: chunks[1].height.saturating_sub(2),
                };
                (chunks[1], text, Some(chunks[2]))
            }
            Chrome::Hosted => {
                // `area` is the host's **full** column rect, border cells
                // included, so the body has to be inset by the frame before
                // anything is drawn into it. A zero-sized interior returns
                // without painting: the host's chrome is already on screen, so
                // the honest result is an empty column, not a panic.
                let inner = area.inner(Margin::new(1, 1));
                if inner.width == 0 || inner.height == 0 {
                    return;
                }
                // One row is held back for the scroll indicator, and only when
                // there is room for content as well. Reserving it
                // unconditionally rather than only once there is something to
                // report is what keeps `visible_lines` independent of whether
                // the indicator renders — otherwise the viewport would depend
                // on its own output.
                if inner.height >= 2 {
                    let body = Rect {
                        height: inner.height - 1,
                        ..inner
                    };
                    let status = Rect {
                        y: inner.y.saturating_add(body.height),
                        height: 1,
                        ..inner
                    };
                    (body, body, Some(status))
                } else {
                    (inner, inner, None)
                }
            }
        };

        let visible_lines = text_area.height as usize;
        // Long log lines must wrap rather than run off the right edge. The
        // rows are produced here instead of by `Paragraph::wrap` because
        // `line_count` is behind ratatui's unstable `rendered-line-info`
        // feature, so the scroll offset has to be counted against the same
        // wrapping this code performs — otherwise a wrapped line occupies
        // several rows while the offset advances by one.
        let inner_width = text_area.width;
        let display_lines: Vec<Line<'static>> = if let Some(styled_lines) = &self.styled_lines {
            styled_lines.clone()
        } else {
            self.lines
                .iter()
                .flat_map(|s| crate::tui::wrap::wrap_to_strings(s, inner_width))
                .map(|row| Line::from(Span::styled(row, Style::default().fg(theme.foreground))))
                .collect()
        };
        let total_lines = display_lines.len();
        let max_scroll = total_lines.saturating_sub(visible_lines);

        let start_idx = self.scroll.min(max_scroll);
        let end_idx = (start_idx + visible_lines).min(total_lines);

        let visible: Vec<Line> = display_lines[start_idx..end_idx].to_vec();
        let content_para = match chrome {
            Chrome::Owned => Paragraph::new(visible)
                .style(Style::default().fg(theme.foreground))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                ),
            Chrome::Hosted => Paragraph::new(visible).style(Style::default().fg(theme.foreground)),
        };

        let scroll_indicator = if total_lines > visible_lines {
            Some(format!(
                "Showing {}-{} of {}",
                start_idx + 1,
                end_idx,
                total_lines
            ))
        } else {
            None
        };

        match chrome {
            Chrome::Owned => {
                let footer_text = match (&self.custom_footer, &scroll_indicator) {
                    (Some(custom), Some(indicator)) => format!(" {}  |  {}", indicator, custom),
                    (Some(custom), None) => custom.clone(),
                    (None, Some(indicator)) => {
                        format!(" {}  |  j/k scroll  |  Esc/Enter close ", indicator)
                    }
                    (None, None) => " j/k scroll  |  Esc/Enter close ".to_string(),
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
                frame.render_widget(content_para, para_area);
                frame.render_widget(
                    footer_block,
                    status_area.expect("modal always has a footer"),
                );
            }
            Chrome::Hosted => {
                frame.render_widget(content_para, para_area);
                // The indicator is a status affordance rather than a verb, and
                // the verbs moved to `keymap_hint`. It is kept, on the row
                // reserved above: `Down`/`j` is unbounded by design, so without
                // it a reader who scrolled into the middle of wrapped content
                // has nothing on screen telling them they are not seeing the
                // end.
                if let (Some(status_area), Some(indicator)) = (status_area, scroll_indicator) {
                    let style = Style::default().fg(theme.secondary);
                    frame
                        .render_widget(Paragraph::new(Span::styled(indicator, style)), status_area);
                }
            }
        }
    }
}

impl Component for InfoDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
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

    /// The verbs this box dispatches, for the panel host's status line.
    ///
    /// This is a claim about what a **hosted** box does, because the hosted
    /// panel is the only surface that reads it. `handle_key` has two further
    /// arms, and neither belongs here:
    ///
    /// - `Esc` never arrives. `SidePanelHost::handle_key` claims `Esc`/`q` as
    ///   the universal panel exit and drops focus before the panel is
    ///   consulted, and a hosted panel does not override `owns_exit_key`. In a
    ///   column `Esc` returns the keyboard to the composer; naming it here
    ///   would teach that `Esc` closes the box, which is the one thing it does
    ///   not do.
    /// - `Enter` does arrive, and closes nothing: docking *pops* the box off
    ///   the modal stack, so `TuiMsg::CloseDialog` runs `close_dialog()` against
    ///   an empty stack. `Shift+←` is the way back to a modal, and the host
    ///   already says so next to this text.
    ///
    /// A caller-supplied footer is deliberately not reused. `/project-init`'s
    /// `"a: publish   Esc: cancel"` and `/shell-show`'s six-verb footer
    /// describe the **modal** box, where `Esc` really does close; replaying
    /// them here would re-advertise exactly the dead `Esc` this hint exists to
    /// stop describing, and `ShellShow`'s footer additionally advertises `k
    /// kill` where this widget's `k` scrolls up.
    ///
    /// Per `InfoType` and not per `DialogType`: `ProjectInit` and `Team` share
    /// `DialogType::Team` and therefore one panel, and only the first of the
    /// two arms `a`.
    fn keymap_hint(&self) -> Option<String> {
        Some(match self.info_type {
            InfoType::ProjectInit => format!("{SCROLL_VERBS} · {PUBLISH_VERB}"),
            _ => SCROLL_VERBS.to_string(),
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    /// The body alone, inside the frame the host already drew.
    ///
    /// `Component::render`'s three stacked bordered boxes overwrote the host's
    /// border, its title and the `●` focus marker, which is why this seam
    /// exists. What is left here is deliberately the *same routine* the modal
    /// path uses rather than a second implementation of it: see
    /// [`InfoDialog::render_body`] for why two clamps would not be one
    /// implementation.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Hosted);
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

    /// Draw through one of the two seams and return the rows.
    fn draw(dialog: &mut InfoDialog, width: u16, height: u16, hosted: bool) -> Vec<String> {
        let theme = Arc::new(Theme::dark());
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                let area = frame.area();
                if hosted {
                    dialog.render_in_panel(frame, area, &theme);
                } else {
                    dialog.render(frame, area, &theme);
                }
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

    /// Content long enough that the wrap width, not `lines.len()`, decides how
    /// many rows there are.
    fn wrapped_body() -> Vec<String> {
        let mut lines = vec!["FIRST".to_string()];
        lines.extend(std::iter::repeat_n(
            "the quick brown fox jumps over the lazy dog".to_string(),
            30,
        ));
        lines.push("LASTLINE".to_string());
        lines
    }

    #[test]
    fn the_hosted_path_draws_no_border_of_its_own() {
        // `Component::render` is the modal path and a border is correct there,
        // which is why no static check can separate the two. This is the
        // behavioural form of the claim: in chrome-free mode the buffer holds
        // no box-drawing character at all.
        let mut dialog = InfoDialog::new(Arc::new(Theme::dark()), InfoType::Logs, wrapped_body());
        let rows = draw(&mut dialog, 44, 16, true);
        let joined = rows.concat();
        assert!(
            !joined
                .chars()
                .any(|c| matches!(c, '─' | '│' | '┌' | '┐' | '└' | '┘')),
            "the hosted body drew chrome:\n{}",
            rows.join("\n")
        );
    }

    #[test]
    fn the_hosted_path_shows_the_last_wrapped_row_and_keeps_the_sentinel() {
        // `scroll_to_end` stores `usize::MAX` precisely because this pass owns
        // the clamp, and a line that wraps occupies several rows so the
        // maximum is not `lines.len()`. Two things have to hold, and they pull
        // in opposite directions:
        //
        //  - the effective start must be the real maximum, or the slice begins
        //    at `usize::MAX` and an append-only window like `/logs` renders an
        //    empty body with no error anywhere;
        //  - and the stored sentinel must **survive** the pass, because
        //    `set_styled_content_preserving_scroll` restores it on every
        //    refresh and consumes the offset the reader set.
        let mut dialog = InfoDialog::new(Arc::new(Theme::dark()), InfoType::Logs, wrapped_body());
        dialog.scroll_to_end();
        assert_eq!(dialog.scroll, usize::MAX, "premise: the sentinel is stored");

        let rows = draw(&mut dialog, 44, 16, true);
        let joined = rows.concat();

        assert!(
            joined.contains("LASTLINE"),
            "the last wrapped row is unreachable when hosted:\n{}",
            rows.join("\n")
        );
        assert!(
            !joined.contains("FIRST"),
            "a scrolled-to-end window must not still be showing its head:\n{}",
            rows.join("\n")
        );
        assert!(
            joined.contains("Showing"),
            "the hosted body dropped the scroll indicator, leaving a reader \
             scrolled into the middle of wrapped content with nothing on \
             screen saying they are not seeing the end:\n{}",
            rows.join("\n")
        );
        let at = joined.find("Showing ").unwrap() + "Showing ".len();
        let rest = &joined[at..];
        let of = rest.find(" of ").expect("a total");
        let end: usize = rest[rest.find('-').unwrap() + 1..of]
            .trim()
            .parse()
            .expect("an end");
        let total: usize = rest[of + 4..]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .expect("a total");
        assert_eq!(
            end, total,
            "the indicator says the view ends at {end} of {total}: the clamp \
             and the painted body disagree"
        );
        assert_eq!(
            dialog.scroll,
            usize::MAX,
            "the render consumed the sentinel, so the next refresh loses the \
             reader's position"
        );
    }

    #[test]
    fn both_paths_reach_the_same_last_wrapped_row() {
        // Same content, same `usize::MAX` sentinel, two seams. The host's
        // column is a different size from a modal box so the two may disagree
        // about how many rows fit — they may not disagree about which wrapped
        // row is the last one, nor about how many wrapped rows there are in
        // total.
        let rendered = |hosted: bool| {
            let mut dialog =
                InfoDialog::new(Arc::new(Theme::dark()), InfoType::Logs, wrapped_body());
            dialog.scroll_to_end();
            let rows = draw(&mut dialog, 44, 16, hosted);
            let joined = rows.concat();
            assert!(
                joined.contains("LASTLINE"),
                "the last wrapped row is unreachable (hosted={hosted}):\n{}",
                rows.join("\n")
            );
            assert_eq!(
                dialog.scroll,
                usize::MAX,
                "the sentinel did not survive (hosted={hosted})"
            );
            let at = joined.find("Showing ").expect("an indicator") + "Showing ".len();
            let rest = &joined[at..];
            let end = rest.find('-').unwrap();
            let of = rest.find(" of ").expect("a total");
            (
                rest[end + 1..of]
                    .trim()
                    .parse::<usize>()
                    .expect("an end row"),
                rest[of + 4..]
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap()
                    .parse::<usize>()
                    .expect("a total"),
            )
        };

        let (modal_end, modal_total) = rendered(false);
        let (hosted_end, hosted_total) = rendered(true);
        assert_eq!(
            modal_total, hosted_total,
            "the two paths disagree on how many wrapped rows there are"
        );
        assert_eq!(
            modal_end, modal_total,
            "the modal view does not end on the last wrapped row"
        );
        assert_eq!(
            hosted_end, hosted_total,
            "the hosted view stops at {hosted_end} of {hosted_total}: the tail \
             of wrapped content is unreachable"
        );
    }

    #[test]
    fn keymap_hint_names_publish_only_for_the_content_type_that_arms_it() {
        // Per `InfoType`, never per `DialogType`: `ProjectInit` and `Team` share
        // `DialogType::Team` and therefore one panel, and only the first arms
        // `a`.
        let hint = |info_type| {
            InfoDialog::new(Arc::new(Theme::dark()), info_type, vec!["x".into()])
                .keymap_hint()
                .expect("every InfoType advertises its verbs")
        };

        assert_eq!(hint(InfoType::ProjectInit), "j/k scroll · a publish");
        assert_eq!(hint(InfoType::Team), "j/k scroll");
        assert_eq!(hint(InfoType::Logs), "j/k scroll");
        assert_ne!(hint(InfoType::ProjectInit), hint(InfoType::Team));
    }

    #[test]
    fn keymap_hint_never_names_a_verb_the_hosted_path_cannot_deliver() {
        // `Enter`/`Esc` reach this widget modally and are dispatched here, but
        // neither belongs in the panel status line: `SidePanelHost::handle_key`
        // claims `Esc`/`q` as the universal exit and blurs before the panel is
        // consulted, and `Enter`'s `CloseDialog` runs against an empty modal
        // stack because docking popped the box off it.
        let hint = InfoDialog::new(Arc::new(Theme::dark()), InfoType::Logs, vec!["x".into()])
            .keymap_hint()
            .expect("a hint");
        assert!(
            !hint.contains("esc"),
            "{hint:?} advertises a key the host takes"
        );
        assert!(
            !hint.contains("enter"),
            "{hint:?} advertises a key that closes nothing"
        );
        // The fallback this replaced named an `i` no arm ever dispatched.
        assert!(!hint.contains("i act"), "{hint:?}");
    }
}

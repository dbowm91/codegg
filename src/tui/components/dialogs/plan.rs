use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::scroll::CenteredScroll;
use crate::tui::theme::Theme;

/// Who owns this render's chrome.
///
/// The two paths differ only in that. [`Chrome::Owned`] is the modal box, which
/// draws its own three bordered chunks and their titles; [`Chrome::Hosted`] is
/// a column in a side panel, where the host has already painted a border, a
/// title and the `●` focus marker into the same rect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The verbs this box implements, for the panel host to reuse verbatim.
///
/// `handle_key` below is the implementation and this is the advertisement.
/// `Enter` and `Esc` are deliberately absent even though `handle_key` answers
/// them, and neither one is an oversight: `SidePanelHost::handle_key` claims
/// `Esc` as a universal exit before the widget is consulted, and docking popped
/// this box off the modal stack (`dock_open_box` closes the dialog before
/// handing the component to `adopt`), so `Enter`'s `TuiMsg::CloseDialog` runs
/// against an empty stack and does nothing. **`j`/`k` are therefore the whole
/// keymap of a docked `/plan`** — two ways to move a highlight that a column
/// cannot then confirm, which is exactly what `keymap_hint` has to say out loud
/// rather than leave to a keymap the host would overwrite.
const HINT: &str = "j/k select";

#[derive(Clone)]
pub struct PlanDialog {
    pub plan_id: String,
    pub plan_description: String,
    pub selected: bool,
    pub scroll: CenteredScroll,
}

impl PlanDialog {
    pub fn new(plan_id: String, plan_description: String) -> Self {
        Self {
            plan_id,
            plan_description,
            selected: false,
            scroll: CenteredScroll::new(),
        }
    }

    /// A third, chrome-owning render path, kept exactly as it was.
    ///
    /// Nothing in this crate calls it — `src/tui/app/modal.rs` hands the dialog
    /// to the focus manager as a `Box<dyn Component>`, so both the modal and
    /// the panel go through [`Component::render`] / [`Component::render_in_panel`]
    /// and therefore both go through [`Self::render_body`]. It is left alone
    /// rather than folded in, because folding paths is de-duplication and this
    /// milestone only owns the chrome split.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(6),
            Constraint::Length(5),
        ])
        .split(area);

        let title = Paragraph::new(Line::from(vec![
            Span::styled("Plan ", Style::default().fg(theme.warning)),
            Span::styled("Confirmation", Style::default().fg(theme.foreground)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Plan Request ")
                .border_style(Style::default().fg(theme.warning)),
        );
        frame.render_widget(title, chunks[0]);

        let description_lines: Vec<Line> = self
            .plan_description
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.foreground))))
            .collect();

        let description = Paragraph::new(description_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(description, chunks[1]);

        let options: Vec<Line> = vec![
            if !self.selected {
                Line::from(Span::styled(
                    "> Cancel",
                    Style::default().fg(theme.accent_text).bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Cancel",
                    Style::default().fg(theme.foreground),
                ))
            },
            if self.selected {
                Line::from(Span::styled(
                    "> Confirm",
                    Style::default().fg(theme.accent_text).bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Confirm",
                    Style::default().fg(theme.foreground),
                ))
            },
        ];

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Options (↑↓ select  |  Enter confirm) ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(options_para, chunks[2]);
    }

    /// One body for both render paths.
    ///
    /// The description and the two option lines are built once, above the
    /// `match`, and the chrome switch below only chooses how they are boxed and
    /// how many rows the boxes cost. That split matters because the chunk
    /// heights *are* the layout: a hosted column that reserved the modal's five
    /// rows for an options block with no border would spend two of them on
    /// nothing, and the description — the thing a plan request is actually read
    /// for — is what gets squeezed.
    ///
    /// Hosted, the options heading names what the block is (` Options `) rather
    /// than repeating the keymap the old border title carried. `" Options (↑↓
    /// select  |  Enter confirm) "` was a dead keymap even modally, because both
    /// of those verbs are the host's to answer; the verbs now live in
    /// `keymap_hint`, read once by the host, instead of in three copies of a
    /// border title that a panel overwrote anyway.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        let description_lines: Vec<Line> = self
            .plan_description
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.foreground))))
            .collect();

        let options: Vec<Line> = vec![
            if !self.selected {
                Line::from(Span::styled(
                    "> Cancel",
                    Style::default().fg(theme.accent_text).bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Cancel",
                    Style::default().fg(theme.foreground),
                ))
            },
            if self.selected {
                Line::from(Span::styled(
                    "> Confirm",
                    Style::default().fg(theme.accent_text).bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Confirm",
                    Style::default().fg(theme.foreground),
                ))
            },
        ];

        let heading = Line::from(vec![
            Span::styled("Plan ", Style::default().fg(theme.warning)),
            Span::styled("Confirmation", Style::default().fg(theme.foreground)),
        ]);

        match chrome {
            Chrome::Owned => {
                let chunks = Layout::vertical([
                    Constraint::Length(4),
                    Constraint::Min(6),
                    Constraint::Length(5),
                ])
                .split(area);

                let title = Paragraph::new(heading).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Plan Request ")
                        .border_style(Style::default().fg(theme.warning)),
                );
                frame.render_widget(title, chunks[0]);

                let description = Paragraph::new(description_lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                );
                frame.render_widget(description, chunks[1]);

                let options_para = Paragraph::new(options).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Options (↑↓ select  |  Enter confirm) ")
                        .border_style(Style::default().fg(theme.border)),
                );
                frame.render_widget(options_para, chunks[2]);
            }
            Chrome::Hosted => {
                // No borders, so no two rows per chunk: one row of heading, one
                // of ` Options `, and the two option lines themselves.
                let chunks = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Min(6),
                    Constraint::Length(3),
                ])
                .split(area);

                let title = Paragraph::new(heading);
                frame.render_widget(title, chunks[0]);

                let description = Paragraph::new(description_lines);
                frame.render_widget(description, chunks[1]);

                let mut hosted_options = vec![Line::from(Span::styled(
                    " Options ",
                    Style::default().fg(theme.muted),
                ))];
                hosted_options.extend(options);
                frame.render_widget(Paragraph::new(hosted_options), chunks[2]);
            }
        }
    }
}

/// The legacy single-border layout, converted from
/// `impl ratatui::widgets::Widget for &PlanDialog`.
///
/// A trait method cannot receive the live theme, which is the only reason this
/// path needed a cached field in the first place. Given a parameter it renders
/// byte-identically. Still dead code — deleting it belongs to the legacy-path
/// cleanup — so it is given a theme rather than removed.
impl PlanDialog {
    pub fn render_legacy(
        &self,
        area: Rect,
        buf: &mut ratatui::prelude::Buffer,
        theme: &Arc<Theme>,
    ) {
        // Imported here rather than at the file top: as a `Widget` impl this body
        // got `Block::render` from the trait it was implementing, and an inherent
        // method needs the trait in scope on its own.
        use ratatui::widgets::Widget;

        if area.height < 3 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Plan Request ");

        let inner = block.inner(area);
        block.render(area, buf);

        let description_lines: Vec<Line> = self
            .plan_description
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.foreground))))
            .collect();

        let description = Paragraph::new(description_lines);
        ratatui::widgets::Widget::render(description, inner, buf);

        let options: Vec<Line> = vec![
            if !self.selected {
                Line::from(Span::styled(
                    "> Cancel",
                    Style::default().fg(theme.accent_text).bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Cancel",
                    Style::default().fg(theme.foreground),
                ))
            },
            if self.selected {
                Line::from(Span::styled(
                    "> Confirm",
                    Style::default().fg(theme.accent_text).bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Confirm",
                    Style::default().fg(theme.foreground),
                ))
            },
        ];

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Options (↑↓ select  |  Enter confirm) ")
                .border_style(Style::default().fg(theme.border)),
        );

        let opts_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y.saturating_sub(1),
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };
        ratatui::widgets::Widget::render(options_para, opts_area, buf);
    }
}

impl Component for PlanDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.selected = false;
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.selected = true;
                None
            }
            crossterm::event::KeyCode::Enter => Some(TuiMsg::CloseDialog),
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

    /// The plan and its two options: no borders, no border titles, and a
    /// heading that says what the box is rather than which keys it once had.
    ///
    /// The host has already painted a border, a ` ● Plan ` title and the `●`
    /// focus marker into this same rect, and `render_in_panel` is handed the
    /// full rect rather than the interior, so this box's three bordered chunks
    /// did not stack inside that frame — they overwrote it, focus dot included.
    /// The options heading survives as plain text (` Options `) because it names
    /// the block; the keymap that used to share that title is gone from the
    /// drawing and lives in `keymap_hint`, where the host reads it once. `area`
    /// is the host's full rect, so inset by its border first.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    /// The modal box: three bordered chunks and their titles, all of which are
    /// correct here and only here.
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn dialog() -> PlanDialog {
        PlanDialog::new("plan-1".into(), "ship the thing".into())
    }

    fn hosted_rows(dialog: &mut PlanDialog, width: u16, height: u16) -> Vec<String> {
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Plan ● ")
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

    fn modal_rows(dialog: &mut PlanDialog, width: u16, height: u16) -> Vec<String> {
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

    /// `/plan` stacks **three** bordered blocks when modal — a titled heading, a
    /// description, and a titled options box — against a chrome-free hosted
    /// layout with its own `Constraint::Length` values. Sweeping the sizes that
    /// make each layout take a different shape is what proves the hosted branch
    /// is chrome-free rather than chrome-free at one lucky size.
    #[test]
    fn no_hosted_size_lets_the_body_paint_its_own_chrome() {
        for (width, height) in [(40u16, 20u16), (40, 11), (80, 30), (20, 8), (12, 5)] {
            let mut d = dialog();
            let rows = hosted_rows(&mut d, width, height);
            assert!(
                rows[0].contains("Plan ●"),
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
                        "{width}x{height}: one of the three boxes still draws \
                         itself at row {y} — {row:?}"
                    );
                }
            }
        }
    }

    /// The other direction, and what makes the test above non-vacuous: modally
    /// this box really does draw three stacked boxes, one of which titles
    /// itself with its own keymap.
    #[test]
    fn the_modal_keeps_all_three_of_its_boxes() {
        let mut d = dialog();
        let rows = modal_rows(&mut d, 40, 20);
        assert!(
            rows[0].contains("Plan Request"),
            "the modal keeps its titled heading box — row 0 is {:?}",
            rows[0]
        );
        let text = rows.join("\n");
        assert!(
            text.contains("Options (↑↓ select  |  Enter confirm)"),
            "the modal keeps its keymap-bearing options title"
        );
    }

    /// That options title was the worst copy of the defect in this milestone: it
    /// is a keymap painted into chrome the host overwrites, and **both** verbs in
    /// it are ones a column cannot deliver — `↑/↓` are host panel directions and
    /// `Enter`'s `CloseDialog` runs against an empty modal stack. A hosted `/plan`
    /// must therefore name neither, which leaves `j`/`k` and nothing else.
    #[test]
    fn the_hosted_path_never_names_a_key_the_host_answers() {
        let mut d = dialog();
        let text = hosted_rows(&mut d, 40, 20).join("\n");
        for dead in ["↑↓", "Enter confirm", "Options ("] {
            assert!(
                !text.contains(dead),
                "the hosted path still advertises {dead:?} — {text}"
            );
        }
        assert!(
            text.contains("Cancel") && text.contains("Confirm"),
            "the options themselves are content and must survive: {text}"
        );
    }

    /// `j`/`k` are a docked `/plan`'s only verbs, so the hint says exactly that
    /// and nothing more.
    #[test]
    fn keymap_hint_names_no_key_the_host_answers_first() {
        let d = dialog();
        let hint = d.keymap_hint().expect("plan owns real verbs");
        for dead in ["esc", "Esc", "↑", "↓", "enter", "Enter"] {
            assert!(
                !hint.contains(dead),
                "the hint teaches {dead:?}, which a column cannot deliver: {hint:?}"
            );
        }
        assert_eq!(d.keymap_hint().as_deref(), Some(HINT));
    }
}

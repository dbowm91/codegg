use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// Who owns this render's chrome.
///
/// The two paths differ only in that. [`Chrome::Owned`] is the modal box, which
/// draws its own title, content frame and footer; [`Chrome::Hosted`] is a
/// column in a side panel, where the host has already painted a border, a
/// title and the `●` focus marker into the same rect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The verbs this box dispatches, for the panel host to reuse verbatim.
///
/// `handle_key` also answers `↑/↓` and `Esc`, and neither survives docking:
/// `SidePanelHost::handle_key` claims the arrows as panel directions and
/// claims `Esc`/`q` as its universal exits *before* consulting the widget, so
/// the old `" j/k scroll | Esc close "` footer advertised a pair a focused
/// column can never deliver. The verbs moved to `keymap_hint`; see
/// [`PluginDialog::render_body`] for why the scroll indicator did not.
const HINT: &str = "j/k scroll";

#[derive(Clone)]
pub struct PluginDialog {
    id: String,
    title: String,
    lines: Vec<String>,
    scroll: usize,
}

impl PluginDialog {
    pub fn new(id: String, title: String, lines: Vec<String>) -> Self {
        Self {
            id,
            title,
            lines,
            scroll: 0,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn update_content(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.scroll = 0;
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// One body routine for both render paths.
    ///
    /// The modal footer carried two different things and they do not belong
    /// together:
    ///
    /// - `j/k scroll | Esc close` is a keymap. It moved to
    ///   [`Component::keymap_hint`], because in a column the host's hint line
    ///   is where a docked box says what it does, and because `Esc` cannot
    ///   reach this widget from a panel at all.
    /// - `"12-30 of 30"` is a scroll indicator: it is content, not a verb, and
    ///   it is the only end-of-content signal on screen. `j`/`k` here are
    ///   deliberately unbounded against the rect — `handle_key` caps at
    ///   `lines.len() - 1`, which has nothing to do with how many rows the
    ///   column has — so without it a reader scrolled into the middle cannot
    ///   tell there is more below. It is therefore drawn on a reserved bottom
    ///   row in the hosted path too, on its own.
    ///
    /// The viewport below is derived from the rect actually being filled, in
    /// this routine and nowhere else: the modal spends two more rows on the
    /// content box's border than a column does, and a clamp derived per path
    /// would reach a different last line in each.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        let (content_area, status_area) = match chrome {
            Chrome::Owned => {
                let chunks = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(area);

                let title_block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.title))
                    .border_style(Style::default().fg(theme.border));
                frame.render_widget(title_block, chunks[0]);

                (chunks[1], Some(chunks[2]))
            }
            Chrome::Hosted => {
                // One row is held back for the scroll indicator, and only when
                // there is room for content as well. Reserving it
                // unconditionally would make the viewport depend on its own
                // output; `area` is already inset by the host's border.
                if area.height >= 2 {
                    let content = Rect {
                        height: area.height - 1,
                        ..area
                    };
                    let status = Rect {
                        y: area.y.saturating_add(content.height),
                        height: 1,
                        ..area
                    };
                    (content, Some(status))
                } else {
                    (area, None)
                }
            }
        };

        // The content paragraph has its own two-row border when this widget
        // owns the frame, so the viewport is the content chunk less those rows.
        let visible_lines = match chrome {
            Chrome::Owned => (content_area.height as usize).saturating_sub(2),
            Chrome::Hosted => content_area.height as usize,
        };
        let total_lines = self.lines.len();
        let max_scroll = total_lines.saturating_sub(visible_lines);

        let start_idx = self.scroll.min(max_scroll);
        let end_idx = (start_idx + visible_lines).min(total_lines);

        let display_lines: Vec<Line> = self.lines[start_idx..end_idx]
            .iter()
            .map(|s| {
                Line::from(Span::styled(
                    s.as_str(),
                    Style::default().fg(theme.foreground),
                ))
            })
            .collect();

        let content_para = Paragraph::new(display_lines).wrap(Wrap { trim: false });
        let content_para = match chrome {
            Chrome::Owned => content_para.block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            ),
            Chrome::Hosted => content_para,
        };

        let scroll_indicator = if total_lines > visible_lines {
            format!(" {}-{} of {} ", start_idx + 1, end_idx, total_lines)
        } else {
            String::new()
        };

        match chrome {
            Chrome::Owned => {
                let footer_text = if scroll_indicator.is_empty() {
                    " j/k scroll | Esc close ".to_string()
                } else {
                    format!("{} | j/k scroll | Esc close ", scroll_indicator.trim())
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

                frame.render_widget(content_para, content_area);
                frame.render_widget(
                    footer_block,
                    status_area.expect("the modal layout always has a footer"),
                );
            }
            Chrome::Hosted => {
                frame.render_widget(content_para, content_area);
                // The indicator outlives the footer it used to be half-buried
                // in, because it is content rather than a verb — and it is
                // the only end-of-content signal on screen.
                if let (Some(status_area), false) = (status_area, scroll_indicator.is_empty()) {
                    let indicator = Paragraph::new(Line::from(Span::styled(
                        scroll_indicator,
                        Style::default().fg(theme.secondary),
                    )));
                    frame.render_widget(indicator, status_area);
                }
            }
        }
    }
}

impl Component for PluginDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
    fn dialog_type(&self) -> DialogType {
        DialogType::Plugin
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                let max_scroll = self.lines.len().saturating_sub(1);
                if self.scroll < max_scroll {
                    self.scroll += 1;
                }
                None
            }
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    /// The verbs this box owns, spelled the way this box spells them.
    ///
    /// Deliberately shorter than the footer it replaces: see [`HINT`].
    fn keymap_hint(&self) -> Option<String> {
        Some(HINT.to_string())
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    /// The content alone, inside the frame the host already drew.
    ///
    /// `Component::render` stacked three bordered chunks over the host's
    /// border, its title and the `●` focus marker — overwriting all three,
    /// which is why a focused column and an unfocused one looked the same.
    /// What is left here is the *same* routine the modal path uses, not a
    /// second implementation: see [`PluginDialog::render_body`] for why one
    /// scroll window cannot be computed twice.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_plugin_dialog() {
        let dialog = PluginDialog::new("test-id".into(), "Test Plugin".into(), vec![]);
        assert_eq!(dialog.id(), "test-id");
        assert_eq!(dialog.title, "Test Plugin");
    }

    #[test]
    fn test_scroll_down() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines);

        assert_eq!(dialog.scroll, 0);
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 1);
    }

    #[test]
    fn test_scroll_up() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines);

        dialog.scroll = 5;
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 4);
    }

    #[test]
    fn test_scroll_up_at_zero_stays_at_zero() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines);

        assert_eq!(dialog.scroll, 0);
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_scroll_down_capped() {
        let lines: Vec<String> = (0..3).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines);

        // scroll past the end
        dialog.scroll = 2;
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 2);
    }

    #[test]
    fn test_vim_keys() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines);

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('j'),
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 1);

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('k'),
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_esc_closes_dialog() {
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), vec![]);

        let msg = dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(msg, Some(TuiMsg::CloseDialog));
    }

    #[test]
    fn test_update_content_replaces_lines_and_resets_scroll() {
        let initial_lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), initial_lines);

        dialog.scroll = 10;
        let new_lines: Vec<String> = (0..5).map(|i| format!("new {}", i)).collect();
        dialog.update_content(new_lines);

        assert_eq!(dialog.lines.len(), 5);
        assert_eq!(dialog.lines[0], "new 0");
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_dialog_type() {
        let dialog = PluginDialog::new("id".into(), "Title".into(), vec![]);
        assert_eq!(dialog.dialog_type(), DialogType::Plugin);
    }
}

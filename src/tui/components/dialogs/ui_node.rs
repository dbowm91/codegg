use std::sync::Arc;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use codegg_protocol::ui::UiNode;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::ui_node_renderer::UiNodeRenderer;
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct UiNodeDialog {
    id: String,
    title: String,
    body: UiNode,
    scroll: usize,
    theme: Arc<Theme>,
}

impl UiNodeDialog {
    pub fn new(id: String, title: String, body: UiNode, theme: Arc<Theme>) -> Self {
        Self {
            id,
            title,
            body,
            scroll: 0,
            theme,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn update_content(&mut self, body: UiNode) {
        self.body = body;
        self.scroll = 0;
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn lines(&self) -> Vec<String> {
        UiNodeRenderer::node_to_lines(&self.body)
    }
}

impl Component for UiNodeDialog {
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
                let max_scroll = self.lines().len().saturating_sub(1);
                if self.scroll < max_scroll {
                    self.scroll += 1;
                }
                None
            }
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Enter => {
                Some(TuiMsg::CloseDialog)
            }
            crossterm::event::KeyCode::PageDown => {
                self.scroll = (self.scroll + 10).min(self.lines().len().saturating_sub(1));
                None
            }
            crossterm::event::KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                None
            }
            crossterm::event::KeyCode::Home => {
                self.scroll = 0;
                None
            }
            crossterm::event::KeyCode::End => {
                let total = self.lines().len();
                if total > 0 {
                    self.scroll = total - 1;
                }
                None
            }
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        let lines = self.lines();

        let title_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .border_style(Style::default().fg(self.theme.border));

        let visible_lines = (area.height as usize).saturating_sub(6);
        let total_lines = lines.len();
        let max_scroll = total_lines.saturating_sub(visible_lines);

        let start_idx = self.scroll.min(max_scroll);
        let end_idx = (start_idx + visible_lines).min(total_lines);

        let display_lines: Vec<Line> = lines[start_idx..end_idx]
            .iter()
            .map(|s| {
                Line::from(Span::styled(
                    s.as_str(),
                    Style::default().fg(self.theme.foreground),
                ))
            })
            .collect();

        let content_para = Paragraph::new(display_lines)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.theme.border)),
            );

        let scroll_indicator = if total_lines > visible_lines {
            format!(" {}-{} of {} ", start_idx + 1, end_idx, total_lines)
        } else {
            String::new()
        };

        let footer_text = if scroll_indicator.is_empty() {
            " j/k scroll | PageUp/PageDown page | Home/End jump | Esc/Enter close ".to_string()
        } else {
            format!(
                "{} | j/k scroll | PageUp/PageDown page | Home/End jump | Esc/Enter close ",
                scroll_indicator.trim()
            )
        };

        let footer_block = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(self.theme.secondary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border)),
        );

        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

        frame.render_widget(title_block, chunks[0]);
        frame.render_widget(content_para, chunks[1]);
        frame.render_widget(footer_block, chunks[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Theme;
    use codegg_protocol::ui::TextNode;

    fn test_theme() -> Arc<Theme> {
        Arc::new(Theme::dark())
    }

    fn make_text_node(text: &str) -> UiNode {
        UiNode::Text(TextNode {
            text: text.to_string(),
        })
    }

    #[test]
    fn test_new_dialog_stores_fields() {
        let theme = test_theme();
        let body = make_text_node("hello");
        let dialog = UiNodeDialog::new("id1".into(), "Title".into(), body, Arc::clone(&theme));
        assert_eq!(dialog.id(), "id1");
        assert_eq!(dialog.title(), "Title");
        assert_eq!(dialog.lines(), vec!["hello"]);
    }

    #[test]
    fn test_scroll_down_increments() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..20)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        assert_eq!(dialog.scroll, 0);
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 1);
    }

    #[test]
    fn test_scroll_up_decrements() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..20)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
        dialog.scroll = 5;

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 4);
    }

    #[test]
    fn test_scroll_down_capped_at_last_line() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..3)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
        dialog.scroll = 2;

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 2);
    }

    #[test]
    fn test_scroll_up_capped_at_zero() {
        let theme = test_theme();
        let body = make_text_node("line");
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_esc_closes() {
        let theme = test_theme();
        let body = make_text_node("hello");
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        let msg = dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(msg, Some(TuiMsg::CloseDialog));
    }

    #[test]
    fn test_enter_closes() {
        let theme = test_theme();
        let body = make_text_node("hello");
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        let msg = dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(msg, Some(TuiMsg::CloseDialog));
    }

    #[test]
    fn test_page_down_jumps_10() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..50)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageDown,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 10);
    }

    #[test]
    fn test_page_up_jumps_10() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..50)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
        dialog.scroll = 25;

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 15);
    }

    #[test]
    fn test_home_jumps_to_zero() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..20)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
        dialog.scroll = 15;

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Home,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_end_jumps_to_last() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..20)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::End,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 19);
    }

    #[test]
    fn test_update_content_resets_scroll() {
        let theme = test_theme();
        let body1 = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..20)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("old {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body1, Arc::clone(&theme));
        dialog.scroll = 10;

        let body2 = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..5)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("new {}", i),
                    })
                })
                .collect(),
        });
        dialog.update_content(body2);

        assert_eq!(dialog.lines().len(), 5);
        assert_eq!(dialog.lines()[0], "new 0");
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_lines_flattens_ui_node() {
        let theme = test_theme();
        let node = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: Some("Header".into()),
            children: vec![
                UiNode::Text(TextNode {
                    text: "line1".into(),
                }),
                UiNode::Text(TextNode {
                    text: "line2".into(),
                }),
            ],
        });
        let dialog = UiNodeDialog::new("id".into(), "Title".into(), node, Arc::clone(&theme));
        assert_eq!(dialog.lines(), vec!["Header:", "line1", "line2"]);
    }

    #[test]
    fn test_lines_strips_ansi_from_text() {
        let theme = test_theme();
        let body = UiNode::Text(TextNode {
            text: "\x1b[31mred\x1b[0m".into(),
        });
        let dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
        assert_eq!(dialog.lines(), vec!["red"]);
    }

    #[test]
    fn test_dialog_type_is_plugin() {
        let theme = test_theme();
        let body = make_text_node("hello");
        let dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
        assert_eq!(dialog.dialog_type(), DialogType::Plugin);
    }

    #[test]
    fn test_render_does_not_panic() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: Some("Test".into()),
            children: vec![
                UiNode::Text(TextNode {
                    text: "line1".into(),
                }),
                UiNode::Text(TextNode {
                    text: "line2".into(),
                }),
            ],
        });
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut dialog =
                    UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
                let area = frame.area();
                dialog.render(frame, area, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_empty_node_does_not_panic() {
        let theme = test_theme();
        let body = UiNode::Empty;
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut dialog =
                    UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
                let area = frame.area();
                dialog.render(frame, area, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_page_down_capped_at_end() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..5)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageDown,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 4);
    }

    #[test]
    fn test_page_up_capped_at_zero() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..20)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));
        dialog.scroll = 5;

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_end_on_empty_does_not_underflow() {
        let theme = test_theme();
        let body = UiNode::Empty;
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::End,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_vim_j_k() {
        let theme = test_theme();
        let body = UiNode::Container(codegg_protocol::ui::ContainerNode {
            title: None,
            children: (0..20)
                .map(|i| {
                    UiNode::Text(TextNode {
                        text: format!("line {}", i),
                    })
                })
                .collect(),
        });
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme));

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
    fn test_set_theme() {
        let theme1 = test_theme();
        let theme2 = Arc::new(Theme::dark());
        let body = make_text_node("hello");
        let mut dialog = UiNodeDialog::new("id".into(), "Title".into(), body, Arc::clone(&theme1));
        assert!(Arc::ptr_eq(&dialog.theme, &theme1));

        dialog.set_theme(&theme2);
        assert!(Arc::ptr_eq(&dialog.theme, &theme2));
    }
}

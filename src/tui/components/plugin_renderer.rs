use std::sync::Arc;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::theme::Theme;
use codegg_protocol::ui::{
    CodeNode, ContainerNode, KeyValueEntry, KeyValueNode, MarkdownNode, ProgressNode, TableNode,
    TextNode, UiNode,
};

pub struct PluginUiRenderer;

impl PluginUiRenderer {
    pub fn render_node(frame: &mut Frame, area: Rect, theme: &Arc<Theme>, node: &UiNode) {
        match node {
            UiNode::Text(TextNode { text }) => {
                let paragraph = Paragraph::new(text.as_str()).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, area);
            }
            UiNode::Markdown(MarkdownNode { markdown }) => {
                let paragraph = Paragraph::new(markdown.as_str()).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, area);
            }
            UiNode::Code(CodeNode { language, code }) => {
                let title = match language {
                    Some(lang) => format!("[{}]", lang),
                    None => "[code]".to_string(),
                };
                let block = Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.muted));
                let paragraph = Paragraph::new(code.as_str())
                    .block(block)
                    .wrap(Wrap { trim: false });
                frame.render_widget(paragraph, area);
            }
            UiNode::Table(TableNode { columns, rows }) => {
                let lines = Self::format_table_lines(columns, rows);
                let text: Vec<Line> = lines.iter().map(|l| Line::from(l.as_str())).collect();
                let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, area);
            }
            UiNode::KeyValue(KeyValueNode { entries }) => {
                let lines = Self::format_kv_lines(entries);
                let text: Vec<Line> = lines.iter().map(|l| Line::from(l.as_str())).collect();
                let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, area);
            }
            UiNode::Progress(ProgressNode {
                label,
                current,
                total,
            }) => {
                let text = Self::format_progress_text(label, *current, *total);
                let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, area);
            }
            UiNode::Container(ContainerNode { title, children }) => {
                if let Some(title_text) = title {
                    let block = Block::default().borders(Borders::TOP).title(Span::styled(
                        title_text.as_str(),
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD),
                    ));
                    let inner = block.inner(area);
                    frame.render_widget(block, area);
                    if children.is_empty() {
                        return;
                    }
                    let constraints: Vec<Constraint> =
                        children.iter().map(|_| Constraint::Length(1)).collect();
                    let chunks = Layout::vertical(constraints).split(inner);
                    for (child, chunk) in children.iter().zip(chunks.iter()) {
                        Self::render_node(frame, *chunk, theme, child);
                    }
                } else {
                    if children.is_empty() {
                        return;
                    }
                    let constraints: Vec<Constraint> =
                        children.iter().map(|_| Constraint::Length(1)).collect();
                    let chunks = Layout::vertical(constraints).split(area);
                    for (child, chunk) in children.iter().zip(chunks.iter()) {
                        Self::render_node(frame, *chunk, theme, child);
                    }
                }
            }
            UiNode::Empty => {}
            UiNode::Unsupported { unknown_kind, .. } => {
                let text = format!("Unsupported plugin UI node: {}", unknown_kind);
                let span = Span::styled(text, Style::default().fg(theme.warning));
                let line = Line::from(span);
                let paragraph = Paragraph::new(vec![line]);
                frame.render_widget(paragraph, area);
            }
        }
    }

    pub fn node_to_lines(node: &UiNode) -> Vec<String> {
        match node {
            UiNode::Text(TextNode { text }) => {
                vec![text.clone()]
            }
            UiNode::Markdown(MarkdownNode { markdown }) => {
                vec![markdown.clone()]
            }
            UiNode::Code(CodeNode { language, code }) => {
                let mut lines: Vec<String> = Vec::new();
                if let Some(lang) = language {
                    lines.push(format!("// {}", lang));
                }
                lines.extend(code.lines().map(|l| l.to_string()));
                lines
            }
            UiNode::Table(TableNode { columns, rows }) => Self::format_table_lines(columns, rows),
            UiNode::KeyValue(KeyValueNode { entries }) => Self::format_kv_lines(entries),
            UiNode::Progress(ProgressNode {
                label,
                current,
                total,
            }) => {
                vec![Self::format_progress_text(label, *current, *total)]
            }
            UiNode::Container(ContainerNode { title, children }) => {
                let mut lines: Vec<String> = Vec::new();
                if let Some(t) = title {
                    lines.push(format!("{}:", t));
                }
                for child in children {
                    lines.extend(Self::node_to_lines(child));
                }
                lines
            }
            UiNode::Empty => vec![],
            UiNode::Unsupported { unknown_kind, .. } => {
                vec![format!("Unsupported plugin UI node: {}", unknown_kind)]
            }
        }
    }

    fn format_table_lines(columns: &[String], rows: &[Vec<String>]) -> Vec<String> {
        if columns.is_empty() {
            return vec![];
        }
        let col_count = columns.len();
        let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
        for row in rows {
            for (i, cell) in row.iter().enumerate().take(col_count) {
                if cell.len() > widths[i] {
                    widths[i] = cell.len();
                }
            }
        }
        let mut lines: Vec<String> = Vec::new();
        let header: String = columns
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<width$}", c, width = widths[i]))
            .collect::<Vec<_>>()
            .join(" | ");
        lines.push(header);
        let sep: String = widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("-+-");
        lines.push(sep);
        for row in rows {
            let cells: String = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let w = widths.get(i).copied().unwrap_or(0);
                    format!("{:<width$}", cell, width = w)
                })
                .collect::<Vec<_>>()
                .join(" | ");
            lines.push(cells);
        }
        lines
    }

    fn format_kv_lines(entries: &[KeyValueEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| format!("{}: {}", e.key, e.value))
            .collect()
    }

    fn format_progress_text(label: &Option<String>, current: u64, total: Option<u64>) -> String {
        match (label, total) {
            (Some(l), Some(t)) => {
                let pct = if t > 0 {
                    (current as f64 / t as f64 * 100.0) as u64
                } else {
                    0
                };
                format!("{} {}/{} ({}%)", l, current, t, pct)
            }
            (Some(l), None) => format!("{} {}", l, current),
            (None, Some(t)) => {
                let pct = if t > 0 {
                    (current as f64 / t as f64 * 100.0) as u64
                } else {
                    0
                };
                format!("{}/{} ({}%)", current, t, pct)
            }
            (None, None) => format!("{}", current),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Arc<Theme> {
        Arc::new(Theme::default())
    }

    #[test]
    fn text_node_produces_correct_lines() {
        let node = UiNode::Text(TextNode {
            text: "hello world".into(),
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn markdown_node_produces_correct_lines() {
        let node = UiNode::Markdown(MarkdownNode {
            markdown: "# Title".into(),
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["# Title"]);
    }

    #[test]
    fn code_node_with_language_includes_label() {
        let node = UiNode::Code(CodeNode {
            language: Some("rust".into()),
            code: "fn main() {}".into(),
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["// rust", "fn main() {}"]);
    }

    #[test]
    fn code_node_without_language_omits_label() {
        let node = UiNode::Code(CodeNode {
            language: None,
            code: "hello".into(),
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["hello"]);
    }

    #[test]
    fn table_node_formats_header_separator_rows() {
        let node = UiNode::Table(TableNode {
            columns: vec!["Name".into(), "Age".into()],
            rows: vec![
                vec!["Alice".into(), "30".into()],
                vec!["Bob".into(), "25".into()],
            ],
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(
            lines,
            vec!["Name  | Age", "------+----", "Alice | 30 ", "Bob   | 25 ",]
        );
    }

    #[test]
    fn key_value_node_formats_key_value_pairs() {
        let node = UiNode::KeyValue(KeyValueNode {
            entries: vec![
                KeyValueEntry {
                    key: "host".into(),
                    value: "localhost".into(),
                },
                KeyValueEntry {
                    key: "port".into(),
                    value: "8080".into(),
                },
            ],
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["host: localhost", "port: 8080"]);
    }

    #[test]
    fn progress_node_with_total_shows_percentage() {
        let node = UiNode::Progress(ProgressNode {
            label: Some("downloading".into()),
            current: 50,
            total: Some(100),
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["downloading 50/100 (50%)"]);
    }

    #[test]
    fn progress_node_without_total_shows_just_current() {
        let node = UiNode::Progress(ProgressNode {
            label: Some("counting".into()),
            current: 42,
            total: None,
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["counting 42"]);
    }

    #[test]
    fn container_with_title_prepends_title() {
        let node = UiNode::Container(ContainerNode {
            title: Some("MySection".into()),
            children: vec![UiNode::Text(TextNode {
                text: "body".into(),
            })],
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["MySection:", "body"]);
    }

    #[test]
    fn container_with_children_flattens_all() {
        let node = UiNode::Container(ContainerNode {
            title: None,
            children: vec![
                UiNode::Text(TextNode {
                    text: "line1".into(),
                }),
                UiNode::Text(TextNode {
                    text: "line2".into(),
                }),
            ],
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["line1", "line2"]);
    }

    #[test]
    fn empty_node_returns_empty_vec() {
        let node = UiNode::Empty;
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert!(lines.is_empty());
    }

    #[test]
    fn unsupported_node_returns_warning_message() {
        let node = UiNode::Unsupported {
            unknown_kind: "tree".into(),
            data: serde_json::json!({}),
        };
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["Unsupported plugin UI node: tree"]);
    }

    #[test]
    fn nested_containers_work_correctly() {
        let node = UiNode::Container(ContainerNode {
            title: Some("Outer".into()),
            children: vec![
                UiNode::Text(TextNode {
                    text: "inside".into(),
                }),
                UiNode::Container(ContainerNode {
                    title: Some("Inner".into()),
                    children: vec![UiNode::Text(TextNode {
                        text: "deep".into(),
                    })],
                }),
            ],
        });
        let lines = PluginUiRenderer::node_to_lines(&node);
        assert_eq!(lines, vec!["Outer:", "inside", "Inner:", "deep"]);
    }

    fn render_with_node(node: UiNode) {
        let theme = theme();
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                PluginUiRenderer::render_node(frame, area, &theme, &node);
            })
            .unwrap();
    }

    #[test]
    fn render_text_node_does_not_panic() {
        render_with_node(UiNode::Text(TextNode {
            text: "test".into(),
        }));
    }

    #[test]
    fn render_markdown_node_does_not_panic() {
        render_with_node(UiNode::Markdown(MarkdownNode {
            markdown: "# Heading".into(),
        }));
    }

    #[test]
    fn render_code_node_does_not_panic() {
        render_with_node(UiNode::Code(CodeNode {
            language: Some("rust".into()),
            code: "fn main() {}".into(),
        }));
    }

    #[test]
    fn render_code_node_no_language_does_not_panic() {
        render_with_node(UiNode::Code(CodeNode {
            language: None,
            code: "echo hello".into(),
        }));
    }

    #[test]
    fn render_table_node_does_not_panic() {
        render_with_node(UiNode::Table(TableNode {
            columns: vec!["A".into(), "B".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        }));
    }

    #[test]
    fn render_key_value_node_does_not_panic() {
        render_with_node(UiNode::KeyValue(KeyValueNode {
            entries: vec![KeyValueEntry {
                key: "k".into(),
                value: "v".into(),
            }],
        }));
    }

    #[test]
    fn render_progress_node_does_not_panic() {
        render_with_node(UiNode::Progress(ProgressNode {
            label: Some("loading".into()),
            current: 50,
            total: Some(100),
        }));
    }

    #[test]
    fn render_container_node_does_not_panic() {
        render_with_node(UiNode::Container(ContainerNode {
            title: Some("Test".into()),
            children: vec![UiNode::Text(TextNode {
                text: "child".into(),
            })],
        }));
    }

    #[test]
    fn render_container_no_title_does_not_panic() {
        render_with_node(UiNode::Container(ContainerNode {
            title: None,
            children: vec![UiNode::Text(TextNode {
                text: "child".into(),
            })],
        }));
    }

    #[test]
    fn render_container_empty_children_does_not_panic() {
        render_with_node(UiNode::Container(ContainerNode {
            title: Some("Empty".into()),
            children: vec![],
        }));
    }

    #[test]
    fn render_empty_node_does_not_panic() {
        render_with_node(UiNode::Empty);
    }

    #[test]
    fn render_unsupported_node_does_not_panic() {
        render_with_node(UiNode::Unsupported {
            unknown_kind: "widget".into(),
            data: serde_json::json!({}),
        });
    }
}

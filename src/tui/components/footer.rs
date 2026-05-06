use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use std::collections::HashMap;
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

use super::super::input::InputAction;
use super::super::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FooterPriority {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

struct FooterItem<'a> {
    spans: Vec<Span<'a>>,
    priority: FooterPriority,
}

pub struct FooterWidget {
    pub theme: Arc<Theme>,
    pub status: String,
    pub tokens: String,
    pub keybinds: String,
    pub context_hint: String,
    pub subagent_count: usize,
    pub subagent_names: Vec<String>,
    pub subagent_work: Option<String>,
    pub loading: bool,
    pub loading_label: Option<String>,
    pub thinking: bool,
    pub thinking_label: Option<String>,
    pub tts_enabled: bool,
    pub tts_speaking: bool,
}

impl FooterWidget {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            status: "idle".to_string(),
            tokens: String::new(),
            keybinds: "/:prompt  ?:help  ^L:model  ^N:new  ^T:sidebar  ^W:close  Tab:agent"
                .to_string(),
            context_hint: String::new(),
            subagent_count: 0,
            subagent_names: Vec::new(),
            subagent_work: None,
            loading: false,
            loading_label: None,
            thinking: false,
            thinking_label: None,
            tts_enabled: false,
            tts_speaking: false,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_status(&mut self, status: String) {
        self.status = status;
    }

    pub fn set_tokens(&mut self, tokens: String) {
        self.tokens = tokens;
    }

    pub fn set_subagents(&mut self, count: usize, names: Vec<String>) {
        self.subagent_count = count;
        self.subagent_names = names;
    }

    pub fn set_loading(&mut self, loading: bool, label: Option<String>) {
        self.loading = loading;
        self.loading_label = label;
    }

    pub fn set_thinking(&mut self, thinking: bool, label: Option<String>) {
        self.thinking = thinking;
        self.thinking_label = label;
    }

    pub fn set_subagent_work(&mut self, name: Option<String>) {
        self.subagent_work = name;
    }

    pub fn set_tts(&mut self, enabled: bool, speaking: bool) {
        self.tts_enabled = enabled;
        self.tts_speaking = speaking;
    }

    pub fn set_context_hint(&mut self, hint: String) {
        self.context_hint = hint;
    }

    pub fn set_undo_message(&mut self, msg: &str) {
        self.context_hint = format!("{} | Press U to undo", msg);
    }

    pub fn clear_undo_message(&mut self) {
        self.context_hint.clear();
    }

    pub fn update_keybinds(&mut self, bindings: &HashMap<(KeyModifiers, KeyCode), InputAction>) {
        let important_actions = [
            (InputAction::FocusPrompt, "/"),
            (InputAction::Help, "?"),
            (InputAction::SelectModel, "^L"),
            (InputAction::NewSession, "^N"),
            (InputAction::ToggleSidebar, "^T"),
            (InputAction::CloseSession, "^W"),
            (InputAction::SwitchAgent, "Tab"),
        ];

        let parts: Vec<String> = important_actions
            .iter()
            .filter(|(action, _)| bindings.values().any(|a| a == action))
            .map(|(action, default_key)| {
                let key_str = bindings
                    .iter()
                    .find(|(_, a)| **a == *action)
                    .map(|(k, _)| format_key(k))
                    .unwrap_or_else(|| default_key.to_string());
                key_str
            })
            .collect();

        if parts.is_empty() {
            self.keybinds = String::new();
        } else {
            self.keybinds = parts.join("  ");
        }
    }
}

fn format_key(key: &(KeyModifiers, KeyCode)) -> String {
    let mut s = String::new();
    if key.0.contains(KeyModifiers::CONTROL) {
        s.push('^');
    }
    if key.0.contains(KeyModifiers::SHIFT) {
        s.push_str("Shift+");
    }
    if key.0.contains(KeyModifiers::ALT) {
        s.push_str("Alt+");
    }
    match key.1 {
        KeyCode::Char(c) => s.push(c.to_ascii_uppercase()),
        KeyCode::Enter => s.push_str("Enter"),
        KeyCode::Esc => s.push_str("Esc"),
        KeyCode::Tab => s.push_str("Tab"),
        KeyCode::Up => s.push_str("Up"),
        KeyCode::Down => s.push_str("Down"),
        KeyCode::Left => s.push_str("Left"),
        KeyCode::Right => s.push_str("Right"),
        KeyCode::PageUp => s.push_str("PgUp"),
        KeyCode::PageDown => s.push_str("PgDown"),
        KeyCode::Home => s.push_str("Home"),
        KeyCode::End => s.push_str("End"),
        KeyCode::Backspace => s.push_str("Backspace"),
        KeyCode::Delete => s.push_str("Delete"),
        _ => s.push('?'),
    }
    s
}

impl Default for FooterWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &FooterWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let status_style = match self.status.as_str() {
            "working" => Style::default()
                .fg(self.theme.warning)
                .add_modifier(Modifier::BOLD),
            "error" => Style::default()
                .fg(self.theme.error)
                .add_modifier(Modifier::BOLD),
            _ => Style::default().fg(self.theme.muted),
        };

        let mut items: Vec<FooterItem<'_>> = Vec::new();

        items.push(FooterItem {
            spans: vec![
                Span::styled(format!(" {} ", self.status), status_style),
                Span::raw("  "),
            ],
            priority: if self.status == "error" {
                FooterPriority::Critical
            } else {
                FooterPriority::High
            },
        });

        items.push(FooterItem {
            spans: vec![Span::styled(
                &self.tokens,
                Style::default().fg(self.theme.muted),
            )],
            priority: FooterPriority::Low,
        });

        if self.loading {
            let label_str = self.loading_label.as_deref().unwrap_or("loading");
            items.push(FooterItem {
                spans: vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("⟳ {label_str}"),
                        Style::default()
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
                priority: FooterPriority::High,
            });
        }

        if self.thinking {
            let label_str = self.thinking_label.as_deref().unwrap_or("Thinking...");
            items.push(FooterItem {
                spans: vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("◌ {label_str}"),
                        Style::default()
                            .fg(self.theme.warning)
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
                priority: FooterPriority::High,
            });
        }

        if self.subagent_count > 0 {
            let subagent_label = if self.subagent_count == 1 {
                "subagent"
            } else {
                "subagents"
            };
            let mut subagent_spans = vec![
                Span::raw("  "),
                Span::styled(
                    format!("{} {}", self.subagent_count, subagent_label),
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                ),
            ];

            if self.subagent_names.len() <= 3 {
                for name in &self.subagent_names {
                    subagent_spans.push(Span::raw(" "));
                    subagent_spans.push(Span::styled(
                        format!("[{name}]"),
                        Style::default().fg(self.theme.muted),
                    ));
                }
            } else {
                let preview: Vec<&String> = self.subagent_names.iter().take(2).collect();
                let names_str = preview
                    .iter()
                    .map(|n| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                subagent_spans.push(Span::raw(" "));
                subagent_spans.push(Span::styled(
                    format!("[{names_str}, +{}]", self.subagent_names.len() - 2),
                    Style::default().fg(self.theme.muted),
                ));
            }

            items.push(FooterItem {
                spans: subagent_spans,
                priority: FooterPriority::Medium,
            });
        }

        if let Some(work_name) = &self.subagent_work {
            items.push(FooterItem {
                spans: vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("⟳ {work_name}"),
                        Style::default()
                            .fg(self.theme.warning)
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
                priority: FooterPriority::High,
            });
        }

        if self.tts_enabled {
            let icon = if self.tts_speaking { "🔊" } else { "🔇" };
            items.push(FooterItem {
                spans: vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{icon} TTS"),
                        Style::default()
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
                priority: FooterPriority::Low,
            });
        }

        if !self.context_hint.is_empty() {
            items.push(FooterItem {
                spans: vec![
                    Span::raw("  "),
                    Span::styled(
                        &self.context_hint,
                        Style::default()
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ],
                priority: FooterPriority::Medium,
            });
        }

        if !self.keybinds.is_empty() {
            items.push(FooterItem {
                spans: vec![Span::styled(
                    &self.keybinds,
                    Style::default().fg(self.theme.muted),
                )],
                priority: FooterPriority::Low,
            });
        }

        items.sort_by_key(|i| i.priority);

        let mut left_spans: Vec<Span<'_>> = Vec::new();
        let mut right_spans: Vec<Span<'_>> = Vec::new();
        let mut current_width: usize = 0;
        let total_width = area.width as usize;

        for item in &items {
            let item_width: usize = item.spans.iter().map(|s| s.width()).sum();
            if current_width + item_width <= total_width {
                left_spans.extend(item.spans.clone());
                current_width += item_width;
            } else if item.priority == FooterPriority::Low && right_spans.is_empty() {
                let remaining = total_width.saturating_sub(current_width);
                if remaining > 4 {
                    let mut truncated = String::new();
                    for s in &item.spans {
                        for c in s.content.chars() {
                            let char_width = UnicodeWidthStr::width(c.to_string().as_str());
                            if truncated.width() + char_width <= remaining.saturating_sub(2) {
                                truncated.push(c);
                            } else {
                                break;
                            }
                        }
                    }
                    if !truncated.is_empty() {
                        right_spans.push(Span::styled(
                            format!(" {}", truncated),
                            Style::default().fg(self.theme.muted),
                        ));
                    }
                }
                break;
            } else {
                break;
            }
        }

        let left_width: usize = left_spans.iter().map(|s| s.width()).sum();
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
        let pad = area.width.saturating_sub((left_width + right_width) as u16);

        let mut all_spans = left_spans;
        all_spans.push(Span::raw(" ".repeat(pad as usize)));
        all_spans.extend(right_spans);

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(self.theme.border));

        let line = Line::from(all_spans);
        let paragraph = Paragraph::new(line).block(block);
        paragraph.render(area, buf);
    }
}

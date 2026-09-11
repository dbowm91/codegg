#![allow(clippy::collapsible_match)]

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use std::collections::HashMap;
use std::sync::Arc;

use super::super::super::input::{ActionKey, KeybindConfig};
use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;

#[derive(Clone)]
pub struct KeybindDialog {
    pub theme: Arc<Theme>,
    pub bindings: HashMap<String, ActionKey>,
    pub selected: usize,
    pub waiting_for_key: Option<usize>,
    pub mode: KeybindMode,
    pub export_text: String,
    pub import_text: String,
    pub conflict: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeybindMode {
    Normal,
    WaitingForKey,
    Export,
    Import,
}

impl KeybindDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            bindings: HashMap::new(),
            selected: 0,
            waiting_for_key: None,
            mode: KeybindMode::Normal,
            export_text: String::new(),
            import_text: String::new(),
            conflict: None,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_bindings(&mut self, bindings: HashMap<String, ActionKey>) {
        self.bindings = bindings;
    }

    pub fn actions() -> Vec<ActionKey> {
        ActionKey::all().to_vec()
    }

    pub fn action_name(action: &ActionKey) -> &'static str {
        action.label()
    }

    pub fn get_binding(&self, action: &ActionKey) -> Option<String> {
        self.bindings
            .iter()
            .find(|(_, v)| *v == action)
            .map(|(k, _)| k.clone())
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_down(&mut self) {
        let max = Self::actions().len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    pub fn start_remap(&mut self) {
        self.waiting_for_key = Some(self.selected);
        self.mode = KeybindMode::WaitingForKey;
        self.conflict = None;
    }

    pub fn cancel_remap(&mut self) {
        self.waiting_for_key = None;
        self.mode = KeybindMode::Normal;
        self.conflict = None;
    }

    pub fn reset_to_defaults(&mut self) {
        self.bindings.clear();
        self.selected = 0;
        self.mode = KeybindMode::Normal;
        self.conflict = None;
    }

    pub fn start_export(&mut self) {
        let mut items: Vec<String> = Vec::new();
        for (key, action) in &self.bindings {
            let action_name = serde_json::to_string(action).unwrap_or_default();
            items.push(format!(
                "        {}: {}",
                serde_json::to_string(key).unwrap_or_default(),
                action_name
            ));
        }
        self.export_text = format!(
            "{{\n    \"bindings\": {{\n{}\n    }}\n}}",
            items.join(",\n")
        );
        self.mode = KeybindMode::Export;
    }

    pub fn start_import(&mut self) {
        self.import_text.clear();
        self.mode = KeybindMode::Import;
    }

    pub fn apply_import(&mut self) -> Result<(), String> {
        let config: KeybindConfig =
            serde_json::from_str(&self.import_text).map_err(|e| e.to_string())?;
        self.bindings = config.bindings;
        self.mode = KeybindMode::Normal;
        Ok(())
    }

    pub fn cancel_mode(&mut self) {
        self.mode = KeybindMode::Normal;
        self.conflict = None;
    }

    pub fn set_conflict(&mut self, key: &str) {
        self.conflict = Some(key.to_string());
    }

    pub fn clear_conflict(&mut self) {
        self.conflict = None;
    }

    /// Apply the currently selected remap while rejecting collisions instead
    /// of silently overwriting another action's binding.
    pub fn bind_waiting_key(&mut self, key_str: String) -> Result<(), String> {
        let Some(action_idx) = self.waiting_for_key else {
            return Ok(());
        };
        let actions = Self::actions();
        let Some(action) = actions.get(action_idx).copied() else {
            return Err("invalid keybinding action selection".to_string());
        };

        if let Some((_, other_action)) = self
            .bindings
            .iter()
            .find(|(key, value)| *key == &key_str && **value != action)
        {
            return Err(format!(
                "{} is already bound to {}",
                key_str,
                Self::action_name(other_action)
            ));
        }

        if let Some(existing_key) = self
            .bindings
            .iter()
            .find(|(_, value)| **value == action)
            .map(|(key, _)| key.clone())
        {
            if existing_key != key_str {
                self.bindings.remove(&existing_key);
            }
        }
        self.bindings.insert(key_str, action);
        Ok(())
    }

    fn format_key_event_for_dialog(&self, key: &crossterm::event::KeyEvent) -> String {
        use crossterm::event::KeyCode;
        let mods = key.modifiers;
        let mut parts = Vec::new();
        if mods.contains(crossterm::event::KeyModifiers::CONTROL) {
            parts.push("ctrl".to_string());
        }
        if mods.contains(crossterm::event::KeyModifiers::SHIFT) {
            parts.push("shift".to_string());
        }
        if mods.contains(crossterm::event::KeyModifiers::ALT) {
            parts.push("alt".to_string());
        }
        let key_part = match key.code {
            KeyCode::Enter => "enter",
            KeyCode::Esc => "esc",
            KeyCode::Tab => "tab",
            KeyCode::Backspace => "backspace",
            KeyCode::Delete => "delete",
            KeyCode::Left => "left",
            KeyCode::Right => "right",
            KeyCode::Up => "up",
            KeyCode::Down => "down",
            KeyCode::Home => "home",
            KeyCode::End => "end",
            KeyCode::PageUp => "pageup",
            KeyCode::PageDown => "pagedown",
            KeyCode::Char(' ') => "space",
            KeyCode::Char(c) => {
                return format!(
                    "{}{}",
                    if parts.is_empty() {
                        String::new()
                    } else {
                        format!("{}+", parts.join("+"))
                    },
                    c.to_lowercase()
                )
            }
            _ => return parts.join("+"),
        };
        if !parts.is_empty() {
            format!("{}+{}", parts.join("+"), key_part)
        } else {
            key_part.to_string()
        }
    }

    fn format_key(key_str: &str) -> String {
        let parts: Vec<&str> = key_str.split('+').collect();
        let key_part = parts.last().unwrap_or(&"");
        let modifiers: Vec<&str> = parts[..parts.len().saturating_sub(1)].to_vec();

        let mut formatted = Vec::new();
        for modifer in modifiers {
            match modifer {
                "ctrl" | "control" => formatted.push("Ctrl".to_string()),
                "shift" => formatted.push("Shift".to_string()),
                "alt" => formatted.push("Alt".to_string()),
                _ => {}
            }
        }

        let key_display = match *key_part {
            "enter" => "Enter",
            "esc" => "Esc",
            "tab" => "Tab",
            "backspace" => "Backspace",
            "delete" => "Delete",
            "left" => "Left",
            "right" => "Right",
            "up" => "Up",
            "down" => "Down",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            "space" => "Space",
            c if c.len() == 1 => &c.to_uppercase(),
            c => c,
        };
        formatted.push(key_display.to_string());

        formatted.join("+")
    }
}

impl Default for KeybindDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &KeybindDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        match self.mode {
            KeybindMode::Normal | KeybindMode::WaitingForKey => {
                lines.push(Line::from(Span::styled(
                    " Keybindings ",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));

                let actions = KeybindDialog::actions();
                for (i, action) in actions.iter().enumerate() {
                    let is_selected = i == self.selected;
                    let is_waiting = self.waiting_for_key == Some(i);

                    let binding = self.get_binding(action);
                    let binding_display = binding
                        .map(|b| KeybindDialog::format_key(&b))
                        .unwrap_or_else(|| "(unbound)".to_string());

                    let style = if is_waiting {
                        Style::default()
                            .fg(self.theme.primary)
                            .bg(self.theme.selection)
                            .add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default()
                            .fg(self.theme.primary)
                            .bg(self.theme.selection)
                    } else {
                        Style::default().fg(self.theme.foreground)
                    };

                    let marker = if is_waiting { "» " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(marker.to_string(), Style::default().fg(self.theme.muted)),
                        Span::styled(format!("{:<20}", KeybindDialog::action_name(action)), style),
                        Span::styled(
                            format!(" {}", binding_display),
                            Style::default().fg(self.theme.muted),
                        ),
                    ]));
                }

                if let Some(ref conflict) = self.conflict {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Conflict: {} is already bound!", conflict),
                        Style::default().fg(self.theme.error),
                    )));
                }

                lines.push(Line::from(""));
                if self.waiting_for_key.is_some() {
                    lines.push(Line::from(Span::styled(
                        "Press a key to bind, Esc to cancel...",
                        Style::default().fg(self.theme.muted),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        "Enter remap  |  R reset  |  Esc close",
                        Style::default().fg(self.theme.muted),
                    )));
                }
            }
            KeybindMode::Export => {
                lines.push(Line::from(Span::styled(
                    " Export Keybindings ",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Copy the JSON below:",
                    Style::default().fg(self.theme.muted),
                )));
                lines.push(Line::from(""));

                for line in self.export_text.lines() {
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default().fg(self.theme.foreground),
                    )));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Press Esc to close",
                    Style::default().fg(self.theme.muted),
                )));
            }
            KeybindMode::Import => {
                lines.push(Line::from(Span::styled(
                    " Import Keybindings ",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Paste JSON config and press Enter:",
                    Style::default().fg(self.theme.muted),
                )));
                lines.push(Line::from(""));

                for (_i, line) in self.import_text.lines().enumerate().take(10) {
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default().fg(self.theme.foreground),
                    )));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Enter import  Esc cancel",
                    Style::default().fg(self.theme.muted),
                )));
            }
        }

        let block = Block::default()
            .title(" Keybinds ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}

impl Component for KeybindDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                if self.mode != KeybindMode::Normal {
                    self.cancel_mode();
                } else {
                    return Some(TuiMsg::CloseDialog);
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
            }
            crossterm::event::KeyCode::Enter => match self.mode {
                KeybindMode::Normal => {
                    self.start_remap();
                }
                KeybindMode::Export | KeybindMode::Import => {
                    self.cancel_mode();
                }
                KeybindMode::WaitingForKey => {
                    let key_str = self.format_key_event_for_dialog(&key);
                    if let Some(action_idx) = self.waiting_for_key {
                        let actions = Self::actions();
                        if action_idx < actions.len() {
                            let action = &actions[action_idx];
                            let existing_key_for_action = self
                                .bindings
                                .iter()
                                .find(|(_, val)| *val == action)
                                .map(|(k, _)| k.clone());

                            if let Some(ref existing_key) = existing_key_for_action {
                                if existing_key != &key_str {
                                    self.bindings.remove(existing_key);
                                }
                            }

                            let current_binding_for_action = self.get_binding(action);
                            if current_binding_for_action.as_ref() != Some(&key_str) {
                                self.bindings.insert(key_str.clone(), *action);
                            }
                            self.clear_conflict();
                            return Some(TuiMsg::KeybindChanged {
                                action: Self::action_name(action).to_string(),
                                binding: key_str,
                            });
                        }
                    }
                    self.waiting_for_key = None;
                    self.mode = KeybindMode::Normal;
                }
            },
            crossterm::event::KeyCode::Char('r') | crossterm::event::KeyCode::Char('R') => {
                if self.mode == KeybindMode::Normal {
                    self.reset_to_defaults();
                }
            }
            crossterm::event::KeyCode::Char('e') | crossterm::event::KeyCode::Char('E') => {
                if self.mode == KeybindMode::Normal {
                    self.start_export();
                }
            }
            crossterm::event::KeyCode::Char('i') | crossterm::event::KeyCode::Char('I') => {
                if self.mode == KeybindMode::Normal {
                    self.start_import();
                }
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, theme: &Arc<Theme>) {
        let mut lines: Vec<Line> = Vec::new();

        match self.mode {
            KeybindMode::Normal | KeybindMode::WaitingForKey => {
                lines.push(Line::from(Span::styled(
                    " Keybindings ",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));

                let actions = KeybindDialog::actions();
                for (i, action) in actions.iter().enumerate() {
                    let is_selected = i == self.selected;
                    let is_waiting = self.waiting_for_key == Some(i);

                    let binding = self.get_binding(action);
                    let binding_display = binding
                        .map(|b| KeybindDialog::format_key(&b))
                        .unwrap_or_else(|| "(unbound)".to_string());

                    let style = if is_waiting {
                        Style::default()
                            .fg(theme.primary)
                            .bg(theme.selection)
                            .add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default().fg(theme.primary).bg(theme.selection)
                    } else {
                        Style::default().fg(theme.foreground)
                    };

                    let marker = if is_waiting { "» " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(marker.to_string(), Style::default().fg(theme.muted)),
                        Span::styled(format!("{:<20}", KeybindDialog::action_name(action)), style),
                        Span::styled(
                            format!(" {}", binding_display),
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                }

                if let Some(ref conflict) = self.conflict {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Conflict: {} is already bound!", conflict),
                        Style::default().fg(theme.error),
                    )));
                }

                lines.push(Line::from(""));
                if self.waiting_for_key.is_some() {
                    lines.push(Line::from(Span::styled(
                        "Press a key to bind, Esc to cancel...",
                        Style::default().fg(theme.muted),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        "Enter remap  |  R reset  |  Esc close",
                        Style::default().fg(theme.muted),
                    )));
                }
            }
            KeybindMode::Export => {
                lines.push(Line::from(Span::styled(
                    " Export Keybindings ",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Copy the JSON below:",
                    Style::default().fg(theme.muted),
                )));
                lines.push(Line::from(""));

                for line in self.export_text.lines() {
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default().fg(theme.foreground),
                    )));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Press Esc to close",
                    Style::default().fg(theme.muted),
                )));
            }
            KeybindMode::Import => {
                lines.push(Line::from(Span::styled(
                    " Import Keybindings ",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Paste JSON config and press Enter:",
                    Style::default().fg(theme.muted),
                )));
                lines.push(Line::from(""));

                for (_i, line) in self.import_text.lines().enumerate().take(10) {
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default().fg(theme.foreground),
                    )));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Enter import  Esc cancel",
                    Style::default().fg(theme.muted),
                )));
            }
        }

        let block = Block::default()
            .title(" Keybinds ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Keybind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_list_matches_canonical_descriptors() {
        assert_eq!(KeybindDialog::actions(), ActionKey::all().to_vec());
        assert!(ActionKey::all()
            .iter()
            .all(|action| !KeybindDialog::action_name(action).is_empty()));
    }

    #[test]
    fn remap_rejects_collision_without_overwriting() {
        let mut dialog = KeybindDialog::default();
        dialog
            .bindings
            .insert("ctrl+a".to_string(), ActionKey::Send);
        dialog
            .bindings
            .insert("ctrl+b".to_string(), ActionKey::Cancel);
        dialog.selected = ActionKey::all()
            .iter()
            .position(|action| *action == ActionKey::Cancel)
            .unwrap();
        dialog.start_remap();

        let error = dialog.bind_waiting_key("ctrl+a".to_string()).unwrap_err();
        assert!(error.contains("Send"));
        assert_eq!(dialog.bindings.get("ctrl+a"), Some(&ActionKey::Send));
        assert_eq!(dialog.bindings.get("ctrl+b"), Some(&ActionKey::Cancel));
    }

    #[test]
    fn export_uses_config_compatible_snake_case_action_names() {
        let mut dialog = KeybindDialog::default();
        dialog
            .bindings
            .insert("ctrl+e".to_string(), ActionKey::ExternalEditor);
        dialog.start_export();
        assert!(dialog.export_text.contains("external_editor"));
        let parsed: KeybindConfig = serde_json::from_str(&dialog.export_text).unwrap();
        assert_eq!(
            parsed.bindings.get("ctrl+e"),
            Some(&ActionKey::ExternalEditor)
        );
    }
}

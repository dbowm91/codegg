#![allow(clippy::collapsible_match)]

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui::Frame;
use std::collections::HashMap;
use std::sync::Arc;

use super::super::super::input::{ActionKey, KeybindConfig};
use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::ListCursor;
use super::super::section::{Section, SectionFrame};
use crate::tui::app::TuiMsg;
use crate::tui::components::hint_line::HintLine;

/// The verbs this box implements in Normal mode, for the panel host to reuse
/// verbatim.
///
/// `handle_key` below is the implementation and this is the advertisement, so
/// the spelling is the lowercase one throughout: it accepts `R`/`E`/`I` too,
/// but `E` and `I` are bound by `insert_panel_direction_bindings` as panel
/// directions and are answered by `SidePanelHost::handle_key` before this box
/// is ever consulted, so `export`/`import` are only reachable from a column
/// through the lowercase spelling. `Esc`, `↑`/`↓` and `q` are absent for the
/// same reason — they are the host's directions and mode switch, and a hosted
/// hint cannot promise a key whose meaning differs between the two surfaces.
fn hint() -> HintLine {
    let mut hint = HintLine::new();
    hint.push("j/k", "move")
        .push("enter", "remap")
        .push("r", "reset")
        .push("e", "export")
        .push("i", "import");
    hint
}

fn mode_footer(mode: &KeybindMode) -> HintLine {
    match mode {
        KeybindMode::Normal => {
            HintLine::from_items(&[("Esc", "close"), ("enter", "remap"), ("r", "reset")])
        }
        KeybindMode::WaitingForKey => HintLine::from_items(&[("Esc", "cancel")]),
        KeybindMode::Export => HintLine::from_items(&[("Esc", "back")]),
        KeybindMode::Import => HintLine::from_items(&[("Esc", "cancel"), ("Enter", "import")]),
    }
}

#[derive(Clone)]
pub struct KeybindDialog {
    pub theme: Arc<Theme>,
    pub bindings: HashMap<String, ActionKey>,
    pub cursor: ListCursor,
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
            cursor: ListCursor::new(),
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
        self.cursor.up(Self::actions().len(), 0);
    }

    pub fn select_down(&mut self) {
        self.cursor.down(Self::actions().len(), 0);
    }

    pub fn start_remap(&mut self) {
        self.waiting_for_key = Some(self.cursor.index(Self::actions().len()));
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
        self.cursor.reset();
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

    /// One listing routine for both render paths.
    ///
    /// A docked column never saw the key they named: `Esc` — the only verb
    /// either the Export or the Import footer offered — is the mode switch,
    /// so in a column it means something else, and those two footers spent a
    /// row in a column too narrow to wrap them onto a panel that taught a
    /// dead key. They are now `SectionFrame::Owned`, where they are still the way a
    /// modal tells a human what Esc does.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: SectionFrame) {
        let mut lines: Vec<Line> = Vec::new();

        match self.mode {
            KeybindMode::Normal | KeybindMode::WaitingForKey => {
                let actions = KeybindDialog::actions();
                let selected = self.cursor.index(actions.len());
                for (i, action) in actions.iter().enumerate() {
                    let is_selected = i == selected;
                    let is_waiting = self.waiting_for_key == Some(i);

                    let binding = self.get_binding(action);
                    let binding_display = binding
                        .map(|b| KeybindDialog::format_key(&b))
                        .unwrap_or_else(|| "(unbound)".to_string());

                    // `selection_text` is derived to read on `selection`;
                    // `primary` is an accent and fails that fill on 14 themes.
                    let style = if is_waiting {
                        Style::default()
                            .fg(theme.selection_text)
                            .bg(theme.selection)
                            .add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default()
                            .fg(theme.selection_text)
                            .bg(theme.selection)
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

                if chrome == SectionFrame::Owned && self.waiting_for_key.is_some() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Press a key to bind, Esc to cancel...",
                        Style::default().fg(theme.muted),
                    )));
                }
            }
            KeybindMode::Export => {
                if chrome == SectionFrame::Owned {
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
                }

                for line in self.export_text.lines() {
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default().fg(theme.foreground),
                    )));
                }
            }
            KeybindMode::Import => {
                if chrome == SectionFrame::Owned {
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
                }

                for (_i, line) in self.import_text.lines().enumerate().take(10) {
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default().fg(theme.foreground),
                    )));
                }
            }
        }

        match chrome {
            SectionFrame::Owned => {
                let chunks = Layout::vertical([
                    Constraint::Min(1),
                    Constraint::Length(Section::owned_height(1)),
                ])
                .split(area);
                let inner = Section::new("Keybinds", theme)
                    .focused(true)
                    .draw(chunks[0], frame.buffer_mut());
                Paragraph::new(lines)
                    .wrap(Wrap { trim: true })
                    .render(inner, frame.buffer_mut());

                let footer_inner = Section::plain(theme).draw(chunks[1], frame.buffer_mut());
                let text = mode_footer(&self.mode).render(usize::from(footer_inner.width));
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        text,
                        Style::default().fg(theme.muted),
                    ))),
                    footer_inner,
                );
            }
            SectionFrame::Hosted => {
                let paragraph = Paragraph::new(lines)
                    .style(Style::default().bg(theme.background))
                    .wrap(Wrap { trim: true });
                paragraph.render(area, frame.buffer_mut());
            }
        }
    }
}

impl Default for KeybindDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Component for KeybindDialog {
    /// A plain viewer box: no composer and no live filter of its own, so the
    /// panel host may resolve `Shift+←/→/↑/↓` as the docking gesture before
    /// this dialog's own handler sees the key.
    fn docks_as_viewer(&self) -> bool {
        true
    }
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

    /// The verbs this box owns in the mode it is currently in, or nothing at
    /// all when that mode has none a column can deliver.
    ///
    /// [`KeybindMode::Export`] and [`KeybindMode::Import`] are exactly that
    /// case: their body is JSON to copy or paste, and the only key either one
    /// dispatches is `Esc` — which is the mode switch, so it means "close"
    /// modally and something else in a column. Naming a verb for them
    /// would advertise a key that cannot fire, so these two modes say nothing
    /// and let the host print its own line. The real defect is that a docked
    /// `/keybinds` cannot leave those modes at all, and that is a host-level
    /// exit question already recorded under milestone M002a; answering it here
    /// would mean adding a key rather than removing a lie.
    fn keymap_hint_line(&self) -> Option<HintLine> {
        match self.mode {
            KeybindMode::Normal | KeybindMode::WaitingForKey => Some(hint()),
            KeybindMode::Export | KeybindMode::Import => None,
        }
    }

    /// The binding list alone: no border, no title, no per-mode footer.
    ///
    /// The host has already painted a border, a ` ● Keybinds ` title and the
    /// `●` focus marker into this same rect, and `render_in_panel` is handed the
    /// full rect rather than the interior, so a second border does not stack —
    /// it overwrites all three, and a focused column renders exactly like an
    /// unfocused one. The footers go with it: all three describe `Esc`, which is
    /// the host's. `area` is the host's full rect, so inset by its border first.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = Section::plain(theme)
            .frame(SectionFrame::Hosted)
            .draw(area, frame.buffer_mut());
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, SectionFrame::Hosted);
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, SectionFrame::Owned);
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
        dialog.cursor.set_index(
            ActionKey::all()
                .iter()
                .position(|action| *action == ActionKey::Cancel)
                .unwrap(),
        );
        dialog.start_remap();

        let error = dialog.bind_waiting_key("ctrl+a".to_string()).unwrap_err();
        assert!(error.contains("Send"));
        assert_eq!(dialog.bindings.get("ctrl+a"), Some(&ActionKey::Send));
        assert_eq!(dialog.bindings.get("ctrl+b"), Some(&ActionKey::Cancel));
    }

    #[test]
    fn keybind_cursor_clamps_at_both_ends_and_never_wraps() {
        use crate::tui::components::scroll::ScrollPolicy;

        let mut dialog = KeybindDialog::default();
        let len = KeybindDialog::actions().len();
        let last = len - 1;

        dialog.select_up();
        assert_eq!(
            dialog.cursor.index(len),
            0,
            "ScrollPolicy::Clamp must stop on the first action, not wrap to the last"
        );

        dialog.cursor.set_index(last);
        dialog.select_down();
        assert_eq!(
            dialog.cursor.index(len),
            last,
            "ScrollPolicy::Clamp must stop on the last action, not wrap to the first"
        );
        assert_eq!(dialog.cursor.policy(), ScrollPolicy::Clamp);
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

    fn rendered_rows(dialog: &KeybindDialog, width: u16, height: u16) -> Vec<String> {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let theme = Arc::new(Theme::dark());
        let mut dialog = dialog.clone();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| Component::render(&mut dialog, frame, frame.area(), &theme))
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

    fn painted_footer(dialog: &KeybindDialog, width: u16, height: u16) -> String {
        let rows = rendered_rows(dialog, width, height);
        rows.into_iter()
            .find(|row| {
                row.contains("Esc close") || row.contains("Esc cancel") || row.contains("Esc back")
            })
            .unwrap_or_else(|| panic!("no footer row painted at {width}x{height}"))
    }

    #[test]
    fn every_mode_paints_its_row_in_the_reserved_chunk() {
        let normal = KeybindDialog::default();
        assert!(
            painted_footer(&normal, 100, 30).contains("Esc close · enter remap · r reset"),
            "Normal must paint its row"
        );

        let mut waiting = KeybindDialog::default();
        waiting.start_remap();
        let painted = painted_footer(&waiting, 100, 30);
        assert!(
            painted.contains("Esc cancel"),
            "WaitingForKey's Esc cancels the remap and the row must say so: {painted:?}"
        );
        assert!(
            !painted.contains("Esc close"),
            "WaitingForKey's Esc does not close the box: {painted:?}"
        );

        let mut export = KeybindDialog::default();
        export.start_export();
        let painted = painted_footer(&export, 100, 30);
        assert!(
            painted.contains("Esc back"),
            "Export's row must be a row, not prose: {painted:?}"
        );
        assert!(
            !painted.contains("Press Esc to close"),
            "the prose footer must be gone: {painted:?}"
        );

        let mut import = KeybindDialog::default();
        import.start_import();
        assert!(
            painted_footer(&import, 100, 30).contains("Esc cancel · Enter import"),
            "Import must paint its row"
        );
    }

    #[test]
    fn the_minimum_size_keeps_one_body_row() {
        let dialog = KeybindDialog::default();
        let rows = rendered_rows(&dialog, 20, 6);
        assert!(
            rows.len() == 6 && rows[1].chars().any(|c| c.is_alphabetic()),
            "the body must keep one painted row at 20x6: {rows:#?}"
        );
        assert!(
            rows[4].contains("Esc close") || rows[5].contains("Esc close"),
            "the footer must still paint at 20x6: {rows:#?}"
        );
    }
}

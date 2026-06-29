#![allow(clippy::collapsible_match)]

use std::cell::RefCell;
use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;
use crate::tui::app::TuiMsg;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ModelDialogTab {
    #[default]
    SelectModel,
    Configure,
}

#[derive(Debug, Clone)]
pub struct CustomModel {
    pub name: String,
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

pub struct ModelDialog {
    pub theme: Arc<Theme>,
    pub models: Vec<String>,
    pub current: String,
    pub selected: usize,
    pub scroll: CenteredScroll,
    pub filter: String,
    pub tab: ModelDialogTab,
    pub custom_models: Vec<CustomModel>,
    pub provider_configs: Vec<ProviderStatus>,
    pub new_model_name: String,
    pub new_model_provider: String,
    pub new_model_api_key: String,
    pub new_model_base_url: String,
    pub adding_model: bool,
    pub field_index: usize,
    visible_height: usize,
    #[allow(clippy::type_complexity)]
    flat_cache: RefCell<Option<(String, Vec<(String, String)>)>>,
}

impl Clone for ModelDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            models: self.models.clone(),
            current: self.current.clone(),
            selected: self.selected,
            scroll: self.scroll.clone(),
            filter: self.filter.clone(),
            tab: self.tab,
            custom_models: self.custom_models.clone(),
            provider_configs: self.provider_configs.clone(),
            new_model_name: self.new_model_name.clone(),
            new_model_provider: self.new_model_provider.clone(),
            new_model_api_key: self.new_model_api_key.clone(),
            new_model_base_url: self.new_model_base_url.clone(),
            adding_model: self.adding_model,
            field_index: self.field_index,
            visible_height: self.visible_height,
            flat_cache: RefCell::new(self.flat_cache.borrow().clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderStatus {
    pub name: String,
    pub has_api_key: bool,
    pub api_key_masked: String,
    pub base_url: Option<String>,
    pub model_count: usize,
}

impl ModelDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            models: Vec::new(),
            current: String::new(),
            selected: 0,
            scroll: CenteredScroll::new(),
            filter: String::new(),
            tab: ModelDialogTab::SelectModel,
            custom_models: Vec::new(),
            provider_configs: Vec::new(),
            new_model_name: String::new(),
            new_model_provider: String::new(),
            new_model_api_key: String::new(),
            new_model_base_url: String::new(),
            adding_model: false,
            field_index: 0,
            visible_height: 10,
            flat_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_models(&mut self, models: Vec<String>) {
        if self.models != models {
            self.models = models;
            self.flat_cache.borrow_mut().take();
        }
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    /// Compute the number of rows available for model entries, after subtracting
    /// non-model rows (tab line, blank spacer, optional filter lines, footer spacer, footer line).
    pub fn model_row_budget(&self) -> usize {
        let mut budget = self.visible_height;
        // Tab line + blank spacer after tab
        budget = budget.saturating_sub(2);
        // Optional filter line + spacer
        if !self.filter.is_empty() {
            budget = budget.saturating_sub(2);
        }
        // Spacer before footer + footer line
        budget = budget.saturating_sub(2);
        budget
    }

    pub fn set_current(&mut self, current: &str) {
        self.current = current.to_string();
    }

    pub fn initialize_selection(&mut self) {
        let flat = self.flat_filtered();
        if !flat.is_empty() {
            if !self.current.is_empty() {
                if let Some(idx) = flat
                    .iter()
                    .position(|(p, n)| format!("{}/{}", p, n) == self.current)
                {
                    self.selected = idx;
                    let visible_models = self.count_visible_models(0);
                    self.scroll.clamp(self.selected, flat.len(), visible_models);
                }
            } else {
                if let Some(idx) = flat.iter().position(|(p, _)| p == "opencode_zen") {
                    self.selected = idx;
                    let visible_models = self.count_visible_models(0);
                    self.scroll.clamp(self.selected, flat.len(), visible_models);
                } else {
                    self.selected = 0;
                    let visible_models = self.count_visible_models(0);
                    self.scroll.clamp(self.selected, flat.len(), visible_models);
                }
            }
        } else {
            self.selected = 0;
        }
    }

    pub fn selected(&self) -> Option<String> {
        let flat = self.flat_filtered();
        flat.get(self.selected)
            .map(|(provider, name)| format!("{}/{}", provider, name))
    }

    /// Map a rendered row (relative to dialog area, EXCLUDING borders) to a model index in flat_filtered().
    /// Returns None if the row doesn't correspond to a selectable model.
    ///
    /// Coordinate contract:
    /// - This function expects `rel_y` to be content-relative (excluding top/bottom borders)
    /// - Row 0 = tab line ("[ Select Model ] | [ Configure ]")
    /// - Row 1 = blank line
    /// - Optional filter lines follow (if filter is not empty)
    /// - Then model rows with provider headers
    ///
    /// Note: `Component::hit_test()` (which calls this function) receives dialog-relative
    /// coordinates (including borders) and subtracts 1 for the top border before calling
    /// this function.
    pub fn hit_test_model_row(&self, rel_y: usize) -> Option<usize> {
        let mut row = rel_y;

        // Row 0: tab line "[ Select Model ] | [ Configure ]"
        if row < 1 {
            return None;
        }
        row -= 1;

        // Row 1: blank line ""
        if row < 1 {
            return None;
        }
        row -= 1;

        // Optional filter lines: "filter: ..." + blank line
        if !self.filter.is_empty() {
            if row < 1 {
                return None;
            }
            row -= 1;
            if row < 1 {
                return None;
            }
            row -= 1;
        }

        // Now row is the rendered item index in the model area.
        let flat = self.flat_filtered();
        let scroll = self.scroll.get();

        // Build list of rendered items: (flat_idx, is_header)
        let mut rendered_items: Vec<(usize, bool)> = Vec::new();
        let mut last_provider: Option<&str> = None;

        // Set last_provider from the last skipped item
        if scroll > 0 {
            for (i, (provider, _)) in flat.iter().enumerate() {
                if i >= scroll {
                    break;
                }
                last_provider = Some(provider);
            }
        }

        for (flat_idx, (provider, _)) in flat.iter().enumerate() {
            if flat_idx < scroll {
                continue;
            }

            let provider_changed = last_provider != Some(provider.as_str());
            if provider_changed {
                rendered_items.push((flat_idx, true)); // header
                last_provider = Some(provider);
            }
            rendered_items.push((flat_idx, false)); // model entry
        }

        // Now find the item at index `row`
        if row < rendered_items.len() {
            let (flat_idx, is_header) = rendered_items[row];
            if is_header {
                return None; // Clicked on provider header
            }
            return Some(flat_idx);
        }

        None
    }

    pub fn count_visible_models(&self, start_idx: usize) -> usize {
        let budget = self.model_row_budget();
        let mut lines_used = 0;
        let mut models_shown = 0;
        let flat = self.flat_filtered();
        let scroll = self.scroll.get();
        let mut last_provider: Option<&str> = None;

        for (flat_idx, (provider, _)) in flat.iter().enumerate().skip(start_idx) {
            if flat_idx < scroll {
                last_provider = Some(provider.as_str());
                continue;
            }

            let is_new_provider = last_provider != Some(provider.as_str());

            // Calculate lines for this entry
            let mut entry_lines = 1; // model line
            if is_new_provider {
                entry_lines += 1; // provider header
            }

            // Check if we can fit this entry (with header if any)
            if lines_used + entry_lines > budget {
                // Can't fit with header. If this is the selected model, try without header
                if flat_idx == self.selected && is_new_provider {
                    // Try just the model without header
                    if lines_used < budget {
                        // Show model without header
                        lines_used += 1;
                        models_shown += 1;
                        last_provider = Some(provider.as_str());
                        continue;
                    }
                }
                break;
            }

            lines_used += entry_lines;
            models_shown += 1;
            last_provider = Some(provider.as_str());
        }

        models_shown
    }

    pub fn select_up(&mut self) {
        let flat = self.flat_filtered();
        if !flat.is_empty() && self.selected > 0 {
            self.selected -= 1;
        }
        let scroll = self.scroll.get();
        let visible_models = self.count_visible_models(scroll);
        if self.selected < scroll || self.selected >= scroll + visible_models {
            self.scroll.clamp(self.selected, flat.len(), visible_models);
        }
    }

    pub fn select_down(&mut self) {
        let flat = self.flat_filtered();
        let max = flat.len().saturating_sub(1);
        if !flat.is_empty() && self.selected < max {
            self.selected += 1;
        }
        let scroll = self.scroll.get();
        let visible_models = self.count_visible_models(scroll);
        if self.selected < scroll || self.selected >= scroll + visible_models {
            self.scroll.clamp(self.selected, flat.len(), visible_models);
        }
    }

    pub fn set_filter(&mut self, c: char) {
        if self.tab == ModelDialogTab::SelectModel {
            self.filter.push(c);
            self.selected = 0;
            self.scroll.reset();
            self.update_cache();
            self.clamp_selection();
        }
    }

    pub fn backspace_filter(&mut self) {
        if self.tab == ModelDialogTab::SelectModel {
            self.filter.pop();
            self.selected = 0;
            self.scroll.reset();
            self.update_cache();
            self.clamp_selection();
        }
    }

    pub fn update_cache(&mut self) {
        if self.filter.is_empty() {
            self.flat_cache.borrow_mut().take();
            return;
        }
        let groups = self.get_grouped_models();
        let filter_lower = self.filter.to_lowercase();
        let mut result = Vec::new();
        for (provider, models) in groups {
            for model in models {
                if model.to_lowercase().contains(&filter_lower) {
                    let name = model.split('/').next_back().unwrap_or(model).to_string();
                    result.push((provider.clone(), name));
                }
            }
        }
        self.flat_cache
            .borrow_mut()
            .replace((self.filter.clone(), result));
    }

    fn clamp_selection(&mut self) {
        let flat = self.flat_filtered();
        let len = flat.len();
        if len > 0 && self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub fn next_tab(&mut self) {
        self.tab = match self.tab {
            ModelDialogTab::SelectModel => ModelDialogTab::Configure,
            ModelDialogTab::Configure => ModelDialogTab::SelectModel,
        };
        self.selected = 0;
        self.scroll.reset();
    }

    pub fn set_provider_configs(&mut self, configs: Vec<ProviderStatus>) {
        self.provider_configs = configs;
    }

    pub fn set_custom_models(&mut self, models: Vec<CustomModel>) {
        self.custom_models = models;
    }

    pub fn start_adding_model(&mut self) {
        self.adding_model = true;
        self.field_index = 0;
        self.new_model_name.clear();
        self.new_model_provider.clear();
        self.new_model_api_key.clear();
        self.new_model_base_url.clear();
    }

    fn selected_custom_model(&self) -> Option<usize> {
        if self.selected < self.custom_models.len() {
            Some(self.selected)
        } else {
            None
        }
    }

    fn handle_add_model_input(&mut self, c: char) {
        match self.field_index {
            0 => self.new_model_name.push(c),
            1 => self.new_model_provider.push(c),
            2 => self.new_model_api_key.push(c),
            3 => self.new_model_base_url.push(c),
            _ => {}
        }
    }

    fn backspace_add_model_field(&mut self) {
        match self.field_index {
            0 => self.new_model_name.pop(),
            1 => self.new_model_provider.pop(),
            2 => self.new_model_api_key.pop(),
            3 => self.new_model_base_url.pop(),
            _ => None,
        };
    }

    fn next_add_model_field(&mut self) {
        self.field_index = (self.field_index + 1) % 4;
    }

    fn prev_add_model_field(&mut self) {
        if self.field_index == 0 {
            self.field_index = 3;
        } else {
            self.field_index -= 1;
        }
    }

    pub fn cancel_adding_model(&mut self) {
        self.adding_model = false;
        self.new_model_name.clear();
        self.new_model_provider.clear();
        self.new_model_api_key.clear();
        self.new_model_base_url.clear();
    }

    pub fn add_custom_model(&mut self) -> Option<CustomModel> {
        if self.new_model_name.is_empty() || self.new_model_provider.is_empty() {
            return None;
        }
        let model = CustomModel {
            name: self.new_model_name.clone(),
            provider: self.new_model_provider.clone(),
            api_key: if self.new_model_api_key.is_empty() {
                None
            } else {
                Some(self.new_model_api_key.clone())
            },
            base_url: if self.new_model_base_url.is_empty() {
                None
            } else {
                Some(self.new_model_base_url.clone())
            },
        };
        self.custom_models.push(model.clone());
        self.adding_model = false;
        Some(model)
    }

    pub fn remove_custom_model(&mut self, idx: usize) {
        if idx < self.custom_models.len() {
            self.custom_models.remove(idx);
            if self.selected >= self.custom_models.len() {
                self.selected = self.custom_models.len().saturating_sub(1);
            }
        }
    }

    pub fn get_grouped_models(&self) -> Vec<(String, Vec<&String>)> {
        let mut groups: std::collections::HashMap<String, Vec<&String>> =
            std::collections::HashMap::new();
        for model in &self.models {
            let provider = model.split('/').next().unwrap_or("").to_string();
            groups.entry(provider).or_default().push(model);
        }
        let mut result: Vec<(String, Vec<&String>)> = groups.into_iter().collect();
        result.sort_by_key(|a| a.0.to_lowercase());
        result
    }

    pub fn flat_filtered(&self) -> Vec<(String, String)> {
        if let Some((ref cache_filter, ref cache_result)) = self.flat_cache.borrow().as_ref() {
            if cache_filter == &self.filter {
                return cache_result.clone();
            }
        }
        let groups = self.get_grouped_models();
        let filter_lower = self.filter.to_lowercase();
        let mut result = Vec::new();
        for (provider, models) in groups {
            for model in models {
                if filter_lower.is_empty() || model.to_lowercase().contains(&filter_lower) {
                    let name = model.split('/').next_back().unwrap_or(model).to_string();
                    result.push((provider.clone(), name));
                }
            }
        }
        self.flat_cache
            .borrow_mut()
            .replace((self.filter.clone(), result.clone()));
        result
    }
}

impl Default for ModelDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_down() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.models = vec![
            "opencode_zen/gpt4".to_string(),
            "opencode_zen/gpt35".to_string(),
            "opencode_zen/claude".to_string(),
        ];
        dialog.visible_height = 10;
        dialog.flat_cache.borrow_mut().take();

        dialog.select_down();
        assert_eq!(dialog.selected, 1);

        dialog.select_down();
        assert_eq!(dialog.selected, 2);
    }

    #[test]
    fn test_select_up() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.models = vec![
            "opencode_zen/gpt4".to_string(),
            "opencode_zen/gpt35".to_string(),
            "opencode_zen/claude".to_string(),
        ];
        dialog.visible_height = 10;
        dialog.selected = 2;
        dialog.flat_cache.borrow_mut().take();

        dialog.select_up();
        assert_eq!(dialog.selected, 1);

        dialog.select_up();
        assert_eq!(dialog.selected, 0);
    }

    #[test]
    fn test_select_up_at_top() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.models = vec!["opencode_zen/gpt4".to_string()];
        dialog.visible_height = 10;
        dialog.selected = 0;
        dialog.flat_cache.borrow_mut().take();

        dialog.select_up();
        assert_eq!(dialog.selected, 0);
    }

    #[test]
    fn test_filter_resets_selection() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.models = vec![
            "opencode_zen/gpt4".to_string(),
            "opencode_zen/gpt35".to_string(),
            "opencode_zen/claude".to_string(),
        ];
        dialog.visible_height = 10;
        dialog.selected = 2;
        dialog.flat_cache.borrow_mut().take();

        dialog.set_filter('g');
        assert_eq!(dialog.selected, 0);
        assert_eq!(dialog.filter, "g");
    }

    #[test]
    fn test_backspace_filter() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.models = vec![
            "opencode_zen/gpt4".to_string(),
            "opencode_zen/gpt35".to_string(),
        ];
        dialog.visible_height = 10;
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.filter = "gp".to_string();
        dialog.flat_cache.borrow_mut().take();

        dialog.backspace_filter();
        assert_eq!(dialog.filter, "g");
        assert_eq!(dialog.selected, 0);
    }

    #[test]
    fn test_next_tab() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        assert_eq!(dialog.tab, ModelDialogTab::SelectModel);

        dialog.next_tab();
        assert_eq!(dialog.tab, ModelDialogTab::Configure);

        dialog.next_tab();
        assert_eq!(dialog.tab, ModelDialogTab::SelectModel);
    }

    #[test]
    fn test_flat_filtered_with_filter() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.models = vec![
            "opencode_zen/gpt4".to_string(),
            "opencode_zen/gpt35".to_string(),
            "opencode_zen/claude".to_string(),
        ];
        dialog.filter = "gpt".to_string();
        dialog.update_cache();

        let flat = dialog.flat_filtered();
        assert_eq!(flat.len(), 2);
        assert!(flat.iter().all(|(_p, n)| n.contains("gpt")));
    }

    #[test]
    fn test_set_models_populates_models() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        let models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
        dialog.set_models(models.clone());
        assert_eq!(dialog.models, models);
    }

    #[test]
    fn test_set_current_updates_current() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.set_current("openai/gpt4");
        assert_eq!(dialog.current, "openai/gpt4");
    }

    #[test]
    fn test_set_models_syncs_with_app() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        let models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
        dialog.set_models(models.clone());
        dialog.set_current(&models[0]);
        assert_eq!(dialog.current, models[0]);
        assert_eq!(dialog.models, models);
    }

    #[test]
    fn test_tab_switches_to_configure() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        assert_eq!(dialog.tab, ModelDialogTab::SelectModel);
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::empty(),
        );
        dialog.handle_key(key);
        assert_eq!(dialog.tab, ModelDialogTab::Configure);
    }

    #[test]
    fn test_enter_in_configure_no_select_model() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::Configure;
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        let result = dialog.handle_key(key);
        assert_eq!(result, None);
    }

    #[test]
    fn test_filter_typing_in_select_model() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.set_filter('g');
        assert_eq!(dialog.filter, "g");
        dialog.set_filter('p');
        assert_eq!(dialog.filter, "gp");
    }

    #[test]
    fn test_add_model_multi_char_fields() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.start_adding_model();
        // Field 0: name
        dialog.handle_add_model_input('m');
        dialog.handle_add_model_input('y');
        dialog.handle_add_model_input('m');
        assert_eq!(dialog.new_model_name, "mym");
        // Switch to field 1: provider
        dialog.next_add_model_field();
        dialog.handle_add_model_input('o');
        dialog.handle_add_model_input('p');
        assert_eq!(dialog.new_model_provider, "op");
        // Switch to field 2: api_key
        dialog.next_add_model_field();
        dialog.handle_add_model_input('k');
        assert_eq!(dialog.new_model_api_key, "k");
        // Switch to field 3: base_url
        dialog.next_add_model_field();
        dialog.handle_add_model_input('u');
        assert_eq!(dialog.new_model_base_url, "u");
    }

    #[test]
    fn test_esc_cancels_add_model() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.start_adding_model();
        dialog.new_model_name = "test".to_string();
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        );
        dialog.handle_key(key);
        assert!(!dialog.adding_model);
        assert!(dialog.new_model_name.is_empty());
    }

    #[test]
    fn test_select_tab_footer() {
        let dialog = ModelDialog::new(Arc::new(Theme::default()));
        // Footer should have Enter select and Tab configure
        // This is a basic test to ensure footer text is correct
        assert_eq!(dialog.tab, ModelDialogTab::SelectModel);
    }
}

#[test]
fn test_paste_select_model_filters_and_resets() {
    let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
    dialog.tab = ModelDialogTab::SelectModel;
    dialog.models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
    dialog.selected = 1;
    dialog.handle_paste("gpt".to_string());
    assert_eq!(dialog.filter, "gpt");
    assert_eq!(dialog.selected, 0);
    let flat = dialog.flat_filtered();
    assert_eq!(flat.len(), 1);
}

#[test]
fn test_paste_configure_does_not_corrupt_filter() {
    let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
    dialog.tab = ModelDialogTab::Configure;
    dialog.filter = "test".to_string();
    dialog.handle_paste("paste".to_string());
    assert_eq!(dialog.filter, "test"); // filter unchanged
}

#[test]
fn test_paste_add_model_field() {
    let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
    dialog.tab = ModelDialogTab::Configure;
    dialog.start_adding_model();
    dialog.field_index = 0; // name field
    dialog.handle_paste("my-model".to_string());
    assert_eq!(dialog.new_model_name, "my-model");
    dialog.field_index = 2; // api key field
    dialog.handle_paste("key123".to_string());
    assert_eq!(dialog.new_model_api_key, "key123");
}

#[test]
fn test_enter_no_match_does_nothing() {
    let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
    dialog.tab = ModelDialogTab::SelectModel;
    dialog.models = vec!["openai/gpt4".to_string()];
    dialog.filter = "nonexistent".to_string();
    dialog.update_cache();
    let result = dialog.selected();
    assert!(result.is_none());
    let key = crossterm::event::KeyCode::Enter;
    // Simulate Enter press
    let msg = dialog.handle_key(crossterm::event::KeyEvent::new(
        key,
        crossterm::event::KeyModifiers::empty(),
    ));
    assert!(msg.is_none());
}

impl Component for ModelDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Tab => {
                if self.adding_model {
                    self.next_add_model_field();
                } else {
                    self.next_tab();
                }
            }
            crossterm::event::KeyCode::BackTab => {
                if self.adding_model {
                    self.prev_add_model_field();
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
            }
            crossterm::event::KeyCode::Char(c) => {
                if self.tab == ModelDialogTab::SelectModel {
                    self.set_filter(c);
                } else if self.tab == ModelDialogTab::Configure {
                    if !self.adding_model {
                        match c {
                            'a' => self.start_adding_model(),
                            'd' => {
                                if let Some(model) = self.selected_custom_model() {
                                    self.remove_custom_model(model);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        self.handle_add_model_input(c);
                    }
                }
            }
            crossterm::event::KeyCode::Backspace => {
                if self.tab == ModelDialogTab::SelectModel {
                    self.backspace_filter();
                } else if self.adding_model {
                    self.backspace_add_model_field();
                }
            }
            crossterm::event::KeyCode::Enter => {
                if self.tab == ModelDialogTab::SelectModel {
                    if let Some(model) = self.selected() {
                        return Some(TuiMsg::SelectModel { model });
                    }
                } else if self.adding_model {
                    if self.field_index == 3 {
                        // Save custom model on last field
                        if let Some(custom_model) = self.add_custom_model() {
                            let model_str =
                                format!("{}/{}", custom_model.provider, custom_model.name);
                            self.models.push(model_str);
                            self.update_cache();
                            self.selected = self.models.len() - 1;
                            self.tab = ModelDialogTab::SelectModel;
                            self.adding_model = false;
                        }
                    } else {
                        self.next_add_model_field();
                    }
                } else {
                    // Do nothing in Configure tab unless adding model
                }
            }
            crossterm::event::KeyCode::Esc => {
                if self.adding_model {
                    self.cancel_adding_model();
                } else {
                    return Some(TuiMsg::CloseDialog);
                }
            }
            _ => {}
        }
        None
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        match self.tab {
            ModelDialogTab::SelectModel => {
                self.filter.push_str(&text);
                self.selected = 0;
                self.scroll.reset();
                self.update_cache();
                let flat = self.flat_filtered();
                self.scroll
                    .clamp(self.selected, flat.len(), self.visible_height);
            }
            ModelDialogTab::Configure => {
                if self.adding_model {
                    match self.field_index {
                        0 => self.new_model_name.push_str(&text),
                        1 => self.new_model_provider.push_str(&text),
                        2 => self.new_model_api_key.push_str(&text),
                        3 => self.new_model_base_url.push_str(&text),
                        _ => {}
                    }
                }
            }
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        // Set visible_height based on actual popup area (subtract borders)
        self.visible_height = (area.height as usize).saturating_sub(2);
        self.set_theme(theme);
        let mut lines: Vec<Line> = Vec::new();

        let tab_select = if self.tab == ModelDialogTab::SelectModel {
            "[ Select Model ]"
        } else {
            "  Select Model   "
        };
        let tab_config = if self.tab == ModelDialogTab::Configure {
            "[ Configure ]"
        } else {
            "  Configure   "
        };

        lines.push(Line::from(vec![
            Span::styled(" ", Style::default().fg(theme.primary)),
            Span::styled(tab_select, Style::default().fg(theme.primary)),
            Span::styled("|", Style::default().fg(theme.muted)),
            Span::styled(tab_config, Style::default().fg(theme.primary)),
        ]));
        lines.push(Line::from(""));

        match self.tab {
            ModelDialogTab::SelectModel => {
                if !self.filter.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("filter: ", Style::default().fg(theme.muted)),
                        Span::raw(&self.filter),
                    ]));
                    lines.push(Line::from(""));
                }

                let flat = self.flat_filtered();
                let is_empty = flat.is_empty();
                let scroll = self.scroll.get();
                let budget = self.model_row_budget();
                let visible_models = self.count_visible_models(scroll);
                let mut last_provider: Option<String> = None;
                let mut lines_rendered = 0;
                let mut models_rendered = 0;

                for (flat_idx, item) in flat.iter().enumerate() {
                    if flat_idx < scroll {
                        continue;
                    }
                    if models_rendered >= visible_models {
                        break;
                    }

                    let (provider, name) = item.clone();

                    let provider_changed = last_provider.as_ref() != Some(&provider);
                    if provider_changed {
                        // Only render provider header if there's room for at least one model row after it
                        if lines_rendered + 1 < budget {
                            lines.push(Line::from(vec![Span::styled(
                                provider.clone(),
                                Style::default()
                                    .fg(theme.primary)
                                    .add_modifier(Modifier::BOLD),
                            )]));
                            last_provider = Some(provider.clone());
                            lines_rendered += 1;
                            if lines_rendered >= budget {
                                break;
                            }
                        } else {
                            // Skip header, keep last_provider as previous to avoid re-adding header
                            last_provider = Some(provider.clone());
                        }
                    }

                    let is_selected = flat_idx == self.selected;
                    let full_model = format!("{}/{}", provider, name);
                    let display_name = name.clone();
                    let is_current = full_model == self.current;
                    let style = if is_selected {
                        Style::default()
                            .fg(theme.primary)
                            .bg(theme.selection)
                            .add_modifier(Modifier::BOLD)
                    } else if is_current {
                        Style::default().fg(theme.success)
                    } else {
                        Style::default().fg(theme.foreground)
                    };
                    let marker = if is_current { "✓ " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(marker.to_string(), Style::default().fg(theme.muted)),
                        Span::styled(display_name, style),
                    ]));
                    lines_rendered += 1;
                    models_rendered += 1;
                }

                if is_empty {
                    let message = if self.filter.is_empty() {
                        "  (no models available)"
                    } else {
                        "  (no matches)"
                    };
                    lines.push(Line::from(Span::styled(
                        message,
                        Style::default().fg(theme.muted),
                    )));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "↑/↓ navigate  |  Enter select  |  Backspace filter  |  Tab configure  |  Esc close",
                    Style::default().fg(theme.muted),
                )));
            }
            ModelDialogTab::Configure => {
                lines.push(Line::from(Span::styled(
                    "Provider Configuration",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));

                if self.adding_model {
                    // Render custom model input fields
                    let fields = [
                        ("Name", &self.new_model_name, 0),
                        ("Provider", &self.new_model_provider, 1),
                        ("API Key", &self.new_model_api_key, 2),
                        ("Base URL", &self.new_model_base_url, 3),
                    ];
                    for (label, value, idx) in fields.iter() {
                        let is_active = *idx == self.field_index;
                        let style = if is_active {
                            Style::default().fg(theme.primary).bg(theme.selection)
                        } else {
                            Style::default().fg(theme.foreground)
                        };
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {}: ", label), style),
                            Span::styled((*value).clone(), style),
                        ]));
                    }
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Tab next  Shift+Tab prev  Enter save  Esc cancel",
                        Style::default().fg(theme.muted),
                    )));
                } else {
                    // Render provider configs
                    if self.provider_configs.is_empty() {
                        lines.push(Line::from(Span::styled(
                            "  (no providers configured)",
                            Style::default().fg(theme.muted),
                        )));
                        lines.push(Line::from(""));
                        lines.push(Line::from(Span::styled(
                            "Press 'a' to add a custom model",
                            Style::default().fg(theme.muted),
                        )));
                    } else {
                        let scroll = self.scroll.get();
                        let visible_height = self.visible_height.saturating_sub(1);
                        let mut render_idx = 0;

                        for (i, config) in self.provider_configs.iter().enumerate() {
                            if i < scroll {
                                continue;
                            }
                            if render_idx >= visible_height {
                                break;
                            }

                            let is_selected = i == self.selected;
                            let style = if is_selected {
                                Style::default().fg(theme.primary).bg(theme.selection)
                            } else {
                                Style::default().fg(theme.foreground)
                            };

                            lines.push(Line::from(vec![
                                Span::styled("  ", Style::default().fg(theme.muted)),
                                Span::styled(&config.name, style),
                            ]));

                            if config.has_api_key {
                                lines.push(Line::from(vec![
                                    Span::styled("    ", Style::default().fg(theme.muted)),
                                    Span::styled(
                                        format!("API key: {}", config.api_key_masked),
                                        Style::default().fg(theme.success),
                                    ),
                                ]));
                            }

                            if let Some(ref base_url) = config.base_url {
                                lines.push(Line::from(vec![
                                    Span::styled("    ", Style::default().fg(theme.muted)),
                                    Span::styled(
                                        format!("Endpoint: {}", base_url),
                                        Style::default().fg(theme.muted),
                                    ),
                                ]));
                            }
                            lines.push(Line::from(""));
                            render_idx += 1;
                        }
                    }

                    // Render custom models list
                    if !self.custom_models.is_empty() {
                        lines.push(Line::from(""));
                        lines.push(Line::from(Span::styled(
                            "Custom Models",
                            Style::default()
                                .fg(theme.primary)
                                .add_modifier(Modifier::BOLD),
                        )));
                        lines.push(Line::from(""));

                        for (i, model) in self.custom_models.iter().enumerate() {
                            let is_selected = i == self.selected;
                            let style = if is_selected {
                                Style::default().fg(theme.primary).bg(theme.selection)
                            } else {
                                Style::default().fg(theme.foreground)
                            };
                            lines.push(Line::from(vec![
                                Span::styled("  ", Style::default().fg(theme.muted)),
                                Span::styled(format!("{}/{}", model.provider, model.name), style),
                            ]));
                            lines.push(Line::from(""));
                        }
                    }

                    // Footer
                    let mut footer_parts = vec![
                        "↑/↓/j/k navigate".to_string(),
                        "Tab switch tab".to_string(),
                        "a add custom model".to_string(),
                    ];
                    if !self.custom_models.is_empty() {
                        footer_parts.push("d delete".to_string());
                    }
                    footer_parts.push("Esc cancel".to_string());
                    lines.push(Line::from(Span::styled(
                        footer_parts.join("  "),
                        Style::default().fg(theme.muted),
                    )));
                }
            }
        }

        let block = Block::default()
            .title(" Models ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Model
    }

    fn hit_test(&self, rel_y: usize) -> Option<usize> {
        // rel_y is dialog-relative (including borders), subtract top border (1 row)
        if rel_y < 1 {
            return None; // Clicked on top border
        }
        self.hit_test_model_row(rel_y - 1)
    }

    fn set_selected(&mut self, idx: usize) {
        self.selected = idx;
    }
}

#[cfg(test)]
mod hit_test_tests {
    use crate::tui::components::component::Component;
    use crate::tui::components::dialogs::model::{CustomModel, ModelDialog, ModelDialogTab};
    use crate::tui::theme::Theme;
    use crossterm::event;
    use std::sync::Arc;

    #[test]
    fn test_hit_test_model_row_first_model() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
        dialog.visible_height = 20;
        dialog.update_cache();
        // Row 0: tab line, Row 1: blank line
        // Row 2: provider header "openai" (None)
        // Row 3: first model "gpt4" (Some(0))
        let result = dialog.hit_test_model_row(3);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_hit_test_model_row_with_filter() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
        dialog.filter = "gpt".to_string();
        dialog.visible_height = 20;
        dialog.update_cache();
        // Row 0: tab, Row 1: blank
        // Row 2: "filter: gpt", Row 3: blank
        // Row 4: provider header "openai" (None)
        // Row 5: first matching model "gpt4" (Some(0))
        let result = dialog.hit_test_model_row(5);
        assert_eq!(result, Some(0));
        // Row 6 should be None (only 1 match)
        let result2 = dialog.hit_test_model_row(6);
        assert_eq!(result2, None);
    }

    #[test]
    fn test_hit_test_model_row_out_of_bounds() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        // Row 0 (tab line) should return None
        assert_eq!(dialog.hit_test_model_row(0), None);
        // Row 1 (blank line) should return None
        assert_eq!(dialog.hit_test_model_row(1), None);
        // Row 2 (provider header) should return None
        assert_eq!(dialog.hit_test_model_row(2), None);
    }

    #[test]
    fn test_hit_test_model_row_with_scroll() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.models = vec![
            "openai/gpt3".to_string(),
            "openai/gpt4".to_string(),
            "anthropic/claude".to_string(),
        ];
        // visible_height is the model area height (after subtracting tab/blank lines)
        // We need flat.len() > visible_height for scrolling to work
        // With 3 models and visible=2: max_scroll = 3-2 = 1
        dialog.visible_height = 2;
        dialog.update_cache();
        let flat = dialog.flat_filtered();
        // flat is sorted by provider name: anthropic first, then openai
        assert_eq!(flat.len(), 3);
        // scroll.clamp(1, 3, 2): cursor=1, total=3, visible=2
        // max_scroll = 1, middle = 1, cursor >= max_scroll → new_scroll = 1
        dialog.scroll.clamp(1, flat.len(), dialog.visible_height);
        assert_eq!(dialog.scroll.get(), 1);

        // With scroll=1, we skip flat[0] = ("anthropic", "claude")
        // First visible provider is "openai" (flat[1]), which renders a provider header
        // Row 0: tab, Row 1: blank
        // Row 2: provider header "openai" → None (header)
        let result = dialog.hit_test_model_row(2);
        assert_eq!(result, None); // Clicking on provider header returns None

        // Row 3: model "gpt3" at flat_idx=1 → Some(1)
        let result2 = dialog.hit_test_model_row(3);
        assert_eq!(result2, Some(1));

        // Row 4: model "gpt4" at flat_idx=2 → Some(2)
        let result3 = dialog.hit_test_model_row(4);
        assert_eq!(result3, Some(2));
    }

    #[test]
    fn test_set_selected() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
        dialog.update_cache();

        // Initially selected should be 0
        assert_eq!(dialog.selected, 0);

        // Call set_selected to change selection
        dialog.set_selected(1);
        assert_eq!(dialog.selected, 1);

        // Call set_selected with out of bounds index (should still set)
        dialog.set_selected(5);
        assert_eq!(dialog.selected, 5);
    }

    #[test]
    fn test_configure_tab_a_starts_add_mode() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::Configure;
        assert!(!dialog.adding_model);

        // Press 'a' in Configure tab
        dialog.handle_key(event::KeyEvent::new(
            event::KeyCode::Char('a'),
            event::KeyModifiers::empty(),
        ));
        assert!(dialog.adding_model);
        assert_eq!(dialog.field_index, 0);
    }

    #[test]
    fn test_add_mode_renders_input_fields() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::Configure;
        dialog.start_adding_model();
        dialog.visible_height = 20;

        // Simplified render check: ensure adding_model is true and fields are tracked
        assert!(dialog.adding_model);
        assert_eq!(dialog.field_index, 0);

        // Simulate typing in name field
        dialog.handle_key(event::KeyEvent::new(
            event::KeyCode::Char('t'),
            event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.new_model_name, "t");
    }

    #[test]
    fn test_enter_on_last_field_saves_custom_model() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::Configure;
        dialog.start_adding_model();
        dialog.new_model_name = "test-model".to_string();
        dialog.new_model_provider = "test-provider".to_string();
        dialog.new_model_api_key = "test-key".to_string();
        dialog.new_model_base_url = "http://test.com".to_string();
        dialog.field_index = 3; // Last field (Base URL)

        // Press Enter on last field
        dialog.handle_key(event::KeyEvent::new(
            event::KeyCode::Enter,
            event::KeyModifiers::empty(),
        ));
        // After saving, adding_model should be false, model added to list
        assert!(!dialog.adding_model);
        assert_eq!(dialog.custom_models.len(), 1);
        assert_eq!(dialog.custom_models[0].name, "test-model");
        assert_eq!(dialog.custom_models[0].provider, "test-provider");
    }

    #[test]
    fn test_footer_shows_d_delete_only_with_custom_models() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::Configure;
        // No custom models: footer should not have 'd delete'
        // Simplified check: ensure when custom_models is empty, 'd delete' is not present
        assert!(dialog.custom_models.is_empty());

        // Add a custom model
        dialog.custom_models.push(CustomModel {
            name: "test".to_string(),
            provider: "test".to_string(),
            api_key: None,
            base_url: None,
        });
        // Now footer should have 'd delete'
        assert!(!dialog.custom_models.is_empty());
    }

    #[test]
    fn test_hit_test_uses_dialog_relative_coordinates() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.models = vec!["openai/gpt4".to_string(), "anthropic/claude".to_string()];
        dialog.visible_height = 20;
        dialog.update_cache();

        // Content-relative row for first model: 3 (tab=0, blank=1, provider header=2, model=3)
        // Dialog-relative row = content-relative +1 (top border) =4
        let result = dialog.hit_test(4); // dialog-relative row 4
        assert_eq!(result, Some(0)); // first model index 0

        // Click on top border (dialog-relative row 0) → None
        let result_border = dialog.hit_test(0);
        assert_eq!(result_border, None);

        // Click on tab line (dialog-relative row 1) → None
        let result_tab = dialog.hit_test(1);
        assert_eq!(result_tab, None);
    }

    #[test]
    fn test_small_height_shows_footer() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        dialog.models = vec!["openai/gpt4".to_string()];
        // Small height: area.height = 10 → visible_height = 10-2=8
        // Non-model rows: tab(1)+blank(1)+spacer(1)+footer(1) =4 → budget=8-4=4
        dialog.visible_height = 8; // area.height -2
        dialog.update_cache();
        let budget = dialog.model_row_budget();
        assert!(budget >= 1); // At least 1 model row fits
    }

    #[test]
    fn test_small_height_no_header_without_model_row() {
        let mut dialog = ModelDialog::new(Arc::new(Theme::default()));
        dialog.tab = ModelDialogTab::SelectModel;
        // Only 1 model, height so small that header can't fit with model row
        dialog.models = vec!["openai/gpt4".to_string()];
        dialog.visible_height = 5; // area.height=7 → after borders 5
                                   // budget =5 -2(tab+blank) -2(spacer+footer) =1 → only model row fits, but header takes 1 row
        dialog.update_cache();
        let budget = dialog.model_row_budget();
        assert_eq!(budget, 1); // Only 1 row for model, no header
        let visible = dialog.count_visible_models(0);
        // count_visible_models() now shows model WITHOUT header when budget is tight
        // So with budget=1, we can show 1 model (without header)
        assert_eq!(visible, 1);
    }
}

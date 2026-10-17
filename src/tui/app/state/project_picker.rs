//! Project picker state for the Multi-Project TUI (milestone 2).
//!
//! The picker is a bounded, modal dialog that lets the user search
//! the daemon-backed project catalog, select a project, optionally
//! choose a workspace, and register new local projects.
//!
//! Invariants:
//! * The picker never stores paths as project identity.
//! * All async completions carry a `request_id` so stale results are
//!   dropped at apply time.
//! * Registration input is gated to local transports only.

use crate::protocol::dto::{ProjectDetailsDto, ProjectSummaryDto};
use crate::tui::app::state::async_request::AsyncUiRequestState;
use crate::tui::input::InputMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Maximum number of open project tabs.
pub const MAX_OPEN_PROJECT_TABS: usize = 16;

/// Maximum items shown in the picker's filtered list.
pub const MAX_PROJECT_LIST_ITEMS: usize = 128;

/// Maximum visible rows rendered in the picker viewport.
pub const MAX_PICKER_VISIBLE_ROWS: usize = 16;

/// Maximum character length for a tab label.
pub const MAX_TAB_LABEL_LEN: usize = 24;

/// Maximum number of tags in a registration draft.
pub const MAX_REGISTRATION_TAGS: usize = 10;

/// Maximum total tag characters (sum of all tags).
pub const MAX_REGISTRATION_TAG_CHARS: usize = 128;

/// Maximum description length for registration.
pub const MAX_REGISTRATION_DESC_LEN: usize = 256;

/// Bounded session summary cache entry per tab.
#[derive(Debug, Clone)]
pub struct SessionSummaryCacheEntry {
    pub session_id: String,
    pub title: String,
    pub time_updated: i64,
    pub archived: bool,
}

/// The current phase of the project picker dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerPhase {
    /// Searching the catalog list.
    Catalog,
    /// Choosing a workspace when the project has >1 workspaces.
    WorkspaceSelection,
    /// Entering registration input (path, local only).
    RegistrationInput,
    /// Confirming registration before submitting.
    RegistrationConfirm,
    /// Loading the current session count before presenting permanent destroy.
    DestroyLoading,
    /// Waiting for explicit confirmation to permanently delete a project.
    DestroyConfirm,
    /// A project lifecycle mutation is in flight.
    LifecycleChanging,
    /// Permanent project deletion is in flight after explicit confirmation.
    Destroying,
    /// Loading the project/workspace values before opening the edit form.
    ProjectEditLoading,
    /// Editing the project's display name and bound directory.
    ProjectEdit,
    /// Project update is being committed.
    ProjectUpdating,
    /// An error occurred; show the error and a retry hint.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerCatalogAction {
    None,
    Close,
    Select,
}

/// Bounded registration draft for creating a new project.
#[derive(Debug, Clone, Default)]
pub struct RegistrationDraft {
    pub display_name: String,
    pub description: String,
    pub tags: Vec<String>,
}

impl RegistrationDraft {
    pub fn push_tag(&mut self, tag: String) {
        if self.tags.len() >= MAX_REGISTRATION_TAGS {
            return;
        }
        let total: usize = self.tags.iter().map(|t| t.len()).sum();
        if total + tag.len() > MAX_REGISTRATION_TAG_CHARS {
            return;
        }
        self.tags.push(tag);
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn set_description(&mut self, desc: String) {
        if desc.len() <= MAX_REGISTRATION_DESC_LEN {
            self.description = desc;
        } else {
            self.description = desc.chars().take(MAX_REGISTRATION_DESC_LEN).collect();
        }
    }
}

/// State for the project picker dialog.
#[derive(Debug)]
pub struct ProjectPickerState {
    /// Current filter query text.
    pub query: String,
    /// Current phase of the picker.
    pub phase: PickerPhase,
    /// Local filter mode shown in the picker badge.
    pub input_mode: InputMode,
    /// Selected row index in the filtered list.
    pub selected_row: usize,
    /// The catalog entry selected for detail lookup.
    pub pinned_project_id: Option<String>,
    /// Resolved ProjectGet result, cached per picker generation.
    pub cached_detail: Option<ProjectDetailsDto>,
    /// Name and exact session count shown by the destroy confirmation.
    pub destroy_confirmation: Option<ProjectDestroyConfirmation>,
    /// Current rename/repath draft while the edit form is mounted.
    pub project_edit: Option<ProjectEditDraft>,
    /// Per-picker request generation for catalog/detail/registration.
    pub request_id: u64,
    /// Snapshot of catalog cache generation used to build this picker.
    pub catalog_generation: u64,
    /// Whether archived projects are shown.
    pub show_archived: bool,
    /// Last error message.
    pub last_error: Option<String>,
    /// Registration draft (bounded).
    pub registration: RegistrationDraft,
    /// Path text input for local registration.
    pub registration_input: String,
    /// Whether the transport supports local path registration.
    pub transport_local: bool,
    /// Async request state for picker operations.
    pub picker_request: AsyncUiRequestState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDestroyConfirmation {
    pub display_name: String,
    pub session_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEditDraft {
    pub display_name: String,
    pub root: String,
    pub expected_updated_at: i64,
    pub editing_root: bool,
}

impl ProjectPickerState {
    /// Create a fresh picker state. `transport_local` should be set
    /// based on `app.ui_state.mode`.
    pub fn new(transport_local: bool, catalog_generation: u64) -> Self {
        Self {
            query: String::new(),
            phase: PickerPhase::Catalog,
            input_mode: InputMode::Normal,
            selected_row: 0,
            pinned_project_id: None,
            cached_detail: None,
            destroy_confirmation: None,
            project_edit: None,
            request_id: 0,
            catalog_generation,
            show_archived: false,
            last_error: None,
            registration: RegistrationDraft::default(),
            registration_input: String::new(),
            transport_local,
            picker_request: AsyncUiRequestState::new(),
        }
    }

    /// Compute filtered indices over the catalog entries. Uses fuzzy
    /// scoring on display_name, tags, and project_id. Returns indices
    /// into `entries`, capped at `MAX_PROJECT_LIST_ITEMS`, sorted by
    /// score descending (lexicographic tiebreak on project_id).
    pub fn filtered_indices(&self, entries: &[ProjectSummaryDto]) -> Vec<usize> {
        let query = self.query.trim();
        let mut scored: Vec<(usize, bool, usize, &str)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.show_archived || e.archived_at.is_none())
            .map(|(i, e)| {
                let name_score = crate::util::fuzzy::fuzzy_score(query, &e.display_name);
                let tag_score: usize = e
                    .tags
                    .iter()
                    .map(|t| crate::util::fuzzy::fuzzy_score(query, t))
                    .max()
                    .unwrap_or(0);
                let id_score = crate::util::fuzzy::fuzzy_score(query, &e.project_id);
                let best = name_score.max(tag_score).max(id_score);
                (i, e.archived_at.is_some(), best, e.project_id.as_str())
            })
            .filter(|(_, _, score, _)| *score > 0 || query.is_empty())
            .collect();

        // Keep inactive projects at the bottom while preserving score and
        // project-id ordering within each lifecycle group.
        scored.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)).then(a.3.cmp(b.3)));

        scored
            .into_iter()
            .take(MAX_PROJECT_LIST_ITEMS)
            .map(|(i, _, _, _)| i)
            .collect()
    }

    /// Apply one catalog-phase key. The App owns the effects that require
    /// daemon requests; this state transition owns local filter/navigation
    /// behavior so the Normal/Insert contract can be tested directly.
    pub fn handle_catalog_key(
        &mut self,
        key: KeyEvent,
        filtered_count: usize,
    ) -> PickerCatalogAction {
        match key.code {
            KeyCode::Esc if self.input_mode == InputMode::Insert => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Esc if self.query.is_empty() => return PickerCatalogAction::Close,
            KeyCode::Esc => {
                self.query.clear();
                self.selected_row = 0;
            }
            KeyCode::Up | KeyCode::Char('k')
                if key.code == KeyCode::Up || self.input_mode == InputMode::Normal =>
            {
                self.select_up(filtered_count);
            }
            KeyCode::Down | KeyCode::Char('j')
                if key.code == KeyCode::Down || self.input_mode == InputMode::Normal =>
            {
                self.select_down(filtered_count);
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected_row = 0;
            }
            KeyCode::Char('i')
                if self.input_mode == InputMode::Normal && key.modifiers == KeyModifiers::NONE =>
            {
                self.input_mode = InputMode::Insert;
            }
            KeyCode::Enter => return PickerCatalogAction::Select,
            KeyCode::Tab => {
                self.show_archived = !self.show_archived;
                self.selected_row = 0;
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                if self.query.chars().count() < 64 {
                    self.query.push(c);
                    self.selected_row = 0;
                }
            }
            _ => {}
        }
        PickerCatalogAction::None
    }

    /// Begin a new picker request generation.
    pub fn begin_request(&mut self) -> u64 {
        self.request_id = self.picker_request.begin();
        self.request_id
    }

    /// Check if the given request id is current.
    pub fn is_request_current(&self, request_id: u64) -> bool {
        self.picker_request.is_current(request_id)
    }

    /// Move selection up, clamping to valid range.
    pub fn select_up(&mut self, visible_count: usize) {
        if visible_count == 0 {
            return;
        }
        if self.selected_row > 0 {
            self.selected_row -= 1;
        }
    }

    /// Move selection down, clamping to valid range.
    pub fn select_down(&mut self, visible_count: usize) {
        if visible_count == 0 {
            return;
        }
        if self.selected_row + 1 < visible_count {
            self.selected_row += 1;
        }
    }

    /// Reset to catalog phase with a fresh query.
    pub fn reset_to_catalog(&mut self) {
        self.phase = PickerPhase::Catalog;
        self.selected_row = 0;
        self.pinned_project_id = None;
        self.cached_detail = None;
        self.destroy_confirmation = None;
        self.project_edit = None;
        self.last_error = None;
        self.registration = RegistrationDraft::default();
        self.registration_input.clear();
    }
}

/// Truncate a tab label to `MAX_TAB_LABEL_LEN` characters.
pub fn truncate_tab_label(label: &str) -> String {
    if label.len() <= MAX_TAB_LABEL_LEN {
        return label.to_string();
    }
    label.chars().take(MAX_TAB_LABEL_LEN).collect()
}

/// Disambiguate duplicate labels by appending a short stable suffix.
/// The suffix is derived from the last 6 chars of the workspace_id or
/// tab_id — never from a path.
pub fn disambiguate_label(label: &str, suffix_source: &str) -> String {
    let suffix = if suffix_source.len() >= 6 {
        suffix_source[suffix_source.len() - 6..].to_string()
    } else {
        suffix_source.to_string()
    };
    let max_base = MAX_TAB_LABEL_LEN.saturating_sub(suffix.len() + 1); // +1 for dash
    let base: String = label.chars().take(max_base).collect();
    format!("{}-{}", base, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_summary(id: &str, name: &str) -> ProjectSummaryDto {
        ProjectSummaryDto {
            project_id: id.to_string(),
            display_name: name.to_string(),
            lifecycle: "active".to_string(),
            description: None,
            tags: Vec::new(),
            time_last_opened_at: None,
            registration_source: "protocol".to_string(),
            archived_at: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn picker_starts_in_catalog_phase() {
        let picker = ProjectPickerState::new(true, 0);
        assert_eq!(picker.phase, PickerPhase::Catalog);
        assert_eq!(picker.selected_row, 0);
        assert!(picker.query.is_empty());
    }

    #[test]
    fn filtered_indices_empty_entries() {
        let picker = ProjectPickerState::new(true, 0);
        let indices = picker.filtered_indices(&[]);
        assert!(indices.is_empty());
    }

    #[test]
    fn filtered_indices_empty_query_returns_all() {
        let picker = ProjectPickerState::new(true, 0);
        let entries = vec![
            make_summary("a", "Alpha"),
            make_summary("b", "Beta"),
            make_summary("c", "Charlie"),
        ];
        let indices = picker.filtered_indices(&entries);
        assert_eq!(indices.len(), 3);
    }

    #[test]
    fn filtered_indices_fuzzy_match() {
        let picker = ProjectPickerState {
            query: "alp".to_string(),
            ..ProjectPickerState::new(true, 0)
        };
        let entries = vec![
            make_summary("a", "Alpha"),
            make_summary("b", "Beta"),
            make_summary("c", "Charlie"),
        ];
        let indices = picker.filtered_indices(&entries);
        assert_eq!(indices.len(), 1);
        assert_eq!(entries[indices[0]].display_name, "Alpha");
    }

    #[test]
    fn filtered_indices_archived_filtered_by_default() {
        let picker = ProjectPickerState::new(true, 0);
        let mut entries = vec![make_summary("a", "Alpha"), make_summary("b", "Beta")];
        entries[1].archived_at = Some(1000);
        let indices = picker.filtered_indices(&entries);
        assert_eq!(indices.len(), 1);
        assert_eq!(entries[indices[0]].project_id, "a");
    }

    #[test]
    fn filtered_indices_show_archived_includes_archived() {
        let mut picker = ProjectPickerState::new(true, 0);
        picker.show_archived = true;
        let mut entries = vec![make_summary("a", "Alpha"), make_summary("b", "Beta")];
        entries[1].archived_at = Some(1000);
        let indices = picker.filtered_indices(&entries);
        assert_eq!(indices.len(), 2);
    }

    #[test]
    fn archived_projects_sink_below_active_projects() {
        let mut picker = ProjectPickerState::new(true, 0);
        picker.show_archived = true;
        let mut entries = vec![
            make_summary("z-active", "Zebra"),
            make_summary("a-inactive", "Alpha"),
            make_summary("a-active", "Alpha"),
        ];
        entries[1].archived_at = Some(1000);
        let indices = picker.filtered_indices(&entries);
        assert_eq!(
            indices
                .iter()
                .map(|index| entries[*index].project_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a-active", "z-active", "a-inactive"]
        );
    }

    #[test]
    fn normal_mode_filters_and_navigates_with_query() {
        let mut picker = ProjectPickerState::new(true, 0);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        picker.handle_catalog_key(key(KeyCode::Char('a')), 4);
        assert_eq!(picker.query, "a");
        picker.handle_catalog_key(key(KeyCode::Char('j')), 4);
        assert_eq!(picker.selected_row, 1);
        assert_eq!(picker.query, "a");
        picker.handle_catalog_key(key(KeyCode::Char('k')), 4);
        assert_eq!(picker.selected_row, 0);
        assert_eq!(
            picker.handle_catalog_key(key(KeyCode::Enter), 4),
            PickerCatalogAction::Select
        );
    }

    #[test]
    fn picker_mode_escape_insert_path_and_selection() {
        let mut picker = ProjectPickerState::new(true, 0);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        picker.handle_catalog_key(key(KeyCode::Char('i')), 4);
        assert_eq!(picker.input_mode, InputMode::Insert);
        picker.handle_catalog_key(key(KeyCode::Char('j')), 4);
        assert_eq!(picker.query, "j");
        assert_eq!(
            picker.handle_catalog_key(key(KeyCode::Esc), 4),
            PickerCatalogAction::None
        );
        assert_eq!(picker.input_mode, InputMode::Normal);
        assert_eq!(picker.query, "j");
        picker.handle_catalog_key(key(KeyCode::Esc), 4);
        assert!(picker.query.is_empty());
        assert_eq!(
            picker.handle_catalog_key(key(KeyCode::Esc), 4),
            PickerCatalogAction::Close
        );
        assert_eq!(
            picker.handle_catalog_key(key(KeyCode::Enter), 4),
            PickerCatalogAction::Select
        );
    }

    #[test]
    fn picker_normal_mode_switch_preserves_insert_char_typing() {
        let mut picker = ProjectPickerState::new(true, 0);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        picker.handle_catalog_key(key(KeyCode::Char('i')), 4);
        picker.handle_catalog_key(key(KeyCode::Char('i')), 4);
        assert_eq!(picker.query, "i");
    }

    #[test]
    fn select_up_clamps() {
        let mut picker = ProjectPickerState::new(true, 0);
        picker.select_up(5);
        assert_eq!(picker.selected_row, 0);
    }

    #[test]
    fn select_down_clamps() {
        let mut picker = ProjectPickerState::new(true, 0);
        picker.selected_row = 3;
        picker.select_down(5);
        assert_eq!(picker.selected_row, 4);
        picker.select_down(5);
        assert_eq!(picker.selected_row, 4);
    }

    #[test]
    fn select_up_and_down_roundtrip() {
        let mut picker = ProjectPickerState::new(true, 0);
        picker.select_down(5);
        assert_eq!(picker.selected_row, 1);
        picker.select_up(5);
        assert_eq!(picker.selected_row, 0);
    }

    #[test]
    fn reset_to_catalog_clears_state() {
        let mut picker = ProjectPickerState::new(true, 0);
        picker.query = "foo".to_string();
        picker.phase = PickerPhase::WorkspaceSelection;
        picker.selected_row = 3;
        picker.pinned_project_id = Some("x".to_string());
        picker.last_error = Some("err".to_string());
        picker.reset_to_catalog();
        assert_eq!(picker.phase, PickerPhase::Catalog);
        assert_eq!(picker.selected_row, 0);
        assert!(picker.pinned_project_id.is_none());
        assert!(picker.last_error.is_none());
    }

    #[test]
    fn registration_draft_tag_bounds() {
        let mut draft = RegistrationDraft::default();
        for i in 0..20 {
            draft.push_tag(format!("tag{}", i));
        }
        assert_eq!(draft.tags.len(), MAX_REGISTRATION_TAGS);
    }

    #[test]
    fn registration_draft_tag_char_bounds() {
        let mut draft = RegistrationDraft::default();
        // Each tag is ~20 chars, so 10 tags = 200 chars > 128
        for i in 0..10 {
            draft.push_tag(format!("tag-{:04}-extra", i));
        }
        assert!(draft.tags.len() < 10);
    }

    #[test]
    fn registration_draft_desc_bounds() {
        let mut draft = RegistrationDraft::default();
        let long_desc = "x".repeat(500);
        draft.set_description(long_desc);
        assert_eq!(draft.description.len(), MAX_REGISTRATION_DESC_LEN);
    }

    #[test]
    fn truncate_tab_label_short() {
        assert_eq!(truncate_tab_label("hello"), "hello");
    }

    #[test]
    fn truncate_tab_label_exact_boundary() {
        let label = "a".repeat(MAX_TAB_LABEL_LEN);
        assert_eq!(truncate_tab_label(&label).len(), MAX_TAB_LABEL_LEN);
    }

    #[test]
    fn truncate_tab_label_over_boundary() {
        let label = format!("{}extra", "a".repeat(MAX_TAB_LABEL_LEN));
        let result = truncate_tab_label(&label);
        assert!(result.len() <= MAX_TAB_LABEL_LEN);
        // Should not be empty
        assert!(!result.is_empty());
    }

    #[test]
    fn truncate_tab_label_unicode() {
        // 3-byte UTF-8 characters
        let label: String = "é".repeat(30);
        let result = truncate_tab_label(&label);
        // Should truncate at char boundary, not byte boundary
        assert!(!result.is_empty());
        assert!(result.chars().count() <= MAX_TAB_LABEL_LEN);
    }

    #[test]
    fn disambiguate_label_uses_suffix() {
        let result = disambiguate_label("MyProject", "abc123");
        assert!(result.starts_with("MyProject"));
        assert!(result.ends_with("123"));
    }

    #[test]
    fn disambiguate_label_short_suffix() {
        let result = disambiguate_label("MyProject", "ab");
        assert!(result.contains("ab"));
    }

    #[test]
    fn picker_request_lifecycle() {
        let mut picker = ProjectPickerState::new(true, 0);
        let id1 = picker.begin_request();
        assert_eq!(id1, 1);
        assert!(picker.is_request_current(id1));
        let id2 = picker.begin_request();
        assert!(!picker.is_request_current(id1));
        assert!(picker.is_request_current(id2));
    }

    #[test]
    fn max_open_project_tabs_is_reasonable() {
        const {
            assert!(MAX_OPEN_PROJECT_TABS >= 8);
            assert!(MAX_OPEN_PROJECT_TABS <= 64);
        }
    }
}

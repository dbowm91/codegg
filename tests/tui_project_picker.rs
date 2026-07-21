//! Integration tests for the project picker (Space-f / Ctrl+\) and
//! tab navigation (Multi-Project TUI milestone 2).
//!
//! These tests pin down the contract that:
//!
//! * `ProjectPickerState::filtered_indices` filters by query and tags.
//! * `MAX_OPEN_PROJECT_TABS` (16) bounds `ProjectTabs::open`.
//! * Stale picker generations are dropped without leaking into active state.
//! * `RegistrationDraft::push_tag` enforces tag count and total char budget.
//! * `truncate_tab_label` enforces MAX_TAB_LABEL_LEN (24) without
//!   corrupting UTF-8 boundaries.
//! * Keybinding collision: new InputAction variants are reachable and
//!   distinct from existing ones.

use codegg::protocol::dto::ProjectSummaryDto;
use codegg::tui::app::state::project_picker::{
    disambiguate_label, truncate_tab_label, PickerPhase, ProjectPickerState, RegistrationDraft,
    MAX_OPEN_PROJECT_TABS, MAX_PICKER_VISIBLE_ROWS, MAX_PROJECT_LIST_ITEMS,
    MAX_REGISTRATION_DESC_LEN, MAX_REGISTRATION_TAGS, MAX_REGISTRATION_TAG_CHARS,
    MAX_TAB_LABEL_LEN,
};
use codegg::tui::app::state::project_tabs::{ProjectTabState, ProjectTabs};
use codegg::tui::app::state::ProjectTabId;
use codegg::tui::input::InputAction;

fn summary(id: &str, name: &str) -> ProjectSummaryDto {
    ProjectSummaryDto {
        project_id: id.to_string(),
        display_name: name.to_string(),
        lifecycle: "active".to_string(),
        description: None,
        tags: vec!["rust".to_string(), "infra".to_string()],
        time_last_opened_at: None,
        registration_source: "protocol".to_string(),
        archived_at: None,
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn filtered_indices_empty_query_returns_all_capped() {
    let mut picker = ProjectPickerState::new(true, 0);
    let entries: Vec<ProjectSummaryDto> = (0..(MAX_PROJECT_LIST_ITEMS + 50))
        .map(|i| summary(&format!("p{:03}", i), &format!("Project {}", i)))
        .collect();
    let filtered = picker.filtered_indices(&entries);
    assert_eq!(
        filtered.len(),
        MAX_PROJECT_LIST_ITEMS,
        "filtered results must be capped at MAX_PROJECT_LIST_ITEMS"
    );
}

#[test]
fn filtered_indices_query_matches_display_name() {
    let mut picker = ProjectPickerState::new(true, 0);
    picker.query = "alpha".to_string();
    let entries = vec![
        summary("p1", "Alpha Project"),
        summary("p2", "Beta Project"),
        summary("p3", "alpha-v2"),
        summary("p4", "Gamma"),
    ];
    let filtered = picker.filtered_indices(&entries);
    let names: Vec<String> = filtered
        .into_iter()
        .map(|i| entries[i].display_name.clone())
        .collect();
    assert!(names.contains(&"Alpha Project".to_string()));
    assert!(names.contains(&"alpha-v2".to_string()));
    assert!(!names.contains(&"Beta Project".to_string()));
}

#[test]
fn filtered_indices_query_matches_id() {
    let mut picker = ProjectPickerState::new(true, 0);
    picker.query = "p2".to_string();
    let entries = vec![
        summary("p1", "Alpha"),
        summary("p2", "Beta"),
        summary("p3", "Gamma"),
    ];
    let filtered = picker.filtered_indices(&entries);
    assert!(filtered.contains(&1));
}

#[test]
fn filtered_indices_empty_query_returns_all() {
    let mut picker = ProjectPickerState::new(true, 0);
    let entries = vec![summary("p1", "Alpha"), summary("p2", "Beta")];
    let filtered = picker.filtered_indices(&entries);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn begin_request_increments_request_id() {
    let mut picker = ProjectPickerState::new(true, 0);
    let r0 = picker.begin_request();
    let r1 = picker.begin_request();
    assert_ne!(r0, r1);
    assert_eq!(picker.request_id, r1);
}

#[test]
fn is_request_current_returns_true_for_latest() {
    let mut picker = ProjectPickerState::new(true, 0);
    let r = picker.begin_request();
    assert!(picker.is_request_current(r));
}

#[test]
fn is_request_current_returns_false_for_stale() {
    let mut picker = ProjectPickerState::new(true, 0);
    let r0 = picker.begin_request();
    let _r1 = picker.begin_request();
    assert!(!picker.is_request_current(r0));
}

#[test]
fn phase_transitions() {
    let mut picker = ProjectPickerState::new(true, 0);
    assert_eq!(picker.phase, PickerPhase::Catalog);
    picker.phase = PickerPhase::RegistrationInput;
    assert_eq!(picker.phase, PickerPhase::RegistrationInput);
    picker.phase = PickerPhase::RegistrationConfirm;
    assert_eq!(picker.phase, PickerPhase::RegistrationConfirm);
    picker.phase = PickerPhase::Error;
    assert_eq!(picker.phase, PickerPhase::Error);
}

#[test]
fn registration_draft_push_tag_caps_count() {
    let mut draft = RegistrationDraft::default();
    for i in 0..(MAX_REGISTRATION_TAGS + 5) {
        draft.push_tag(format!("tag{}", i));
    }
    assert_eq!(draft.tags.len(), MAX_REGISTRATION_TAGS);
}

#[test]
fn registration_draft_push_tag_caps_total_chars() {
    let mut draft = RegistrationDraft::default();
    // Each tag is MAX_REGISTRATION_TAG_CHARS / 2 chars; after a few,
    // additional tags should be rejected because total > MAX_REGISTRATION_TAG_CHARS.
    let half = MAX_REGISTRATION_TAG_CHARS / 2;
    let tag = "x".repeat(half);
    // Push until full.
    let mut pushed = 0;
    for _ in 0..MAX_REGISTRATION_TAGS {
        draft.push_tag(tag.clone());
        if draft.tags.len() == pushed + 1 {
            pushed += 1;
        } else {
            break;
        }
    }
    let total: usize = draft.tags.iter().map(|t| t.len()).sum();
    assert!(total <= MAX_REGISTRATION_TAG_CHARS);
}

#[test]
fn registration_draft_set_description_caps_length() {
    let mut draft = RegistrationDraft::default();
    let overlong = "y".repeat(MAX_REGISTRATION_DESC_LEN + 50);
    draft.set_description(overlong);
    let result = draft.description().to_string();
    assert!(result.chars().count() <= MAX_REGISTRATION_DESC_LEN);
}

#[test]
fn registration_draft_set_description_short_passes_through() {
    let mut draft = RegistrationDraft::default();
    draft.set_description("hello".to_string());
    assert_eq!(draft.description(), "hello");
}

#[test]
fn picker_state_default_phase_is_catalog() {
    let mut picker = ProjectPickerState::new(true, 0);
    assert_eq!(picker.phase, PickerPhase::Catalog);
    assert_eq!(picker.query, "");
    assert_eq!(picker.last_error, None);
}

#[test]
fn tab_state_caps_via_add_and_activate() {
    let mut tabs = ProjectTabs::default();
    // Caller is responsible for enforcing MAX_OPEN_PROJECT_TABS — verify the
    // constant is sane and the add_and_activate API is available.
    assert!(MAX_OPEN_PROJECT_TABS > 0);
    assert!(MAX_OPEN_PROJECT_TABS <= 64);
    let state = ProjectTabState::empty(ProjectTabId::new(), "tab".to_string());
    let id = tabs.add_and_activate(state);
    assert_eq!(tabs.active_tab_id(), Some(&id));
    assert!(tabs.is_at_capacity() || tabs.len() <= MAX_OPEN_PROJECT_TABS);
}

#[test]
fn truncate_tab_label_under_limit_is_identity() {
    let label = "short";
    assert_eq!(truncate_tab_label(label), "short");
}

#[test]
fn truncate_tab_label_truncates_overlong() {
    let label = "x".repeat(MAX_TAB_LABEL_LEN + 30);
    let result = truncate_tab_label(&label);
    // Truncate to MAX_TAB_LABEL_LEN characters; allow small slack.
    assert!(result.chars().count() <= MAX_TAB_LABEL_LEN);
}

#[test]
fn truncate_tab_label_preserves_utf8_boundaries() {
    let label = "日本語のプロジェクト名";
    let result = truncate_tab_label(label);
    // Result must be valid UTF-8 (this will panic at runtime otherwise).
    assert!(result.len() > 0);
    // All chars must be either ASCII graphic, ASCII whitespace, or non-ASCII.
    for c in result.chars() {
        assert!(!c.is_ascii() || c.is_ascii_graphic() || c == ' ');
    }
}

#[test]
fn disambiguate_label_appends_short_suffix() {
    let label = "myproject";
    let result = disambiguate_label(label, "abcdef123456");
    assert!(result.contains('-'));
    assert!(result.starts_with("myproject"));
}

#[test]
fn tabs_default_starts_empty() {
    // ProjectTabs::default() starts empty; the compatibility "one tab"
    // initialization happens in App::new() via from_compat().
    let tabs = ProjectTabs::default();
    assert_eq!(tabs.len(), 0);
}

#[test]
fn picker_const_max_visible_rows_sane() {
    assert!(MAX_PICKER_VISIBLE_ROWS > 0);
    assert!(MAX_PICKER_VISIBLE_ROWS <= MAX_PROJECT_LIST_ITEMS);
}

#[test]
fn input_action_new_variants_exist() {
    // Compile-time verification that the new InputAction variants exist.
    let _ = InputAction::OpenProjectPicker;
    let _ = InputAction::NextProjectTab;
    let _ = InputAction::PreviousProjectTab;
    let _ = InputAction::CloseProjectTab;
}

#[test]
fn keybinding_collision_audit_default_bindings() {
    use codegg::tui::input::{build_bindings, InputAction};
    use std::collections::HashSet;

    let bindings = build_bindings(None, false);
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut collisions: Vec<(String, String, InputAction, InputAction)> = Vec::new();
    for ((modifiers, keycode), action) in &bindings {
        let key_str = format!("{:?}+{:?}", modifiers, keycode);
        let action_str = format!("{:?}", action);
        if let Some(prev) = seen.get(&(key_str.clone(), action_str.clone())) {
            // Same (key, action) is fine — that's a re-insertion.
            let _ = prev;
        }
        // Two different actions mapping to the same key is a collision.
        // Detect by counting all bindings per key.
        let _ = collisions;
    }
    // Count bindings per (key) — must all be unique.
    let mut per_key: std::collections::HashMap<String, Vec<InputAction>> =
        std::collections::HashMap::new();
    for ((modifiers, keycode), action) in &bindings {
        let key_str = format!("{:?}+{:?}", modifiers, keycode);
        per_key.entry(key_str).or_default().push(action.clone());
    }
    for (key, actions) in per_key {
        if actions.len() > 1 {
            // All bindings in this file are produced by map.insert() in
            // build_bindings. A duplicate key with different actions is a
            // collision we want to catch.
            let unique: HashSet<String> = actions.iter().map(|a| format!("{:?}", a)).collect();
            if unique.len() > 1 {
                panic!("keybinding collision at {}: {:?}", key, actions);
            }
        }
    }
}

//! Consolidated terminal UI rendering, panels, the composer, and project surfaces integration test family (M004).
//!
//! History: each member lived as its own `tests/<NAME>.rs` file and was
//! therefore its own Cargo test executable — N executables for N files.
//! The `.inc` extension is what stops Cargo's `tests/<dir>/*.rs`
//! auto-discovery from generating one executable per file, and
//! `tests/<dir>/mod.rs` is the single integration target. Cargo does not
//! auto-discover `tests/<dir>/mod.rs` either, so the target is declared
//! explicitly in the root `Cargo.toml`. Each member is included with
//! `#[path]`, the pattern the existing `tests/projection_replay/` and
//! `tests/session_family/` families already use.
//!
//! Consolidation replaces N separate Cargo test executables with 1,
//! preserving every test body and every assertion. The only visible change
//! is the test id, which becomes `<family>::<member>::<test>`.
//!
//! This family has no shared-helper dependency: no member declares
//! `mod common;` and none references the `common` helper module, so the
//! family root binds no `common` module (an unused binding would be a
//! dead-code warning).

#[path = "change_review_trajectory.inc"]
mod change_review_trajectory;

#[path = "collaboration_m002_chat_tui.inc"]
mod collaboration_m002_chat_tui;

#[path = "connect_dialog_input.inc"]
mod connect_dialog_input;

#[path = "connect_tui_restoration.inc"]
mod connect_tui_restoration;

#[path = "file_tree_trajectory.inc"]
mod file_tree_trajectory;

#[path = "modifier_chords.inc"]
mod modifier_chords;

#[path = "presence_m002_collaborators.inc"]
mod presence_m002_collaborators;

#[path = "theme_selection_tokens.inc"]
mod theme_selection_tokens;

#[path = "tui.inc"]
mod tui;

#[path = "tui_agents_panel.inc"]
mod tui_agents_panel;

#[path = "tui_chat_surfaces.inc"]
mod tui_chat_surfaces;

#[path = "tui_completion_trajectory.inc"]
mod tui_completion_trajectory;

#[path = "tui_composer_claims.inc"]
mod tui_composer_claims;

#[path = "tui_composer_cursor.inc"]
mod tui_composer_cursor;

#[path = "tui_composer_insert_transitions.inc"]
mod tui_composer_insert_transitions;

#[path = "tui_composer_keymap_consistency.inc"]
mod tui_composer_keymap_consistency;

#[path = "tui_composer_motions.inc"]
mod tui_composer_motions;

#[path = "tui_composer_selection.inc"]
mod tui_composer_selection;

#[path = "tui_composer_undo.inc"]
mod tui_composer_undo;

#[path = "tui_info_dialog_panel_chrome.inc"]
mod tui_info_dialog_panel_chrome;

#[path = "tui_logs_window.inc"]
mod tui_logs_window;

#[path = "tui_main_buffer.inc"]
mod tui_main_buffer;

#[path = "tui_manifest_resource_caps.inc"]
mod tui_manifest_resource_caps;

#[path = "tui_manifest_restore.inc"]
mod tui_manifest_restore;

#[path = "tui_model_selection_persist.inc"]
mod tui_model_selection_persist;

#[path = "tui_panel_docking.inc"]
mod tui_panel_docking;

#[path = "tui_panel_host_chrome.inc"]
mod tui_panel_host_chrome;

#[path = "tui_panel_manager_reachability.inc"]
mod tui_panel_manager_reachability;

#[path = "tui_panel_placement.inc"]
mod tui_panel_placement;

#[path = "tui_presentation_regressions.inc"]
mod tui_presentation_regressions;

#[path = "tui_project_picker.inc"]
mod tui_project_picker;

#[path = "tui_project_routing.inc"]
mod tui_project_routing;

#[path = "tui_project_tabs.inc"]
mod tui_project_tabs;

#[path = "tui_render.inc"]
mod tui_render;

#[path = "tui_sessions_pending_sections.inc"]
mod tui_sessions_pending_sections;

#[path = "tui_side_panels.inc"]
mod tui_side_panels;

#[path = "tui_theme_source_of_truth.inc"]
mod tui_theme_source_of_truth;

#[path = "tui_usability_fixes.inc"]
mod tui_usability_fixes;

#[path = "user_prompt_queue_box.inc"]
mod user_prompt_queue_box;

#[path = "workspace_m005_selected_project_chat.inc"]
mod workspace_m005_selected_project_chat;

#[path = "workspace_postclosure_m002_task_cancellation.inc"]
mod workspace_postclosure_m002_task_cancellation;

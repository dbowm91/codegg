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
pub mod change_review_trajectory;

#[path = "collaboration_m002_chat_tui.inc"]
pub mod collaboration_m002_chat_tui;

#[path = "connect_dialog_input.inc"]
pub mod connect_dialog_input;

#[path = "connect_tui_restoration.inc"]
pub mod connect_tui_restoration;

#[path = "delegated_transcript.inc"]
pub mod delegated_transcript;

#[path = "dialog_dispatch_coverage.inc"]
pub mod dialog_dispatch_coverage;

#[path = "file_picker_surface_corrective.inc"]
pub mod file_picker_surface_corrective;

#[path = "file_tree_trajectory.inc"]
pub mod file_tree_trajectory;

#[path = "modifier_chords.inc"]
pub mod modifier_chords;

#[path = "nested_runtime_command_dispatch.inc"]
pub mod nested_runtime_command_dispatch;

#[path = "presence_m002_collaborators.inc"]
pub mod presence_m002_collaborators;

#[path = "theme_selection_tokens.inc"]
pub mod theme_selection_tokens;

#[path = "tui.inc"]
pub mod tui;

#[path = "tui_agents_panel.inc"]
pub mod tui_agents_panel;

#[path = "tui_chat_surfaces.inc"]
pub mod tui_chat_surfaces;

#[path = "tui_completion_trajectory.inc"]
pub mod tui_completion_trajectory;

#[path = "tui_composer_claims.inc"]
pub mod tui_composer_claims;

#[path = "tui_composer_cursor.inc"]
pub mod tui_composer_cursor;

#[path = "tui_composer_insert_transitions.inc"]
pub mod tui_composer_insert_transitions;

#[path = "tui_composer_keymap_consistency.inc"]
pub mod tui_composer_keymap_consistency;

#[path = "tui_composer_motions.inc"]
pub mod tui_composer_motions;

#[path = "tui_composer_selection.inc"]
pub mod tui_composer_selection;

#[path = "tui_composer_undo.inc"]
pub mod tui_composer_undo;

#[path = "tui_composer_wrap.inc"]
pub mod tui_composer_wrap;

#[path = "transcript_follow.inc"]
pub mod transcript_follow;

#[path = "tui_formatted_json_detail_view.inc"]
pub mod tui_formatted_json_detail_view;

#[path = "tui_info_dialog_panel_chrome.inc"]
pub mod tui_info_dialog_panel_chrome;

#[path = "tui_logs_window.inc"]
pub mod tui_logs_window;

#[path = "tui_main_buffer.inc"]
pub mod tui_main_buffer;

#[path = "tui_main_buffer_collapsed_layout.inc"]
pub mod tui_main_buffer_collapsed_layout;

#[path = "tui_main_buffer_row_accounting.inc"]
pub mod tui_main_buffer_row_accounting;

#[path = "tui_main_buffer_row_precise_disclosure.inc"]
pub mod tui_main_buffer_row_precise_disclosure;

#[path = "tui_main_buffer_word_wrap.inc"]
pub mod tui_main_buffer_word_wrap;

#[path = "tui_manifest_resource_caps.inc"]
pub mod tui_manifest_resource_caps;

#[path = "tui_manifest_restore.inc"]
pub mod tui_manifest_restore;

#[path = "tui_model_selection_persist.inc"]
pub mod tui_model_selection_persist;

#[path = "tui_panel_docking.inc"]
pub mod tui_panel_docking;

#[path = "tui_panel_host_chrome.inc"]
pub mod tui_panel_host_chrome;

#[path = "tui_panel_manager_reachability.inc"]
pub mod tui_panel_manager_reachability;

#[path = "tui_panel_placement.inc"]
pub mod tui_panel_placement;

#[path = "tui_presentation_regressions.inc"]
pub mod tui_presentation_regressions;

#[path = "tui_project_picker.inc"]
pub mod tui_project_picker;

#[path = "tui_project_routing.inc"]
pub mod tui_project_routing;

#[path = "tui_project_tabs.inc"]
pub mod tui_project_tabs;

#[path = "tui_render_cost.inc"]
pub mod tui_render_cost;

#[path = "tui_render.inc"]
pub mod tui_render;

#[path = "tui_sessions_pending_sections.inc"]
pub mod tui_sessions_pending_sections;

#[path = "tui_side_panels.inc"]
pub mod tui_side_panels;

#[path = "tui_theme_source_of_truth.inc"]
pub mod tui_theme_source_of_truth;

#[path = "tui_usability_fixes.inc"]
pub mod tui_usability_fixes;

#[path = "user_prompt_queue_box.inc"]
pub mod user_prompt_queue_box;

#[path = "workspace_m005_selected_project_chat.inc"]
pub mod workspace_m005_selected_project_chat;

#[path = "workspace_postclosure_m002_task_cancellation.inc"]
pub mod workspace_postclosure_m002_task_cancellation;

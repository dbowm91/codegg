//! Snapshot helpers for the persistence service.
//!
//! Bridges between [`crate::tui::app::state::project_tabs::ProjectTabs`]
//! and the persisted [`TuiWorkspaceManifest`] format. The TUI
//! produces snapshots on state changes; the persistence service
//! debounces and writes them to disk.
//!
//! Snapshots are intentionally read-only projections: nothing in the
//! `ProjectTabs` collection is mutated by snapshotting.

use crate::tui::app::state::manifest::{
    ManifestPreferences, PersistedProjectTab, TuiWorkspaceManifest,
};
use crate::tui::app::state::persistence::PersistedSnapshot;
use crate::tui::app::state::project_tabs::ProjectTabs;

/// Build a [`PersistedSnapshot`] from the current `ProjectTabs`
/// state. Returns a snapshot whose manifest contains one entry per
/// open tab in display order. Active tab id and active session id
/// are recorded when present. Frontend-local `ProjectTabId`s are
/// NOT persisted — the daemon-typed `project_id` is the durable
/// identity.
pub fn snapshot_from_tabs(tabs: &ProjectTabs) -> PersistedSnapshot {
    let ordered = tabs.ordered();
    let mut manifest = TuiWorkspaceManifest::default();
    manifest.ordered_tabs = ordered
        .iter()
        .enumerate()
        .map(|(idx, tab)| PersistedProjectTab {
            project_id: tab.project_id.clone(),
            workspace_id: tab.workspace_id.clone(),
            session_id: tab.session_id.clone(),
            label_hint: Some(bounded_label(&tab.label)),
            selected_model_id: non_empty(tab.model.clone()),
            selected_agent: non_empty(tab.agent.clone()),
            order_key: Some(order_key_for(idx, &tab.tab_id)),
        })
        .collect();

    if let Some(active) = tabs.active() {
        manifest.active_project_id = active.project_id.clone();
        manifest.active_session_id = active.session_id.clone();
    }

    // Snapshot preferences with a sane default sidebar_visible.
    manifest.preferences = ManifestPreferences {
        sidebar_visible: Some(true),
    };

    PersistedSnapshot { manifest }
}

/// Build an empty snapshot. Used on startup before any tabs have
/// been restored.
pub fn empty_snapshot() -> PersistedSnapshot {
    PersistedSnapshot {
        manifest: TuiWorkspaceManifest::default(),
    }
}

fn bounded_label(label: &str) -> String {
    if label.chars().count() <= crate::tui::app::state::manifest::MAX_PERSISTED_LABEL_LEN {
        label.to_string()
    } else {
        label
            .chars()
            .take(crate::tui::app::state::manifest::MAX_PERSISTED_LABEL_LEN)
            .collect()
    }
}

fn non_empty(input: String) -> Option<String> {
    if input.trim().is_empty() {
        None
    } else {
        Some(input)
    }
}

fn order_key_for(
    index: usize,
    tab_id: &crate::tui::app::state::project_tabs::ProjectTabId,
) -> String {
    // Deterministic, sortable, never collides with daemon ids.
    // Format: `idx#uuid` so we can sort lexicographically by idx
    // prefix and never confuse with a UUID-shape.
    format!("{:04}#{}", index, tab_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::project_tabs::{ProjectTabState, ProjectTabs};

    fn tab(label: &str, project_id: Option<&str>) -> ProjectTabState {
        let mut t = ProjectTabState::empty(
            crate::tui::app::state::project_tabs::ProjectTabId::new(),
            label.into(),
        );
        t.project_id = project_id.map(str::to_string);
        t
    }

    #[test]
    fn empty_tabs_yield_empty_manifest() {
        let tabs = ProjectTabs::new();
        let snap = snapshot_from_tabs(&tabs);
        assert!(snap.manifest.ordered_tabs.is_empty());
        assert!(snap.manifest.active_project_id.is_none());
    }

    #[test]
    fn snapshot_includes_active_project_and_session() {
        let mut tabs = ProjectTabs::new();
        let mut a = tab("a", Some("proj-a"));
        a.session_id = Some("sess-1".into());
        tabs.add_and_activate(a);
        let snap = snapshot_from_tabs(&tabs);
        assert_eq!(snap.manifest.active_project_id.as_deref(), Some("proj-a"));
        assert_eq!(snap.manifest.active_session_id.as_deref(), Some("sess-1"));
        assert_eq!(snap.manifest.ordered_tabs.len(), 1);
    }

    #[test]
    fn snapshot_omits_empty_model_and_agent() {
        let mut tabs = ProjectTabs::new();
        tabs.add_and_activate(tab("a", Some("p1")));
        let snap = snapshot_from_tabs(&tabs);
        assert!(snap.manifest.ordered_tabs[0].selected_model_id.is_none());
        assert!(snap.manifest.ordered_tabs[0].selected_agent.is_none());
    }

    #[test]
    fn order_key_is_stable() {
        let mut tabs = ProjectTabs::new();
        tabs.add_and_activate(tab("a", Some("p1")));
        tabs.add_tab(tab("b", Some("p2")));
        let s1 = snapshot_from_tabs(&tabs);
        let s2 = snapshot_from_tabs(&tabs);
        // Two calls produce identical manifests (tab ids are stable
        // within one process).
        assert_eq!(s1.manifest, s2.manifest);
    }

    #[test]
    fn snapshot_includes_label_hint() {
        let mut tabs = ProjectTabs::new();
        tabs.add_and_activate(tab("My Project", Some("p1")));
        let snap = snapshot_from_tabs(&tabs);
        assert_eq!(
            snap.manifest.ordered_tabs[0].label_hint.as_deref(),
            Some("My Project")
        );
    }

    #[test]
    fn empty_snapshot_is_default() {
        let snap = empty_snapshot();
        assert!(snap.manifest.ordered_tabs.is_empty());
    }
}

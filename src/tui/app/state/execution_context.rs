//! Explicit project execution context for TUI dispatch.
//!
//! The active project tab is the only frontend source of routing authority.
//! This value is resolved before work is spawned and then moved into the
//! task, so a later tab switch cannot retarget an already-dispatched action.

use std::path::PathBuf;

use super::project_tabs::{ProjectTabState, ProjectTabs};

/// Immutable project/workspace/session locators captured for one TUI action.
///
/// `workspace_root` is a locator, not an identity. The typed/string IDs are
/// retained separately so callers can preserve daemon routing fields without
/// deriving identity from a path. `project_key` is only for legacy protocol
/// requests that predate the canonical project binding; it falls back to the
/// explicit bootstrap root for compatibility and must not be persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectExecutionContext {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace_root: PathBuf,
}

impl ProjectExecutionContext {
    pub fn from_tab(tab: &ProjectTabState) -> Result<Self, String> {
        let workspace_root = tab.workspace_root.clone().ok_or_else(|| {
            format!(
                "project '{}' has no resolved workspace root; select or restore the workspace before running this action",
                tab.project_id.as_deref().unwrap_or("active tab")
            )
        })?;
        if workspace_root.as_os_str().is_empty() {
            return Err("active project has an empty workspace root".to_string());
        }
        Ok(Self {
            project_id: tab.project_id.clone(),
            workspace_id: tab.workspace_id.clone(),
            session_id: tab.session_id.clone(),
            workspace_root,
        })
    }

    /// Compatibility key for legacy requests whose `project_id` field still
    /// accepts the old directory-shaped value.
    pub fn project_key(&self) -> String {
        self.project_id
            .clone()
            .unwrap_or_else(|| self.workspace_root.to_string_lossy().into_owned())
    }
}

/// Resolve the active tab once at the dispatch boundary.
pub fn resolve_active(tabs: &ProjectTabs) -> Result<ProjectExecutionContext, String> {
    let tab = tabs.active().ok_or_else(|| {
        "no active project tab; choose a project before running this action".to_string()
    })?;
    ProjectExecutionContext::from_tab(tab)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::project_tabs::ProjectTabId;

    fn tabs_with_root(root: &str) -> ProjectTabs {
        let mut tabs = ProjectTabs::new();
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "project".into());
        tab.project_id = Some("project-b".into());
        tab.workspace_id = Some("workspace-b".into());
        tab.session_id = Some("session-b".into());
        tab.workspace_root = Some(PathBuf::from(root));
        tabs.add_and_activate(tab);
        tabs
    }

    #[test]
    fn resolves_all_active_tab_locators_without_process_cwd() {
        let context = resolve_active(&tabs_with_root("/projects/b")).unwrap();
        assert_eq!(context.project_id.as_deref(), Some("project-b"));
        assert_eq!(context.workspace_id.as_deref(), Some("workspace-b"));
        assert_eq!(context.session_id.as_deref(), Some("session-b"));
        assert_eq!(context.workspace_root, PathBuf::from("/projects/b"));
    }

    #[test]
    fn missing_active_tab_fails_closed() {
        let error = resolve_active(&ProjectTabs::new()).unwrap_err();
        assert!(error.contains("no active project tab"));
    }

    #[test]
    fn missing_workspace_root_fails_closed() {
        let mut tabs = ProjectTabs::new();
        tabs.add_and_activate(ProjectTabState::empty(
            ProjectTabId::new(),
            "project".into(),
        ));
        let error = resolve_active(&tabs).unwrap_err();
        assert!(error.contains("no resolved workspace root"));
    }

    #[test]
    fn switching_tabs_refreshes_project_command_catalog_without_chdir() {
        let roots = tempfile::tempdir().unwrap();
        let root_a = roots.path().join("a");
        let root_b = roots.path().join("b");
        std::fs::create_dir_all(root_a.join("commands")).unwrap();
        std::fs::create_dir_all(root_b.join("commands")).unwrap();
        std::fs::write(root_a.join("commands/a-only.md"), "---\n---\nA\n").unwrap();
        std::fs::write(root_b.join("commands/b-only.md"), "---\n---\nB\n").unwrap();

        let mut app = crate::tui::app::App::new_for_testing(root_a.display().to_string());
        let mut tab_b = ProjectTabState::empty(ProjectTabId::new(), "b".into());
        tab_b.project_id = Some("project-b".into());
        tab_b.workspace_root = Some(root_b.clone());
        let tab_b_id = tab_b.tab_id.clone();
        app.project_tabs.add_tab(tab_b);

        assert!(app
            .command_registry
            .find_by_name_or_alias("/a-only")
            .is_some());
        assert!(app.switch_active_tab(&tab_b_id));
        assert!(app
            .command_registry
            .find_by_name_or_alias("/b-only")
            .is_some());
        assert!(app
            .command_registry
            .find_by_name_or_alias("/a-only")
            .is_none());
    }
}

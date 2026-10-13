//! M006-F: agent configuration editing through the TUI editor.
//!
//! The editor buffer is the only text surface in this module. Agent TOML is
//! read through the same `document.v1` surface as any other file, written
//! through the document controller, and re-read through the daemon asset
//! refresh — there is **no** second write path, and no local file handling.
//!
//! # Two scopes, and the box always says which one
//!
//! An agent config can live in one of two places, and they are not
//! interchangeable:
//!
//! | Scope   | Location                    | Who sees it                       |
//! |---------|-----------------------------|-----------------------------------|
//! | project | `<root>/.codegg/agents/`    | this project only                 |
//! | global  | `~/.config/codegg/agents/`  | every project on this machine     |
//!
//! They are reached through **different daemon authorities**
//! (`DocumentScope::Workspace` vs `DocumentScope::GlobalAgent`), not through
//! one authority with two paths. The global root is *outside* every
//! workspace, so workspace containment cannot reach it; rather than relaxing
//! that containment, the daemon resolves a second, separately named root that
//! accepts only a bare `<name>.toml`. The global scope is therefore
//! **stricter** than the workspace scope, never wider.
//!
//! Because the two are invisible in the path (`build.toml` reads the same at
//! either scope) but differ enormously in blast radius, [`scope_label`] and
//! the row badge are not decoration — a user who `:wq`s a global agent has
//! just changed what every project does, and the UI has to say so before and
//! after.
//!
//! Built-in agents (`build`, `explore`, …) are compiled into
//! `src/agent/builtins/generated.rs` and have **no file of their own**. They
//! are "edited" the way the registry already supports: by writing an override.
//! That is what makes *restore* exact — removing the override falls back to
//! the compiled definition with no saved copy to go stale.

use crate::agent::asset_context::{AssetContext, AssetContextBuilder, ProjectId};
use crate::agent::registry::{AgentRegistry, AgentSourceKind};
use crate::config::schema::Config;
use crate::tui::app::App;
use codegg_protocol::document::DocumentScope;

/// Project-relative directory holding user-authored agent TOML.
pub const PROJECT_AGENT_DIR: &str = ".codegg/agents";

/// Filename of one agent config inside [`PROJECT_AGENT_DIR`].
pub fn agent_file_name(name: &str) -> String {
    format!("{name}.toml")
}

/// Workspace-relative path of one project agent config.
pub fn project_agent_path(name: &str) -> String {
    format!("{PROJECT_AGENT_DIR}/{}", agent_file_name(name))
}

/// Path of one **global** agent config.
///
/// A bare filename, not a joined path: the global scope confines documents to
/// exactly one file in the agent directory and rejects anything with a
/// separator. Building the path here — rather than joining a discovered root
/// the TUI does not own — keeps that rule in one place.
pub fn global_agent_path(name: &str) -> String {
    agent_file_name(name)
}

/// Human label for a scope, used in badges, toasts, and the editor status row.
pub fn scope_label(scope: DocumentScope) -> &'static str {
    match scope {
        DocumentScope::Workspace => "project",
        DocumentScope::GlobalAgent => "global",
    }
}

/// Whether `name` is usable as an agent filename.
///
/// Deliberately strict and deliberately small: the name becomes a path
/// segment, so anything that could escape the directory or surprise a shell
/// is refused here rather than at the daemon. The daemon applies the same rule
/// independently — this is the friendly copy, not the enforcement point.
pub fn is_valid_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// The starting template for a brand-new agent.
///
/// `:wq` on this file produces a loadable agent with no required edits —
/// `name` and `description` are the only fields the loader insists on, and a
/// blank agent that fails to load teaches the user nothing.
pub fn default_agent_template(name: &str, scope: DocumentScope) -> String {
    let scope_note = match scope {
        DocumentScope::Workspace => {
            "This file lives in the project's .codegg/agents/ — it applies to this project only."
        }
        DocumentScope::GlobalAgent => {
            "This file lives in the global agent directory — it applies to EVERY project on this machine."
        }
    };
    format!(
        r#"# Agent definition for `{name}`.
# {scope_note}
# Saved with :wq — the agent appears in /agents only after a successful save.

[agent]
name = "{name}"
description = "Describe what this agent does"
mode = "Subagent"

# Uncomment to narrow or widen what this agent may do.
# [agent.permissions]
# write = "ask"
# bash = "ask"

# Uncomment to pick a model explicitly.
# model = ""
"#
    )
}

/// Provenance of one agent as the `/agents` box presents it.
///
/// Three cases rather than two, because the *available actions* differ three
/// ways: a compiled default cannot be deleted, a user-authored agent at
/// either scope can, and only a default can be restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOrigin {
    /// Compiled into the binary; overridable but not deletable.
    Default,
    /// A user-authored config in the global agent directory.
    Global,
    /// A user-authored config in this project's `.codegg/agents/`.
    Project,
}

impl AgentOrigin {
    /// Badge shown next to the name.
    pub fn badge(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Global => "global",
            Self::Project => "project",
        }
    }

    /// The authority root this origin's file lives under.
    pub fn scope(self) -> DocumentScope {
        match self {
            Self::Default | Self::Project => DocumentScope::Workspace,
            Self::Global => DocumentScope::GlobalAgent,
        }
    }

    /// Whether a delete action applies.
    ///
    /// Defaults are compiled in, so deleting one would only remove the
    /// override and leave the agent present — a delete that does not delete
    /// is worse than no delete.
    pub fn can_delete(self) -> bool {
        matches!(self, Self::Global | Self::Project)
    }

    /// Whether an "undo my override" action applies.
    pub fn can_restore(self) -> bool {
        matches!(self, Self::Default)
    }

    /// Whether this agent is backed by a file the user wrote.
    pub fn is_user_authored(self) -> bool {
        !matches!(self, Self::Default)
    }
}

/// One row of the agent management box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub name: String,
    pub description: String,
    pub mode: String,
    pub origin: AgentOrigin,
    /// Path relative to [`AgentRow::scope`]'s authority root. `None` for a
    /// default with no override written yet — the "create" case.
    pub path: Option<String>,
    /// Which authority root `path` is relative to.
    ///
    /// Separate from `origin` because a default's override can live at
    /// *either* scope: the origin says what the agent is, the scope says
    /// where the file that shadows it is.
    pub scope: DocumentScope,
    /// Whether an override currently suppresses this agent.
    pub disabled: bool,
}

impl AgentRow {
    /// The editor target for this row, when it has a file.
    pub fn target(&self) -> Option<(DocumentScope, String)> {
        self.path.clone().map(|path| (self.scope, path))
    }
}

/// Whether an agent name collides with an existing agent.
pub fn name_taken(name: &str, rows: &[AgentRow]) -> bool {
    rows.iter().any(|row| row.name == name)
}

/// Build the agent rows for the active workspace.
///
/// Classifies by provenance: the registry reports which file each resolved
/// agent came from, and a user-authored source at either scope makes it an
/// agent the user owns.
///
/// When a default carries overrides at **both** scopes, the project file wins
/// — the registry layers project over global, so the project file is the
/// definition actually in effect. Presenting the effective one is the honest
/// choice: editing the global file in that state would change nothing the
/// user can observe until the project file is removed.
pub fn agent_rows_for_root(workspace_root: &std::path::Path) -> Vec<AgentRow> {
    let config = Config::load_or_default();
    let ctx: AssetContext = AssetContextBuilder::new()
        .with_synthetic_project_id(ProjectId::new())
        .with_workspace_root(workspace_root)
        .build()
        .expect("workspace root is valid");
    let registry = match AgentRegistry::load_for_context(&config, &ctx) {
        Ok(registry) => registry,
        Err(_) => return Vec::new(),
    };
    registry
        .list()
        .map(|resolved| {
            let project_source = resolved
                .sources
                .iter()
                .find(|source| source.kind == AgentSourceKind::ProjectFile);
            let global_source = resolved
                .sources
                .iter()
                .find(|source| source.kind == AgentSourceKind::GlobalFile);
            let (origin, scope, path) = match (project_source, global_source) {
                (Some(source), _) => (
                    AgentOrigin::Project,
                    DocumentScope::Workspace,
                    source.path.as_ref().map(|path| {
                        path.strip_prefix(workspace_root)
                            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
                    }),
                ),
                (None, Some(source)) => (
                    AgentOrigin::Global,
                    DocumentScope::GlobalAgent,
                    // Only the filename travels: the global scope accepts a
                    // bare name and the TUI never constructs the absolute
                    // path the daemon will resolve.
                    source
                        .path
                        .as_ref()
                        .and_then(|path| path.file_name())
                        .map(|name| name.to_string_lossy().into_owned()),
                ),
                (None, None) => (AgentOrigin::Default, DocumentScope::Workspace, None),
            };
            AgentRow {
                name: resolved.agent.name.clone(),
                description: resolved.agent.description.clone(),
                mode: format!("{:?}", resolved.agent.mode),
                origin,
                path,
                scope,
                disabled: false,
            }
        })
        .collect()
}

/// Result of validating a `:new` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewAgentOutcome {
    /// Opened the template for this new name.
    Created(NewAgentTarget),
    /// Opened the existing config for editing.
    Existing(NewAgentTarget),
    /// Refused; the message names the reason.
    Refused(String),
}

/// Where a `:new` request should land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentTarget {
    pub name: String,
    pub scope: DocumentScope,
    pub path: String,
}

/// Split an optional leading scope word off a `:new` argument.
///
/// `:new foo` uses `default_scope` and `:new global foo` is global. A bare
/// `:new global` is an agent *named* `global` at `default_scope`, not a scope
/// word with no name — the scope word is only consumed when a name follows
/// it, so no agent name is unreachable.
pub fn split_scope_word(argument: &str, default_scope: DocumentScope) -> (DocumentScope, String) {
    let mut parts = argument.split_whitespace();
    let first = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    if !rest.is_empty() {
        match first {
            "global" => return (DocumentScope::GlobalAgent, rest.join(" ")),
            "project" => return (DocumentScope::Workspace, rest.join(" ")),
            _ => {}
        }
    }
    (default_scope, argument.trim().to_string())
}

/// Decide what `:new` should do.
///
/// A name that already exists opens its config rather than refusing, because
/// "create" is how a user reaches *edit* as well and a second definition of
/// the same name would only shadow the first.
pub fn resolve_new_agent(
    argument: Option<String>,
    default_scope: DocumentScope,
    rows: &[AgentRow],
) -> NewAgentOutcome {
    let raw = argument.unwrap_or_default();
    let (scope, name) = split_scope_word(&raw, default_scope);
    if name.is_empty() {
        return NewAgentOutcome::Refused(
            "`:new` needs a name — try `:new my-agent`, or `:new global my-agent`".to_string(),
        );
    }
    if !is_valid_agent_name(&name) {
        return NewAgentOutcome::Refused(format!(
            "`{name}` is not a usable agent name — use letters, digits, `-` and `_`"
        ));
    }
    let path = match scope {
        DocumentScope::GlobalAgent => global_agent_path(&name),
        DocumentScope::Workspace => project_agent_path(&name),
    };
    let target = NewAgentTarget {
        name: name.clone(),
        scope,
        path: path.clone(),
    };
    if name_taken(&name, rows) {
        // An existing name may be defined at the *other* scope. Reuse the
        // row's real scope so `:new global build` edits the global file
        // rather than creating a second, shadowed definition.
        if let Some(row) = rows.iter().find(|row| row.name == name) {
            if let Some((row_scope, row_path)) = row.target() {
                return NewAgentOutcome::Existing(NewAgentTarget {
                    name,
                    scope: row_scope,
                    path: row_path,
                });
            }
        }
        return NewAgentOutcome::Existing(target);
    }
    NewAgentOutcome::Created(target)
}

/// The TOML that writes `disable = true` for `name`.
///
/// Written as a normal override file so the daemon applies the same overlay it
/// already reads, rather than inventing a second suppression mechanism. The
/// scope note makes the blast radius explicit at the point of creation.
pub fn disabled_override_toml(name: &str, scope: DocumentScope) -> String {
    let scope_note = match scope {
        DocumentScope::Workspace => "in the project's .codegg/agents/",
        DocumentScope::GlobalAgent => "in the global agent directory",
    };
    format!(
        r#"# Override that suppresses the built-in `{name}` agent.
# Saved {scope_note}.
# Delete this file (or use the box's restore action) to bring the default back.

[agent]
name = "{name}"
disable = true
"#
    )
}

/// Implement `:new`, routing through the normal editor open path.
///
/// `name` may be empty (refused with a message) and may carry a leading
/// scope word; `default_scope` is what applies when it does not.
pub fn create_agent_draft(app: &mut App, name: &str, default_scope: DocumentScope) {
    let workspace_root = match app.project_execution_context() {
        Ok(context) => context.workspace_root,
        Err(_) => {
            app.messages_state
                .toasts
                .warning("Agent editing needs a project tab — select one first");
            return;
        }
    };
    let rows = agent_rows_for_root(&workspace_root);
    let argument = if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    };
    match resolve_new_agent(argument, default_scope, &rows) {
        NewAgentOutcome::Refused(message) => {
            app.messages_state.toasts.warning(&message);
        }
        NewAgentOutcome::Existing(target) => {
            super::editor::open_editor_in(app, &target.path, target.scope);
        }
        NewAgentOutcome::Created(target) => {
            // Seed the template through the same open path a file uses. The
            // daemon creates the file on request, so there is no local write
            // here.
            super::editor::open_editor_template_in(
                app,
                &target.path,
                &default_agent_template(&target.name, target.scope),
                target.scope,
            );
        }
    }
}

/// Toggle suppression for `row` and report what happened.
///
/// A default with an override already on disk is *edited*, not re-created:
/// writing the template again would be refused as an overwrite, which reads
/// as a broken button rather than as "you already did this".
pub fn toggle_agent_disabled(app: &mut App, row: &AgentRow) {
    match row.target() {
        // A file already exists at this agent's effective scope — open it so
        // the user can flip `disable` by hand.
        Some((scope, path)) => super::editor::open_editor_in(app, &path, scope),
        // A default with no override yet: seed one at project scope. Project
        // is the conservative choice — it shadows the default for this
        // project only, rather than silently changing every project.
        None => {
            let scope = DocumentScope::Workspace;
            let path = project_agent_path(&row.name);
            super::editor::open_editor_template_in(
                app,
                &path,
                &disabled_override_toml(&row.name, scope),
                scope,
            );
        }
    }
}

/// Label for the primary action on `row`, used by the box footer.
pub fn primary_action_label(row: &AgentRow) -> &'static str {
    match row.origin {
        AgentOrigin::Default => "override",
        AgentOrigin::Global | AgentOrigin::Project => "edit",
    }
}

/// Resolve one agents-panel action against the loaded rows.
///
/// The panel hides an unavailable verb, and this refuses one anyway — the
/// panel and this function are the same rule reached two ways, so a future
/// keybind cannot bypass the provenance check the box relies on.
pub fn apply_agent_selection(
    app: &mut App,
    selection: crate::tui::panels::agents::AgentSelection,
    name: &str,
) {
    use crate::tui::panels::agents::AgentSelection;

    let workspace_root = match app.project_execution_context() {
        Ok(context) => context.workspace_root,
        Err(error) => {
            app.messages_state.toasts.warning(&format!(
                "Agent actions need a project tab — select one first ({error})"
            ));
            return;
        }
    };
    let rows = agent_rows_for_root(&workspace_root);
    if matches!(selection, AgentSelection::New) {
        // `n` with no argument opens the project-scoped template; `:new
        // global <name>` is the explicit way to create a global one.
        crate::tui::commands::editor::open_agent_template(app, name, DocumentScope::Workspace);
        return;
    }
    let Some(row) = rows.iter().find(|row| row.name == name) else {
        app.messages_state
            .toasts
            .warning(&format!("Agent `{name}` is no longer defined"));
        return;
    };
    match selection {
        AgentSelection::Edit => match row.target() {
            Some((scope, path)) => super::editor::open_editor_in(app, &path, scope),
            None => app.messages_state.toasts.warning(&format!(
                "`{}` is a built-in agent with no config file yet — press `t` to override it",
                row.name
            )),
        },
        AgentSelection::ToggleDisabled => toggle_agent_disabled(app, row),
        AgentSelection::Delete => {
            if !row.origin.can_delete() {
                app.messages_state.toasts.warning(&format!(
                    "`{}` is a built-in agent and cannot be deleted — deactivate it instead",
                    row.name
                ));
                return;
            }
            delete_agent_config(app, row);
        }
        AgentSelection::Restore => {
            if !can_restore(row) {
                app.messages_state
                    .toasts
                    .info(&format!("`{}` has no override to restore", row.name));
                return;
            }
            let Some((scope, path)) = row.target() else {
                return;
            };
            super::editor::delete_file_in(app, &path, scope);
        }
        AgentSelection::New => unreachable!("handled above"),
    }
}

/// Remove a user-authored agent's config file.
///
/// Routed through the editor's daemon-owned delete so the daemon owns the
/// unlink and its containment check; the TUI never removes a file itself.
fn delete_agent_config(app: &mut App, row: &AgentRow) {
    let Some((scope, path)) = row.target() else {
        return;
    };
    super::editor::delete_file_in(app, &path, scope);
}

/// Whether a restore action would do anything for `row`.
///
/// Restoring means dropping an override, so it is only meaningful for a
/// default that currently has one.
pub fn can_restore(row: &AgentRow) -> bool {
    row.origin.can_restore() && row.path.is_some()
}

/// Commands the `:` line offers inside an agent config.
///
/// Kept beside the agent flow so `:wq` in an agent file is documented where
/// the file is created rather than only in the editor's own help.
pub const AGENT_EDITOR_HINT: &str = ":wq saves the agent and refreshes /agents";

/// Whether a path is an agent config **at the given scope**.
///
/// Scope-qualified on purpose. Under `Workspace` the file must be a direct
/// child TOML of the project agent directory. Under `GlobalAgent` it must be
/// a bare `<name>.toml` — the same rule the daemon enforces. Matching on the
/// path alone would treat a workspace file that happens to be called
/// `build.toml` as a global agent and refresh the wrong registry.
pub fn is_agent_config_path(path: &str, scope: DocumentScope) -> bool {
    let normalized = path.replace('\\', "/");
    match scope {
        DocumentScope::Workspace => {
            let Some(rest) = normalized.strip_prefix(&format!("{PROJECT_AGENT_DIR}/")) else {
                return false;
            };
            !rest.is_empty() && rest.ends_with(".toml") && !rest.contains('/')
        }
        DocumentScope::GlobalAgent => DocumentScope::GlobalAgent
            .validate_global_agent_filename(&normalized)
            .is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_at(origin: AgentOrigin, path: Option<&str>) -> AgentRow {
        let scope = match origin {
            AgentOrigin::Global => DocumentScope::GlobalAgent,
            _ => DocumentScope::Workspace,
        };
        AgentRow {
            name: "build".into(),
            description: "test".into(),
            mode: "Primary".into(),
            origin,
            path: path.map(str::to_string),
            scope,
            disabled: false,
        }
    }

    fn project_row(name: &str) -> AgentRow {
        let mut row = row_at(AgentOrigin::Project, Some(&project_agent_path(name)));
        row.name = name.to_string();
        row
    }

    fn global_row(name: &str) -> AgentRow {
        let mut row = row_at(AgentOrigin::Global, Some(&global_agent_path(name)));
        row.name = name.to_string();
        row
    }

    fn default_row(name: &str) -> AgentRow {
        let mut row = row_at(AgentOrigin::Default, None);
        row.name = name.to_string();
        row
    }

    #[test]
    fn a_default_agent_cannot_be_deleted_but_a_user_one_can_at_either_scope() {
        // Deleting a built-in would only remove its override and leave the
        // agent present, so the verb must not exist for it.
        assert!(!AgentOrigin::Default.can_delete());
        assert!(AgentOrigin::Global.can_delete());
        assert!(AgentOrigin::Project.can_delete());
    }

    #[test]
    fn origin_and_scope_are_separate_axes() {
        // A default's override can land at either scope, which is why `scope`
        // is carried per row rather than derived from the origin badge.
        assert_eq!(AgentOrigin::Default.scope(), DocumentScope::Workspace);
        assert_eq!(AgentOrigin::Global.scope(), DocumentScope::GlobalAgent);
        assert_eq!(AgentOrigin::Project.scope(), DocumentScope::Workspace);
        let mut overridden = default_row("build");
        overridden.scope = DocumentScope::GlobalAgent;
        overridden.path = Some(global_agent_path("build"));
        assert_eq!(
            overridden.target(),
            Some((DocumentScope::GlobalAgent, "build.toml".into()))
        );
    }

    #[test]
    fn only_a_default_with_an_override_can_be_restored() {
        // A default with no override file has nothing to drop, so there is
        // nothing to restore — the verb must not be offered.
        assert!(!can_restore(&default_row("build")));
        let mut overridden = default_row("build");
        overridden.path = Some(project_agent_path("build"));
        assert!(can_restore(&overridden));
        // A user-authored agent is deleted, not restored.
        assert!(!can_restore(&project_row("mine")));
        assert!(!can_restore(&global_row("mine")));
    }

    #[test]
    fn agent_names_reject_anything_that_could_escape_the_directory() {
        assert!(is_valid_agent_name("reviewer"));
        assert!(is_valid_agent_name("my-agent_2"));
        // Each of these would either traverse or confuse a path.
        assert!(!is_valid_agent_name(""));
        assert!(!is_valid_agent_name("../evil"));
        assert!(!is_valid_agent_name("a/b"));
        assert!(!is_valid_agent_name(".."));
        assert!(!is_valid_agent_name("has space"));
        assert!(!is_valid_agent_name(&"x".repeat(65)));
    }

    #[test]
    fn new_defaults_to_project_scope_and_a_leading_word_can_pick_global() {
        assert_eq!(
            split_scope_word("reviewer", DocumentScope::Workspace),
            (DocumentScope::Workspace, "reviewer".to_string())
        );
        assert_eq!(
            split_scope_word("global reviewer", DocumentScope::Workspace),
            (DocumentScope::GlobalAgent, "reviewer".to_string())
        );
        assert_eq!(
            split_scope_word("project reviewer", DocumentScope::Workspace),
            (DocumentScope::Workspace, "reviewer".to_string())
        );
    }

    #[test]
    fn a_scope_word_with_no_name_is_just_an_agent_named_that() {
        // Otherwise an agent could not be called `global`.
        assert_eq!(
            split_scope_word("global", DocumentScope::Workspace),
            (DocumentScope::Workspace, "global".to_string())
        );
    }

    #[test]
    fn a_new_name_opens_a_template_at_the_requested_scope() {
        let rows = vec![default_row("build")];
        match resolve_new_agent(Some("reviewer".into()), DocumentScope::Workspace, &rows) {
            NewAgentOutcome::Created(target) => {
                assert_eq!(target.scope, DocumentScope::Workspace);
                assert_eq!(target.path, ".codegg/agents/reviewer.toml");
            }
            other => panic!("expected Created, got {other:?}"),
        }
        match resolve_new_agent(
            Some("global reviewer".into()),
            DocumentScope::Workspace,
            &rows,
        ) {
            NewAgentOutcome::Created(target) => {
                assert_eq!(target.scope, DocumentScope::GlobalAgent);
                assert_eq!(target.path, "reviewer.toml");
            }
            other => panic!("expected Created, got {other:?}"),
        }
    }

    #[test]
    fn an_existing_name_opens_the_file_that_actually_defines_it() {
        // `:new global build` when `build` is a *project* agent must not
        // create a second, shadowed global definition — it should reach the
        // file that is really in effect.
        let rows = vec![project_row("build")];
        match resolve_new_agent(Some("global build".into()), DocumentScope::Workspace, &rows) {
            NewAgentOutcome::Existing(target) => {
                assert_eq!(target.scope, DocumentScope::Workspace);
                assert_eq!(target.path, ".codegg/agents/build.toml");
            }
            other => panic!("expected Existing, got {other:?}"),
        }
        // And a global definition stays reachable at its own scope.
        let rows = vec![global_row("reviewer")];
        match resolve_new_agent(
            Some("global reviewer".into()),
            DocumentScope::Workspace,
            &rows,
        ) {
            NewAgentOutcome::Existing(target) => {
                assert_eq!(target.scope, DocumentScope::GlobalAgent);
                assert_eq!(target.path, "reviewer.toml");
            }
            other => panic!("expected Existing, got {other:?}"),
        }
    }

    #[test]
    fn a_refused_name_explains_itself() {
        let rows = Vec::new();
        assert!(matches!(
            resolve_new_agent(None, DocumentScope::Workspace, &rows),
            NewAgentOutcome::Refused(_)
        ));
        assert!(matches!(
            resolve_new_agent(Some("bad/name".into()), DocumentScope::Workspace, &rows),
            NewAgentOutcome::Refused(_)
        ));
    }

    #[test]
    fn the_template_is_already_a_loadable_agent_at_either_scope() {
        // `:wq` on the template must produce a file the registry accepts,
        // so the required fields are present without further editing.
        for scope in [DocumentScope::Workspace, DocumentScope::GlobalAgent] {
            let template = default_agent_template("reviewer", scope);
            assert!(template.contains("name = \"reviewer\""));
            assert!(template.contains("description ="));
        }
        // The global template has to say what it is, because the path does not.
        assert!(
            default_agent_template("reviewer", DocumentScope::GlobalAgent)
                .contains("EVERY project")
        );
        assert!(default_agent_template("reviewer", DocumentScope::Workspace)
            .contains("this project only"));
    }

    #[test]
    fn the_deactivation_override_names_the_agent_it_suppresses() {
        let override_toml = disabled_override_toml("build", DocumentScope::Workspace);
        assert!(override_toml.contains("name = \"build\""));
        assert!(override_toml.contains("disable = true"));
        assert!(disabled_override_toml("build", DocumentScope::GlobalAgent)
            .contains("global agent directory"));
    }

    #[test]
    fn an_agent_config_is_recognised_only_at_its_own_scope() {
        assert!(is_agent_config_path(
            ".codegg/agents/build.toml",
            DocumentScope::Workspace
        ));
        // The same filename at workspace scope is NOT a global agent config,
        // and vice versa. Matching on the path alone would refresh the wrong
        // registry after a save.
        assert!(!is_agent_config_path(
            "build.toml",
            DocumentScope::Workspace
        ));
        assert!(is_agent_config_path(
            "build.toml",
            DocumentScope::GlobalAgent
        ));
        assert!(!is_agent_config_path(
            ".codegg/agents/build.toml",
            DocumentScope::GlobalAgent
        ));
        assert!(!is_agent_config_path(
            "src/main.rs",
            DocumentScope::Workspace
        ));
        assert!(!is_agent_config_path(
            ".codegg/agents/",
            DocumentScope::Workspace
        ));
        assert!(!is_agent_config_path(
            ".codegg/agents/build.md",
            DocumentScope::Workspace
        ));
        // A nested global path is refused by the same rule the daemon applies.
        assert!(!is_agent_config_path(
            "nested/build.toml",
            DocumentScope::GlobalAgent
        ));
    }
}

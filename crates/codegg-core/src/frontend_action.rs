//! M007-D: the declared frontend-action set.
//!
//! ADR-0052 (accepted 2026-10-24) gives the frontend one bounded set of
//! actions it may dispatch locally. Every **frontend-effect** form in the
//! daemon's catalog resolves to exactly one declared action here, and every
//! declared action resolves to at least one frontend-effect form.
//!
//! A frontend that dispatches an action outside this set is a defect:
//! `scripts/check_frontend_action_set.py` proves the set is closed and that
//! no declared action reaches daemon-owned state.
//!
//! The set is plain data with no frontend type in it, so `codegg-core`
//! stays UI-free.

use serde::{Deserialize, Serialize};

use crate::command_catalog::{the_catalog, CommandKind};

/// What a declared frontend action is allowed to do.
///
/// Every action in this set is **non-mutating** with respect to
/// daemon-owned state. The one action that launches something is marked
/// [`FrontendActionDisposition::ProcessLaunching`], and it launches a
/// frontend-local process (the user's own editor), not a CodeGG-managed
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendActionDisposition {
    /// Changes only what the frontend is showing.
    NonMutating,
    /// Launches a frontend-local process on the user's own behalf.
    ProcessLaunching,
}

/// One frontend action a frontend may dispatch.
///
/// The variant is the shared identity; a frontend may present it under its
/// own label, but the action itself is the same everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendAction {
    OpenQueueBox,
    Quit,
    OpenHelpDialog,
    OpenFileTreeDialog,
    OpenAgentsDialog,
    OpenConnectDialog,
    OpenConnectionSelectionDialog,
    OpenContextDialog,
    OpenCostDialog,
    OpenUsageDialog,
    SetTheme,
    ToggleFullscreen,
    OpenStatsDialog,
    ToggleTts,
    OpenPanelManager,
    FocusChat,
    OpenSessionDialog,
    OpenProjectPicker,
    ShowAdvisory,
    ShowTimeline,
    UndoLocalMessages,
    RedoLocalMessages,
    ToggleTimestamps,
    ToggleThinking,
    ExpandAllDisclosures,
    OpenExternalEditor,
    OpenWorkspaceDashboard,
    ShowToolBackends,
    FocusTerminal,
}

impl FrontendAction {
    /// Stable wire name for the action.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenQueueBox => "open_queue_box",
            Self::Quit => "quit",
            Self::OpenHelpDialog => "open_help_dialog",
            Self::OpenFileTreeDialog => "open_file_tree_dialog",
            Self::OpenAgentsDialog => "open_agents_dialog",
            Self::OpenConnectDialog => "open_connect_dialog",
            Self::OpenConnectionSelectionDialog => "open_connection_selection_dialog",
            Self::OpenContextDialog => "open_context_dialog",
            Self::OpenCostDialog => "open_cost_dialog",
            Self::OpenUsageDialog => "open_usage_dialog",
            Self::SetTheme => "set_theme",
            Self::ToggleFullscreen => "toggle_fullscreen",
            Self::OpenStatsDialog => "open_stats_dialog",
            Self::ToggleTts => "toggle_tts",
            Self::OpenPanelManager => "open_panel_manager",
            Self::FocusChat => "focus_chat",
            Self::OpenSessionDialog => "open_session_dialog",
            Self::OpenProjectPicker => "open_project_picker",
            Self::ShowAdvisory => "show_advisory",
            Self::ShowTimeline => "show_timeline",
            Self::UndoLocalMessages => "undo_local_messages",
            Self::RedoLocalMessages => "redo_local_messages",
            Self::ToggleTimestamps => "toggle_timestamps",
            Self::ToggleThinking => "toggle_thinking",
            Self::ExpandAllDisclosures => "expand_all_disclosures",
            Self::OpenExternalEditor => "open_external_editor",
            Self::OpenWorkspaceDashboard => "open_workspace_dashboard",
            Self::ShowToolBackends => "show_tool_backends",
            Self::FocusTerminal => "focus_terminal",
        }
    }

    /// Every action in the declared set, in declaration order.
    pub const ALL: &'static [FrontendAction] = &[
        FrontendAction::OpenQueueBox,
        FrontendAction::Quit,
        FrontendAction::OpenHelpDialog,
        FrontendAction::OpenFileTreeDialog,
        FrontendAction::OpenAgentsDialog,
        FrontendAction::OpenConnectDialog,
        FrontendAction::OpenConnectionSelectionDialog,
        FrontendAction::OpenContextDialog,
        FrontendAction::OpenCostDialog,
        FrontendAction::OpenUsageDialog,
        FrontendAction::SetTheme,
        FrontendAction::ToggleFullscreen,
        FrontendAction::OpenStatsDialog,
        FrontendAction::ToggleTts,
        FrontendAction::OpenPanelManager,
        FrontendAction::FocusChat,
        FrontendAction::OpenSessionDialog,
        FrontendAction::OpenProjectPicker,
        FrontendAction::ShowAdvisory,
        FrontendAction::ShowTimeline,
        FrontendAction::UndoLocalMessages,
        FrontendAction::RedoLocalMessages,
        FrontendAction::ToggleTimestamps,
        FrontendAction::ToggleThinking,
        FrontendAction::ExpandAllDisclosures,
        FrontendAction::OpenExternalEditor,
        FrontendAction::OpenWorkspaceDashboard,
        FrontendAction::ShowToolBackends,
        FrontendAction::FocusTerminal,
    ];
}

/// One declared action: what it does and which catalog forms it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontendActionDeclaration {
    /// The action.
    pub action: FrontendAction,
    /// What the action is allowed to do.
    pub disposition: FrontendActionDisposition,
    /// The catalog forms this action serves, as
    /// `(command_id, form)` pairs.
    pub forms: &'static [(&'static str, &'static str)],
}

/// The declared frontend-action set.
static THE_FRONTEND_ACTION_SET: &[FrontendActionDeclaration] = &[
    FrontendActionDeclaration {
        action: FrontendAction::OpenQueueBox,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("queue", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::Quit,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("exit", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenHelpDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("help", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenFileTreeDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("tree", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenAgentsDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("agents", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenConnectDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("connect", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenConnectionSelectionDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("connections", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenContextDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("context", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenCostDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("cost", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenUsageDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("usage", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::SetTheme,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("themes", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ToggleFullscreen,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("tui", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenStatsDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("tui_stats", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ToggleTts,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("tts", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenPanelManager,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("panels", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::FocusChat,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("chat", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenSessionDialog,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("sessions", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenProjectPicker,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("projects", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ShowAdvisory,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[
            ("rename", ""),
            ("variants", ""),
            ("fork", ""),
            ("workspaces", ""),
        ],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ShowTimeline,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("timeline", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::UndoLocalMessages,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("undo", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::RedoLocalMessages,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("redo", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ToggleTimestamps,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("timestamps", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ToggleThinking,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("thinking", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ExpandAllDisclosures,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("expand_all_disclosures", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenExternalEditor,
        disposition: FrontendActionDisposition::ProcessLaunching,
        forms: &[("editor", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::OpenWorkspaceDashboard,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("workspace", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::ShowToolBackends,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("tool_backends", "")],
    },
    FrontendActionDeclaration {
        action: FrontendAction::FocusTerminal,
        disposition: FrontendActionDisposition::NonMutating,
        forms: &[("terminal_focus", "")],
    },
];

/// The declared frontend-action set.
pub fn the_frontend_action_set() -> &'static [FrontendActionDeclaration] {
    THE_FRONTEND_ACTION_SET
}

/// The declaration for one action.
pub fn find_declaration(action: FrontendAction) -> Option<&'static FrontendActionDeclaration> {
    THE_FRONTEND_ACTION_SET
        .iter()
        .find(|declaration| declaration.action == action)
}

/// The action that serves one catalog form, if the form is a frontend
/// effect and the set declares it.
pub fn action_for_form(command_id: &str, form: &str) -> Option<FrontendAction> {
    THE_FRONTEND_ACTION_SET
        .iter()
        .find(|declaration| {
            declaration
                .forms
                .iter()
                .any(|(id, f)| *id == command_id && *f == form)
        })
        .map(|declaration| declaration.action)
}

/// Every frontend-effect form in the catalog that the declared set does
/// not cover.
///
/// This must be empty: a frontend-effect form with no declared action
/// cannot be dispatched by any frontend, and a declared action with no
/// catalog form is dead weight.
pub fn undeclared_frontend_forms() -> Vec<(&'static str, &'static str)> {
    the_catalog()
        .commands
        .iter()
        .flat_map(|command| {
            command
                .forms
                .iter()
                .filter(|form| form.kind == CommandKind::FrontendEffect)
                .map(move |form| (command.command_id, form.form))
        })
        .filter(|(command_id, form)| action_for_form(command_id, form).is_none())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every frontend-effect form in the catalog is covered by exactly one
    /// declared action, and every declared action covers at least one form.
    ///
    /// This is the closure property: a frontend-effect form with no action
    /// could not be dispatched by anybody, and an action with no form would
    /// be dead weight.
    #[test]
    fn the_declared_set_is_closed_over_the_catalog() {
        assert!(
            undeclared_frontend_forms().is_empty(),
            "every frontend-effect form must be declared"
        );
        for declaration in the_frontend_action_set() {
            assert!(
                !declaration.forms.is_empty(),
                "{} must serve at least one catalog form",
                declaration.action.as_str()
            );
            for (command_id, form) in declaration.forms {
                let command = crate::command_catalog::find_by_command_id(command_id)
                    .unwrap_or_else(|| panic!("{command_id} must be in the catalog"));
                let catalog_form = command
                    .forms
                    .iter()
                    .find(|catalog_form| catalog_form.form == *form)
                    .unwrap_or_else(|| panic!("{command_id}/{form} must be in the catalog"));
                assert_eq!(
                    catalog_form.kind,
                    crate::command_catalog::CommandKind::FrontendEffect,
                    "{command_id}/{form} is a daemon effect and must not be served locally"
                );
            }
        }
    }

    /// No declared action mutates daemon-owned state. The only dispositions
    /// in the set are the non-mutating ones.
    #[test]
    fn no_declared_action_is_mutating() {
        for declaration in the_frontend_action_set() {
            assert!(
                matches!(
                    declaration.disposition,
                    FrontendActionDisposition::NonMutating
                        | FrontendActionDisposition::ProcessLaunching
                ),
                "{} must carry a non-mutating disposition",
                declaration.action.as_str()
            );
        }
    }

    /// The set is finite and every declaration names an action the enum has.
    #[test]
    fn the_set_is_finite_and_declared() {
        let declared: Vec<FrontendAction> = the_frontend_action_set()
            .iter()
            .map(|declaration| declaration.action)
            .collect();
        assert_eq!(
            declared.len(),
            FrontendAction::ALL.len(),
            "every enum variant must be declared exactly once"
        );
        for action in FrontendAction::ALL {
            assert!(
                declared.contains(action),
                "{} must be declared",
                action.as_str()
            );
        }
    }
}

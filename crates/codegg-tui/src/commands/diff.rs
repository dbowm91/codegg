//! The `/diff` command handler (M007-I, work package A).
//!
//! The diff view is a **daemon** effect: the daemon resolves the checkout
//! from its own durable binding and reads the bytes through the read-only
//! git fact layer. This module never spawns `git` and never reads the
//! working tree itself; it sends the command and renders the daemon's
//! answer.
//!
//! The scope comes from [`crate::command_route::scope_for`], so the
//! daemon learns the project and the workspace from the attached session's
//! canonical binding and never from the frontend's guess.

use crate::app::App;
use crate::app::TuiCommand;
use crate::async_cmd::spawn_registered_tui_task;
use crate::task_lifecycle::TuiTaskKind;
use codegg::protocol::core::CoreResponse;

/// Run `/diff <path>` — or the bare `/diff` — against the daemon.
///
/// The empty path is the **bare** branch: the whole working tree against
/// `HEAD`. The two branches are mutually exclusive, exactly as they were
/// when the frontend owned the read.
pub(crate) fn start_diff(app: &mut App, path: String) {
    let core_client = app.core_client.clone();
    let scope = crate::command_route::scope_for(app);
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "diff",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::DiffFinished {
                    response: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let response =
                crate::command_route::send_command(&core_client, scope, "diff", "", &path).await;
            match response {
                Ok(response) => Some(TuiCommand::DiffFinished {
                    response: Some(response),
                    error: None,
                }),
                Err(error) => Some(TuiCommand::DiffFinished {
                    response: None,
                    error: Some(format!("Failed to read the diff: {}", error)),
                }),
            }
        },
    );
}

/// Render the daemon's answer.
///
/// The daemon's typed refusal is rendered as it arrives. The TUI never
/// substitutes a local approximation of the read it could not perform, and
/// it keeps no fallback that would report a stale diff.
pub(crate) fn apply_diff_finished(
    app: &mut App,
    response: Option<CoreResponse>,
    error: Option<String>,
) {
    use crate::panels::hosted::PlaceholderState;
    use crate::panels::SidePanelId;

    // The panel that started this read is *waiting* for it: its placeholder
    // has said "Loading" since the user selected it. Every completion ends
    // that state and, while the panel is empty, paints the answer there —
    // the modal is for a diff the user asked for from the composer, not for
    // the column they are already looking at.
    let panel_waiting = app.panel_host.panel_is_loading(SidePanelId::Diff);
    let settle = |app: &mut App, state: PlaceholderState| {
        if panel_waiting {
            app.panel_host
                .set_placeholder_state(SidePanelId::Diff, state);
        }
    };

    if let Some(error) = error {
        settle(app, PlaceholderState::Failed(error.clone()));
        app.messages_state.toasts.error(&error);
        return;
    }
    let Some(response) = response else {
        settle(
            app,
            PlaceholderState::Failed("the diff request returned no answer".to_string()),
        );
        return;
    };
    match response {
        CoreResponse::Error { code, message } => {
            let refusal = format!("{code}: {message}");
            settle(app, PlaceholderState::Failed(refusal.clone()));
            app.messages_state.toasts.error(&refusal);
        }
        CoreResponse::GitDiff { result } => {
            if result.old_content == result.new_content {
                // An empty answer is an answer. The panel says so in place
                // instead of holding a spinner that will never resolve.
                settle(
                    app,
                    PlaceholderState::Empty("No changes: the working tree matches HEAD."),
                );
                app.messages_state.toasts.info("No changes detected");
                return;
            }
            let dialog = crate::components::dialogs::diff::DiffDialog::new(
                result.old_content.into_boxed_str(),
                result.new_content.into_boxed_str(),
                result.title.into_boxed_str(),
            );
            if panel_waiting {
                app.panel_host.adopt(SidePanelId::Diff, Box::new(dialog));
                return;
            }
            app.dialog_state.diff_dialog = Some(dialog);
            app.open_dialog(crate::Dialog::Diff);
        }
        _ => {
            settle(
                app,
                PlaceholderState::Failed("the diff returned an unexpected response".to_string()),
            );
            app.messages_state
                .toasts
                .warning("The diff returned an unexpected response");
        }
    }
}

pub(crate) fn dispatch_diff(app: &mut App, argument: &str) {
    let path_arg = argument;
    app.ui_state.command_mode = false;
    // M007-I (work package A): the daemon owns the read. The TUI
    // no longer spawns `git` and no longer reads the working
    // tree for this.
    crate::commands::diff::start_diff(app, path_arg.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::hosted::PlaceholderState;
    use crate::panels::SidePanelId;
    use codegg::protocol::git::GitDiffResultDto;

    fn result(old: &str, new: &str) -> GitDiffResultDto {
        GitDiffResultDto {
            project_id: "p-1".to_string(),
            workspace_id: "w-1".to_string(),
            path: None,
            old_content: old.to_string(),
            new_content: new.to_string(),
            title: "diff: working tree".to_string(),
        }
    }

    /// A panel the user just selected: it has been saying Loading since the
    /// manager started the read.
    fn waiting_panel() -> App {
        let mut app = App::new_for_testing("diff-test".to_string());
        app.panel_host
            .set_placeholder_state(SidePanelId::Diff, PlaceholderState::Loading);
        assert!(app.panel_host.panel_is_loading(SidePanelId::Diff));
        app
    }

    /// The reported defect: the panel never displays anything. The daemon's
    /// answer must land in the column the user selected, and the placeholder
    /// must stop saying a load is in flight.
    #[test]
    fn a_waiting_diff_panel_adopts_the_daemon_answer_instead_of_a_modal() {
        let mut app = waiting_panel();
        apply_diff_finished(
            &mut app,
            Some(CoreResponse::GitDiff {
                result: result("", "+one\n+two\n"),
            }),
            None,
        );

        assert!(
            !app.panel_host.panel_is_loading(SidePanelId::Diff),
            "the load ended when the answer arrived"
        );
        assert_eq!(
            app.panel_host.hosted_panel_has_content(SidePanelId::Diff),
            Some(true),
            "the selected panel must hold the diff it asked for"
        );
        assert_eq!(
            app.ui_state.dialog,
            crate::Dialog::None,
            "the box must not cover the column the user is looking at"
        );
        let painted = app
            .panel_host
            .with_hosted_mut::<crate::components::dialogs::diff::DiffDialog, _>(
                SidePanelId::Diff,
                |dialog| dialog.viewer.new_content.clone(),
            )
            .expect("the panel holds the diff viewer");
        assert_eq!(
            painted, "+one\n+two\n",
            "the panel paints the daemon's bytes, not an approximation"
        );
    }

    /// A clean tree is an answer, not a stuck spinner: the panel says so in
    /// place rather than waiting for a reply that already arrived.
    #[test]
    fn a_clean_tree_ends_the_panel_load_with_an_explicit_empty_state() {
        let mut app = waiting_panel();
        apply_diff_finished(
            &mut app,
            Some(CoreResponse::GitDiff {
                result: result("same\n", "same\n"),
            }),
            None,
        );

        assert!(!app.panel_host.panel_is_loading(SidePanelId::Diff));
        assert_eq!(
            app.panel_host.placeholder_state(SidePanelId::Diff),
            Some(&PlaceholderState::Empty(
                "No changes: the working tree matches HEAD."
            )),
            "an empty answer must be visible in the panel, not only in a toast"
        );
        assert_eq!(app.ui_state.dialog, crate::Dialog::None);
    }

    /// The daemon's typed refusal belongs where the load was started. The
    /// panel used to keep its spinner and report the refusal in a toast that
    /// outlived neither the tick nor the reader's attention.
    #[test]
    fn a_daemon_refusal_is_painted_in_the_panel() {
        let mut app = waiting_panel();
        apply_diff_finished(
            &mut app,
            Some(CoreResponse::Error {
                code: "git_unavailable".to_string(),
                message: "no checkout is bound to this project".to_string(),
            }),
            None,
        );

        assert!(!app.panel_host.panel_is_loading(SidePanelId::Diff));
        assert_eq!(
            app.panel_host.placeholder_state(SidePanelId::Diff),
            Some(&PlaceholderState::Failed(
                "git_unavailable: no checkout is bound to this project".to_string()
            )),
            "the refusal must name itself in the panel"
        );
        assert_eq!(app.ui_state.dialog, crate::Dialog::None);
    }

    #[test]
    fn a_transport_error_settles_the_panel_load() {
        let mut app = waiting_panel();
        apply_diff_finished(
            &mut app,
            None,
            Some("Failed to read the diff: closed".to_string()),
        );

        assert!(!app.panel_host.panel_is_loading(SidePanelId::Diff));
        assert_eq!(
            app.panel_host.placeholder_state(SidePanelId::Diff),
            Some(&PlaceholderState::Failed(
                "Failed to read the diff: closed".to_string()
            ))
        );
    }

    /// A diff typed from the composer keeps its box: no panel was waiting,
    /// so there is no column to fill.
    #[test]
    fn a_diff_asked_for_from_the_composer_still_opens_its_box() {
        let mut app = App::new_for_testing("diff-test".to_string());
        apply_diff_finished(
            &mut app,
            Some(CoreResponse::GitDiff {
                result: result("a\n", "b\n"),
            }),
            None,
        );

        assert_eq!(app.ui_state.dialog, crate::Dialog::Diff);
        assert!(
            app.dialog_state.diff_dialog.is_some(),
            "the box needs the viewer the completion just built"
        );
    }
}

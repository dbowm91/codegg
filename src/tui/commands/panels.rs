//! Side-panel switching and key routing.
//!
//! This is the only place that decides *which* project a panel is bound to
//! and *when* a panel may take focus. Panels themselves never resolve a
//! project: `PanelCtx::chat_project` is handed to them, so a panel cannot
//! become an authority for identity the way an ambient `project_dir` read
//! would.
//!
//! Project resolution order for the chat panel is deliberate:
//!
//! 1. the Workspace dashboard's selected row, when that view is open —
//!    this preserves the existing M005 behaviour, where the side column
//!    always shows the project the user highlighted;
//! 2. otherwise the active tab's explicit project id.
//!
//! The active tab's execution context is the authoritative locator (see
//! `architecture/tui.md`); it is never inferred from the process cwd.

use crossterm::event::KeyEvent;
use std::sync::Arc;

use crate::tui::app::state::chat::ChatState;
use crate::tui::app::App;
use crate::tui::input::{InputAction, InputMode};
use crate::tui::panels::hosted::PlaceholderState;
use crate::tui::panels::{
    PanelCtx, PanelDirection, PanelFill, PanelKey, PanelPlacement, SidePanelId,
};
use crate::tui::theme::Theme;

/// Resolve the project the chat panel should bind to right now.
fn resolve_chat_project(app: &App) -> Option<String> {
    if let Some(selected) =
        crate::tui::commands::workspace_dashboard::workspace_selected_project_id(app)
    {
        return Some(selected);
    }
    app.active_project_id().map(str::to_string)
}

/// Re-bind the chat panel to the project the panel should show, and keep
/// `App::chat_panel_project` in lockstep so the existing chat command seams
/// and `/chat` modal agree about which project is on screen.
///
/// Called on every panel selection and on Workspace selection changes. When
/// the binding changes, the panel's per-view state is dropped: a composer
/// buffer must never survive into a different project's channel list.
pub(crate) fn sync_chat_panel_project(app: &mut App) {
    let resolved = resolve_chat_project(app);
    let changed = app.chat_panel_project.as_deref() != resolved.as_deref();
    app.chat_panel_project = resolved;
    let panel = app.panel_host.chat_panel_mut();
    if changed {
        panel.reset();
    }
    panel.set_bound(app.chat_panel_project.is_some());
}

/// Open the panel manager (`Shift+S`, Normal mode only).
///
/// A *box*, not an overlay drawn inside the column. The old grid selector
/// showed two unlabelled buttons in 30 columns and could not be filtered;
/// the manager names each panel, says what it is for, shows which digit is
/// already bound, and offers the same type-to-filter the rest of the TUI
/// uses. It also has somewhere to explain itself, which is what a switcher
/// for a growing registry needs.
pub(crate) fn open_panel_manager(app: &mut App) {
    // Opening the manager must not change the frame. It used to force the
    // selected edge visible, on the reasoning that a strip describing a
    // collapsed column is confusing — but the cost was that a panel the user
    // had deliberately put away with `Ctrl+B` came back by itself the moment
    // they reached for `Shift+S`, which reads as the TUI undoing their last
    // action. The strip now *names* a collapsed edge instead of resolving the
    // ambiguity by moving it.
    //
    // Resolve the chat binding before the user can pick Chat: the host has no
    // app access and would otherwise find it unbound and refuse focus.
    sync_chat_panel_project(app);
    let dialog = crate::tui::components::dialogs::panel_manager::PanelManagerDialog::new(
        Arc::clone(&app.ui_state.theme),
        app.panel_hotkeys.clone(),
        app.panel_host.active(),
        app.panel_host.placement(),
        app.panel_host.is_visible_at(app.panel_host.placement()),
    );
    app.push_dialog(crate::tui::app::Dialog::PanelManager, Box::new(dialog));
}

/// Open the "which panel should this go in?" box.
///
/// Shared by every `/` command whose content is a panel rather than a modal:
/// the box answers exactly one question and commits through the same
/// [`crate::tui::app::TuiMsg::SelectPanel`] the manager emits, so there is only
/// one route into panel selection and a box and its panel cannot drift.
///
/// `subject` names what is being routed so the box reads as itself rather than
/// as a copy of the manager.
///
/// The pending-routing flag is what makes the selection *mean* something: the
/// manager selects a panel and stops, while this box exists to have the chosen
/// content loaded into the chosen panel afterwards.
pub(crate) fn open_panel_picker(app: &mut App, subject: &'static str) {
    sync_chat_panel_project(app);
    let dialog = crate::tui::components::dialogs::panel_picker::PanelPickerDialog::new(
        Arc::clone(&app.ui_state.theme),
        app.panel_host.active(),
        subject,
    );
    app.pending_panel_picker_subject = Some(subject);
    app.push_dialog(crate::tui::app::Dialog::PanelPicker, Box::new(dialog));
}

/// Push an applied placement back into the mounted manager.
///
/// `TuiMsg` handling runs outside the mounted component, so the box cannot
/// observe the change it just asked for. Without this the layout strip keeps
/// bracketing the edge the user just left — the box would contradict the
/// frame it is describing, which is worse than the strip not existing. Typed
/// by component rather than by `DialogType` because the type is what has to
/// match for the downcast, and the manager is the only `PanelManagerDialog`.
pub(crate) fn sync_manager_placement(app: &mut App) {
    let placement = app.panel_host.placement();
    let visible = app.panel_host.is_visible_at(placement);
    if let Some(dialog) =
        app.focus_manager
            .dialog_mut_any::<crate::tui::components::dialogs::panel_manager::PanelManagerDialog>()
    {
        dialog.set_placement(placement, visible);
    }
}

/// Put the manager's highlighted panel on `placement`.
///
/// The manager's Normal-mode directional. Moving and loading stay separate acts
/// because they answer separate questions: this one says *where the panel
/// lives* and leaves it unloaded, `Enter` says *fill this edge*. Folding them
/// together would mean a gesture that silently starts a daemon load — the same
/// reason walking the panel list never materializes an empty panel.
///
/// The box stays open, like `p`: arranging three panels and arriving at one is
/// one continuous act, and closing would throw away the list mid-arrangement.
pub(crate) fn move_manager_panel(app: &mut App, panel: SidePanelId, placement: PanelPlacement) {
    let from = app.panel_host.placement();
    if from == placement && app.panel_host.panel_at(placement) == panel {
        // Still confirm. A keypress that appears to do nothing is read as a
        // dead key, and the honest "already there" is the useful answer.
        app.messages_state.toasts.info(&format!(
            "{} is already on the {}",
            panel.label(),
            placement.label()
        ));
        return;
    }
    app.panel_host.dock(panel, placement);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
    persist_panel_placement(app, placement);
    app.messages_state.toasts.info(&format!(
        "{} moved to the {}; the {} is now {}",
        panel.label(),
        placement.label(),
        from.label(),
        describe_vacated(app, from)
    ));
    sync_manager_placement(app);
}

/// Step the manager's layout strip, then tell the box what happened.
pub(crate) fn cycle_manager_placement(app: &mut App) {
    cycle_panel_placement(app);
    sync_manager_placement(app);
}

/// Toggle the manager from the action arm.
///
/// Reached when no panel holds focus, so the key never reaches the host's
/// own key handling. When a panel *is* focused the host answers the same
/// action itself, keeping one owner for the toggle.
pub(crate) fn toggle_panel_selector(app: &mut App) {
    open_panel_manager(app);
}

/// Apply a `Ctrl+digit` recall.
///
/// Returns `true` when the key was a slot and has been consumed, whether or
/// not it was bound — an unbound slot is still the manager's digit range and
/// must not fall through into the composer.
pub(crate) fn recall_panel_hotkey(app: &mut App, digit: char) -> bool {
    let Some(panel) = app.panel_hotkeys.get(digit) else {
        app.messages_state.toasts.info(&format!(
            "No panel bound to ^-{digit} — bind it in the panel manager (⇧S)"
        ));
        return true;
    };
    let placement = app.panel_host.placement();
    app.panel_host.set_visible_at(placement, true);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
    if panel == SidePanelId::Chat {
        sync_chat_panel_project(app);
    }
    if select_panel(app, panel) == SelectOutcome::BoxOpen {
        // A live box was opened instead; it owns the keyboard, so there
        // is nothing to focus behind it.
        return true;
    }
    // Focus follows, because the whole point of a saved hotkey is arriving
    // at the panel ready to use it.
    focus_panel(app);
    true
}

/// Persist a new digit binding and confirm it in place.
pub(crate) fn save_panel_hotkey(app: &mut App, digit: char, panel: SidePanelId) {
    let replaced = app.panel_hotkeys.get(digit);
    app.panel_hotkeys.set(digit, panel);
    let notice = match replaced {
        Some(previous) if previous != panel => {
            format!(
                "^-{digit} rebound: was {}, now {}",
                previous.label(),
                panel.label()
            )
        }
        Some(_) => format!("^-{digit} already {} — unchanged", panel.label()),
        None => format!("^-{digit} → {} (Ctrl+{digit})", panel.label()),
    };
    persist_panel_hotkeys(app);
    app.messages_state.toasts.info(&notice);
}

/// Write the hotkey map back to the **global** config.
///
/// Mirrors the theme's persistence path rather than the keybind editor's:
/// a panel hotkey is a per-user preference, so it belongs in the user-level
/// config. `Config::save()` prefers the nearest project config, which would
/// confine the binding to one checkout and could write it into a tracked
/// file — exactly the problem that made the theme use `save_global`.
///
/// Failures are reported but never roll back the in-memory binding: the key
/// works for this session either way, and a user who cannot write their
/// config should still be able to use the panel they just bound.
fn persist_panel_hotkeys(app: &mut App) {
    let mut config = match crate::config::schema::Config::load() {
        Ok(config) => config,
        Err(error) => {
            app.messages_state
                .toasts
                .error(&format!("Hotkey not saved: {error}"));
            return;
        }
    };
    config.panel_hotkeys = Some(app.panel_hotkeys.to_map().into_iter().collect());
    if let Err(error) = config.save_global() {
        app.messages_state
            .toasts
            .error(&format!("Hotkey not saved: {error}"));
    }
}

/// Switch to `id`, binding the chat panel if needed.
///
/// Returns whether the requested panel is loaded. An empty hosted panel is
/// materialized on selection (see [`materialize_hosted_panel`]), but a box
/// that loads asynchronously stays open as a modal instead — callers must
/// not take panel focus behind it.
pub(crate) fn select_panel(app: &mut App, id: SidePanelId) -> SelectOutcome {
    if id == SidePanelId::Chat {
        sync_chat_panel_project(app);
    }
    if !materialize_hosted_panel(app, id) {
        return SelectOutcome::BoxOpen;
    }
    {
        let App {
            panel_host,
            sidebar,
            chat,
            interactive_terminals,
            chat_panel_project,
            sidebar_area,
            ui_state,
            ..
        } = app;
        let focused = panel_host.is_focused();
        let mut ctx = panel_ctx(
            ui_state,
            sidebar,
            chat,
            interactive_terminals,
            chat_panel_project,
            *sidebar_area,
            focused,
            ui_state.input_mode,
        );
        panel_host.select(id, &mut ctx);
    }
    sync_workspace_focus(app);
    SelectOutcome::Selected
}

/// Whether [`select_panel`] loaded the requested panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectOutcome {
    /// The requested panel (or its placeholder) is loaded. Focus it.
    Selected,
    /// A live box was opened instead and owns the keyboard. Leave the
    /// current panel alone: focusing it would claim the badge and hint
    /// line while every key goes to the modal.
    BoxOpen,
}

/// Fill an empty hosted panel when it is selected, so `Shift+S` + `Enter`
/// is one step rather than "run the `/` command, then dock the box".
///
/// Only the command layer may do this: opening a box resolves projects,
/// reads snapshots and starts daemon work, all of which panels are barred
/// from. A panel with content already (or a bespoke panel) is untouched.
///
/// Returns `true` when the caller should select `id`. That is true for
/// everything except the two outcomes where a live box owns the keyboard —
/// [`PanelFill::StillLoading`] and [`PanelFill::OpenedAs`] — because focusing
/// the panel behind an open modal would claim the badge while every key goes
/// to the modal.
///
/// Whatever happened is recorded on the panel itself, so the column shows the
/// truth instead of a standing "nothing is loaded".
fn materialize_hosted_panel(app: &mut App, id: SidePanelId) -> bool {
    if app.panel_host.hosted_panel_has_content(id) != Some(false) {
        return true;
    }
    // A box is already open (the manager itself maps to no dock target, so
    // this is a real one). Leave it alone: adopting it here would steal a
    // box the user opened for another purpose.
    if app.dock_target().is_some() {
        return true;
    }
    match open_panel_box(app, id) {
        PanelFill::Filled => {
            if let Some(widget) = app.close_dialog() {
                app.panel_host.adopt(id, widget);
            }
            true
        }
        PanelFill::Loading => {
            app.panel_host
                .set_placeholder_state(id, PlaceholderState::Loading);
            true
        }
        PanelFill::Cannot(reason) => {
            app.panel_host
                .set_placeholder_state(id, PlaceholderState::Cannot(reason));
            true
        }
        // The box is live and incomplete: adopting it would freeze it on its
        // spinner and cancel the load behind it, and it owns the keyboard.
        // The panel is still recorded as loading, so the moment the user looks
        // at *this* column instead of the box it says why it is empty rather
        // than claiming nothing was ever asked for.
        PanelFill::StillLoading => {
            app.panel_host
                .set_placeholder_state(id, PlaceholderState::Loading);
            app.messages_state
                .toasts
                .info(&format!("{} opened — press L to dock it", id.label()));
            false
        }
        // A box opened, just not for this panel. It keeps the keyboard, and the
        // panel it actually belongs to is the one that can adopt it.
        PanelFill::OpenedAs(other) => {
            app.messages_state
                .toasts
                .info(&format!("Opened as {} — press L to dock it", other.label()));
            false
        }
        // Not hosted, so no placeholder exists and nothing was attempted.
        PanelFill::Bespoke => true,
    }
}

/// Open the box that fills `id`, using the same seams as its `/` command, and
/// report what that actually did.
///
/// Synchronous snapshot boxes (context, logs, models, …) mount immediately and
/// are ready to adopt. Asynchronous ones (goal, doctor, plugins, …) only start
/// their load; their box opens later as a modal through the existing
/// completion path, where `L` docks it as before. Two arms mount a box *and*
/// start a load behind it (research runs, the Task view), so the box is live
/// but incomplete and must not be adopted. Panels that cannot act at all (a
/// plan panel with no pending confirmation, a security panel with no receipt,
/// a memory panel with no query) toast why and say so in the panel.
///
/// **Every arm returns its own outcome.** That is the contract: a caller must
/// never have to re-read `dock_target()` and guess. Two arms deliberately
/// pre-check a condition instead of trusting the helper to report it, and each
/// says which condition it mirrors — see `Tasks`, `Team`, and `Collaborators`.
///
/// Also the entry point for a bare `/agents`: the panel is loaded and focused
/// from the same `P::Agents` arm either way, so the slash command and
/// `Shift+L` cannot disagree about what the panel shows.
pub(crate) fn open_panel_box(app: &mut App, id: SidePanelId) -> PanelFill {
    use crate::tui::app::Dialog;
    use SidePanelId as P;
    match id {
        // Bespoke panels are their own live surface, so there is nothing to
        // open and nothing to adopt. `materialize_hosted_panel` returns before
        // reaching these for a panel that is not hosted; the picker reaches
        // `Agents` directly, which is why that one still does its work.
        P::Session | P::Chat | P::Terminal | P::Help => PanelFill::Bespoke,
        P::Tree => {
            app.open_dialog(Dialog::Tree);
            PanelFill::Filled
        }
        P::Sessions => {
            app.open_dialog(Dialog::Session);
            PanelFill::Filled
        }
        // `start_tasks_command` either starts the async schedule list, whose
        // box arrives from a completion, or lands on the project Task view —
        // a box that belongs to `TaskView` and is still loading. Both are
        // declared here rather than inferred from whichever box appeared.
        P::Tasks => crate::tui::commands::work_orders::start_tasks_command(app),
        P::Plan => {
            if app.dialog_state.plan_dialog.is_some() {
                app.open_dialog(Dialog::Plan);
                PanelFill::Filled
            } else {
                app.messages_state
                    .toasts
                    .info("No pending plan confirmation — /plan shows the current task plan");
                PanelFill::Cannot("no pending plan confirmation")
            }
        }
        P::Goal => match app.session_state.session.clone() {
            Some(session) => {
                crate::tui::commands::goals::start_goal_show(app, session.id);
                app.messages_state.toasts.info("Loading goal…");
                PanelFill::Loading
            }
            None => {
                app.messages_state.toasts.warning("No active session");
                PanelFill::Cannot("no active session")
            }
        },
        // Mirrors `show_collaborators`: it opens its box synchronously from
        // already-resolved presence state, but refuses without a project.
        P::Collaborators => {
            if app.active_project_id().is_none() {
                app.messages_state
                    .toasts
                    .info("No active project — open a project tab first");
                PanelFill::Cannot("no active project")
            } else {
                crate::tui::commands::presence::show_collaborators(app);
                PanelFill::Filled
            }
        }
        P::Memory => {
            app.messages_state
                .toasts
                .info("Memory needs a query — run /memory-search <query>");
            PanelFill::Cannot("memory search needs a query")
        }
        // The browser mounts with `loading = true` and fills from the reply,
        // so the box is real and incomplete: adopting it here would pin the
        // panel to its spinner.
        P::Research => {
            app.handle_research_runs_command();
            PanelFill::StillLoading
        }
        P::Logs => {
            app.open_logs_window();
            PanelFill::Filled
        }
        P::Context => {
            app.open_dialog(Dialog::Context);
            PanelFill::Filled
        }
        P::Cost => {
            app.open_dialog(Dialog::Cost);
            PanelFill::Filled
        }
        P::Usage => {
            app.open_dialog(Dialog::Usage);
            PanelFill::Filled
        }
        P::Stats => {
            app.open_stats_dialog();
            PanelFill::Filled
        }
        P::Doctor => {
            if let Some(ref tx) = app.tui_cmd_tx {
                let _ = crate::tui::app::send_tui(tx, crate::tui::app::TuiCommand::RunDoctor);
                app.messages_state.toasts.info("Running doctor…");
                PanelFill::Loading
            } else {
                app.messages_state
                    .toasts
                    .info("doctor: not connected to a core client");
                PanelFill::Cannot("not connected to a core client")
            }
        }
        P::ShellShow => {
            if app.shell_store.list_recent(10).is_empty() {
                app.messages_state
                    .toasts
                    .info("No shell commands in history");
                PanelFill::Cannot("no shell commands in history")
            } else {
                app.messages_state
                    .toasts
                    .info("Run /shell-show <id|last> to open one, then L to dock it");
                PanelFill::Cannot("no shell selected")
            }
        }
        P::Security => {
            if app.latest_security_review.is_some() {
                app.open_dialog(Dialog::SecurityReview);
                PanelFill::Filled
            } else {
                app.messages_state.toasts.warning(
                    "No security review result available yet. Run /security-review first.",
                );
                PanelFill::Cannot("no security review result yet")
            }
        }
        P::Models => {
            app.open_dialog(Dialog::Model);
            PanelFill::Filled
        }
        P::Mcp => {
            app.open_dialog(Dialog::Mcp);
            PanelFill::Filled
        }
        P::Keybinds => {
            app.open_dialog(Dialog::Keybind);
            PanelFill::Filled
        }
        P::Themes => {
            app.open_dialog(Dialog::Theme);
            PanelFill::Filled
        }
        // Loads its rows and takes focus itself, so there is no box to adopt —
        // but it is reached from the picker, so the work still happens here.
        // `select_panel` is the one seam that both selects and loads, so the
        // picker and `Shift+L` land on the panel by exactly the same path.
        P::Agents => match app.project_execution_context() {
            Ok(context) => {
                // The rows come from the same registry the text form renders,
                // so the panel and `/agents show` can never disagree about
                // which agents exist. Loading is a synchronous registry read,
                // so the panel is filled before it is shown rather than after
                // a frame with stale rows.
                let rows = crate::tui::commands::agent_editor::agent_rows_for_root(
                    &context.workspace_root,
                );
                app.panel_host.set_agents_rows(rows);
                select_panel(app, SidePanelId::Agents);
                focus_panel(app);
                if !app.panel_host.is_focused() {
                    app.messages_state
                        .toasts
                        .warning("Agents panel could not take focus — press L to select it");
                }
                PanelFill::Bespoke
            }
            Err(error) => {
                app.messages_state.toasts.error(&error);
                PanelFill::Cannot("no active project")
            }
        },
        // Starts an async read whose box arrives from the reply, so the panel
        // shows progress until it does. It used to toast "Loading plugins…"
        // while the column said no Plugins existed anywhere.
        P::Plugins => {
            let has_manager = app.plugin_manager.is_some();
            crate::tui::commands::plugin_management::show_plugins(app);
            if has_manager {
                app.messages_state.toasts.info("Loading plugins…");
                PanelFill::Loading
            } else {
                PanelFill::Cannot("plugin manager unavailable")
            }
        }
        // Two branches behind one arm: without a daemon connection the helper
        // opens a box synchronously saying so, and with one it starts an async
        // read whose box arrives from the reply. Mirrors `start_show_team`.
        P::Team => {
            if app.active_project_id().is_none() {
                app.messages_state
                    .toasts
                    .info("No active project — open a project tab first");
                PanelFill::Cannot("no active project")
            } else {
                // Read the branch before dispatching, because the helper takes
                // the same one and returns nothing.
                let without_daemon = app.core_client.is_none();
                crate::tui::commands::team::dispatch_team_command(app, "");
                if without_daemon {
                    PanelFill::Filled
                } else {
                    PanelFill::Loading
                }
            }
        }
        // Mirrors `dispatch_control_command`: it refuses without a session and
        // otherwise starts an async read.
        P::Control => {
            if app.active_session_id().is_none() {
                app.messages_state
                    .toasts
                    .info("No active session — open a session tab first");
                PanelFill::Cannot("no active session")
            } else {
                crate::tui::commands::control::dispatch_control_command(app, "");
                PanelFill::Loading
            }
        }
        // Mounts the Task view and then refreshes it, so the box is real and
        // incomplete — the same shape as `Research`.
        P::TaskView => {
            crate::tui::commands::work_orders::open_task_view(app);
            PanelFill::StillLoading
        }
        // Reports for itself: a clean tree, a missing git root, and a failed
        // `git diff` all return without opening anything.
        P::Diff => {
            if app.handle_diff_command(None) {
                PanelFill::Filled
            } else {
                PanelFill::Cannot("no changes to show")
            }
        }
        P::Review => {
            app.open_dialog(Dialog::Review);
            PanelFill::Filled
        }
        P::Files => {
            app.open_dialog(Dialog::Files);
            PanelFill::Filled
        }
    }
}

/// Whether `action` is one of the Normal-mode-only gestures.
///
/// Re-exported from the input layer so the editor asks the same question
/// `App::on_key` already answers, instead of growing a second list that
/// could drift from the mode gate that makes the keys safe.
pub(crate) use crate::tui::input::is_normal_only;

/// Apply one Normal-mode-only gesture.
///
/// The single dispatcher for the directionals and the panel manager, shared
/// by `App::on_key` and the editor buffer so `Shift+L` means the same thing
/// whether the keyboard is in the composer or in the editor. Returns `true`
/// when the gesture was handled.
pub(crate) fn handle_normal_only_action(app: &mut App, action: InputAction) -> bool {
    match action {
        InputAction::DirectionLeft => {
            move_directionally(app, PanelDirection::Left);
            true
        }
        InputAction::DirectionRight => {
            move_directionally(app, PanelDirection::Right);
            true
        }
        InputAction::DirectionUp => {
            move_directionally(app, PanelDirection::Up);
            true
        }
        InputAction::DirectionDown => {
            move_directionally(app, PanelDirection::Down);
            true
        }
        InputAction::OpenPanelManager => {
            toggle_panel_selector(app);
            true
        }
        _ => false,
    }
}

/// Focus the active panel, if it can act. Reports why it could not rather
/// than leaving keys to a panel that would ignore them.
///
/// Taking focus also parks the composer's mode and switches the app to
/// Normal, because a panel is a Normal-mode surface and `App::on_key`
/// consults a focused panel *only* in Normal. Leaving the composer in
/// Insert while the panel holds the keyboard produced the worst possible
/// state: the badge and the hints said the panel had the keyboard, and
/// every key — including `Shift+H`, the way out — went to the composer
/// instead. That is what docking a box from Insert mode used to do.
pub(crate) fn focus_panel(app: &mut App) -> bool {
    let focused =
        app.panel_host.is_visible_at(app.panel_host.placement()) && app.panel_host.focus();
    if focused {
        app.composer_mode_before_panel_focus
            .get_or_insert(app.ui_state.input_mode);
        app.ui_state.input_mode = crate::tui::input::InputMode::Normal;
    }
    sync_workspace_focus(app);
    focused
}

/// Return focus to the composer, restoring whichever NOR/INS mode it had
/// before the panel took the keyboard.
///
/// Restoring rather than forcing Insert is what makes leaving a panel
/// feel like stepping back out of it: a user who was mid-sentence keeps
/// their caret, and one who was reading comes back to Normal.
pub(crate) fn blur_panel(app: &mut App) {
    app.panel_host.blur();
    restore_composer_mode_if_unfocused(app);
    sync_workspace_focus(app);
}

/// Focus the panel docked at `placement` — the pointer's answer to
/// `Shift+H/J/K/L`.
///
/// Goes through the same park/restore of the composer mode that
/// [`focus_panel`] owns, so a click and the keyboard cannot leave the composer
/// in different modes.
pub(crate) fn focus_panel_at(app: &mut App, placement: PanelPlacement) {
    let focused = app.panel_host.focus_at(placement);
    if focused {
        app.composer_mode_before_panel_focus
            .get_or_insert(app.ui_state.input_mode);
        app.ui_state.input_mode = InputMode::Normal;
    }
    sync_workspace_focus(app);
}

/// Give the composer back the mode it had before a panel took the keyboard.
///
/// Split out because two different routes leave a panel — [`blur_panel`] for
/// the directional gesture, and `SidePanelHost`'s own `blur()` for the
/// `Esc`/`q` exits — and both owe the composer the same restoration.
fn restore_composer_mode_if_unfocused(app: &mut App) {
    if app.panel_host.is_focused() {
        return;
    }
    if let Some(mode) = app.composer_mode_before_panel_focus.take() {
        app.ui_state.input_mode = mode;
    }
}

/// Resolve one directional gesture (`Shift+←/→/↑/↓`, `Shift+H/J/K/L`).
///
/// This is the single meaning of those eight keys across every surface. Two
/// things are read here rather than at each call site, and both come from
/// the one [`PanelPlacement`]:
///
/// * **A dockable box is open** → the gesture *moves the box* into the edge
///   the direction names, swapping whatever was there. "I opened `/chat` and
///   now I want it over there."
/// * **No box is open** → the direction is resolved against what is actually
///   on screen: a direction naming an edge that is showing a panel takes the
///   keyboard into it, the direction away from the selected edge returns to
///   the main buffer, and the remaining perpendicular directions cycle which
///   panel type is loaded.
///
/// The second bullet is why the host is consulted rather than a hardcoded
/// "right". With one right-hand column, `→` enters and `←` leaves; move the
/// panel left and those two swap; put a panel under the buffer and `↓`/`↑`
/// take over. Because the answer comes from the slots, a second column is
/// reachable with the same keys that moved the first one there — no new
/// gesture, and no second implementation of the old one.
///
/// A box calls this from its own Normal-mode handler (a modal short-circuits
/// key routing before the binding map runs); the main buffer arrives via the
/// [`InputAction`] arms in `App::on_key`.
///
/// ## The main buffer is a stop, not a gap
///
/// The second column is not "the other end of the line" from the first one —
/// the **main buffer sits between them**. So stepping away from a panel in a
/// direction that also names another populated edge lands on the buffer
/// first, and the *next* press in that same direction reaches the far edge.
/// With Terminal on the left and Chat on the right, two `Shift+H` from Chat
/// reach Terminal; one jumped straight over the buffer the user was sitting
/// in.
///
/// That is the whole difference between "go left" and "go to the left panel",
/// and it is why this asks *where the keyboard is* rather than which edge is
/// selected: the answer is a position on a line, not a pair of edges.
pub(crate) fn move_directionally(app: &mut App, dir: PanelDirection) {
    if app.dock_target().is_some() {
        dock_open_box(app, dir);
        return;
    }
    // `Up` names no edge — there is no top — so it can never be read as "the
    // panel is over there". That is what stops an empty column from shadowing
    // the way out of a bottom row.
    let named = PanelPlacement::named_by(dir);

    // Keyboard in the main buffer: the gesture steps *into* a populated edge.
    if !app.panel_host.is_focused() {
        if let Some(placement) = named.filter(|p| app.panel_host.is_visible_at(*p)) {
            app.panel_host.select_placement(placement);
            focus_side_panel(app);
            return;
        }
        // Nothing that way. The perpendicular gestures still walk the panel
        // list, so browsing from the buffer is not a dead end.
        if let Some(step) = app.panel_host.placement().cycle_step(dir) {
            step_panel(app, step);
        }
        return;
    }

    let placement = app.panel_host.placement();
    // Stepping *away* from this edge means the main buffer — always, even when
    // the far column has a panel on it. Routing past it made the buffer
    // invisible as a destination, so the one surface the user spends most of
    // their time could not be returned to by the gesture that names it.
    if dir == placement.away() {
        focus_main_buffer(app);
        return;
    }
    if let Some(edge) = named.filter(|p| app.panel_host.is_visible_at(*p)) {
        if edge != placement {
            app.panel_host.select_placement(edge);
        }
        focus_side_panel(app);
        return;
    }
    // Perpendicular to the placement axis, so the gesture is free to mean
    // "walk the panel list". `None` here would be the axis pair, already
    // answered above — one of the two branches always runs.
    if let Some(step) = placement.cycle_step(dir) {
        step_panel(app, step);
    }
}

/// Cycle the loaded panel type by `delta`, keeping focus where it was.
///
/// Selecting a panel does not seize the keyboard: the perpendicular gesture is
/// how a user walks the list while reading titles, and stealing the composer
/// mid-scroll would send their next keystroke to a panel they only looked at.
/// Focus follows only if it already had it.
///
/// Walking deliberately does not materialize empty panels: cycling every entry
/// would fire dozens of loads (several of them daemon round trips) for panels
/// the user only glanced at. Committing from the manager (or a hotkey) is the
/// gesture that loads.
pub(crate) fn step_panel(app: &mut App, delta: i8) {
    if delta == 0 || SidePanelId::ALL.len() < 2 {
        return;
    }
    // Chat must be bound before it can be selected, and only this layer can
    // resolve a project — the host has no app access.
    sync_chat_panel_project(app);
    {
        let App {
            panel_host,
            sidebar,
            chat,
            interactive_terminals,
            chat_panel_project,
            sidebar_area,
            ui_state,
            ..
        } = app;
        let focused = panel_host.is_focused();
        let mut ctx = panel_ctx(
            ui_state,
            sidebar,
            chat,
            interactive_terminals,
            chat_panel_project,
            *sidebar_area,
            focused,
            ui_state.input_mode,
        );
        panel_host.step(delta, &mut ctx);
    }
    sync_workspace_focus(app);
}

/// Dock the open box into the panel.
///
/// This is the multiplexer move: instead of "open the chat box, then separately
/// open the chat panel", the box is *moved* into the panel and the box goes
/// away. Both surfaces run the same [`crate::tui::chat_browser`], so nothing
/// is lost in the transfer — only the frame it draws in changes.
///
/// `dir` names the edge that should receive it, and it always can: pointing a
/// panel at an edge that is not currently occupied **moves** it there and lets
/// the edge it left restore whatever it was showing. So `/chat` then `Shift+H`
/// walks a chat panel to the left column, and whatever was in the right column
/// comes back — including nothing at all, in which case that column simply
/// collapses.
///
/// The earlier behaviour here was to refuse a gesture naming an empty edge,
/// naming the key that worked instead. That was honest but a dead end: the
/// user had said where they wanted the panel, the frame had a perfectly good
/// place to put it, and the answer was "no". Swapping keeps the gesture
/// truthful — pushing a box left *moves* it left — which is the property that
/// made the refusal worth replacing rather than keeping.
pub(crate) fn dock_open_box(app: &mut App, dir: PanelDirection) {
    let Some(id) = app.dock_target() else {
        app.messages_state
            .toasts
            .warning("No panel for this box — open a panel-capable box first");
        return;
    };
    // Refuse **before** unmounting. `SidePanelHost::adopt` consumes the widget
    // it is handed and answers `false` for a panel that is not hosted, so
    // finding that out afterwards means the box is already gone — it used to be
    // discarded exactly like that, silently, because the `bool` went unread.
    //
    // The predicate is `discards_a_docked_box`, **not** `is_bespoke`: `Help`,
    // `Terminal` and `Chat` are bespoke and still dock correctly, because they
    // build or share the very content their box was showing. Only `Agents`
    // pairs a box with a panel that shows something else.
    if id.discards_a_docked_box() {
        app.messages_state.toasts.warning(&format!(
            "{} is the management list, not the editor box — there is nothing to dock into",
            id.label()
        ));
        return;
    }
    // Close first: `select_panel` reads sidebar visibility and focus, and a
    // box still mounted would keep claiming keys the panel is about to own.
    //
    // `close_dialog` hands back the component it unmounted. For a hosted
    // panel that component is the box itself — same filter, same cursor,
    // same scroll offset — so docking is a move rather than a re-open that
    // happens to look similar. `Help`, `Terminal` and `Chat` ignore the
    // returned component and rebuild from their own state, which is why a
    // `false` from `adopt` below is expected for them rather than a failure.
    let unmounted = app.close_dialog();
    let from = app.panel_host.placement();
    let placement = edge_named_by(dir, from);
    app.panel_host.dock(id, placement);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
    if let Some(widget) = unmounted {
        app.panel_host.adopt(id, widget);
    }
    select_panel(app, id);
    focus_panel(app);
    // Only say something when the panel actually crossed an edge. Docking
    // where it already was is the common case and needs no narration, but a
    // swap is exactly the moment the user cannot see the other edge change
    // on its own — it happened off to the side of the gesture they pressed.
    if from != placement {
        app.messages_state.toasts.info(&format!(
            "{} moved to the {}; the {} is now {}",
            id.label(),
            placement.label(),
            from.label(),
            describe_vacated(app, from)
        ));
    }
}

/// The edge a directional gesture names.
///
/// The gesture is literal — `Shift+H` means the left column — rather than
/// being reinterpreted through the selected edge's axis. That is the same
/// rule `move_directionally` uses when there is no box to dock, so the two
/// paths cannot disagree about what `H` means; and it is what makes pushing a
/// box to the far side work without the user first moving the panel there.
///
/// `Down` is the bottom row. `Up` names no edge — there is no top — so it
/// resolves to the current placement rather than inventing one: a user who
/// presses `Shift+K` with a box open gets the panel where it already is,
/// which is at least the reading they can act on.
fn edge_named_by(dir: PanelDirection, current: PanelPlacement) -> PanelPlacement {
    // One table, shared with the manager's own directional: `named_by` says
    // which edge a gesture means, and this only decides what the one gesture
    // that names no edge should do instead.
    PanelPlacement::named_by(dir).unwrap_or(current)
}

/// Move keyboard focus into the panel, on whichever side it is docked.
///
/// The gesture that reaches here is [`PanelPlacement::toward`], so under a
/// left placement it is `Shift+H` and under a bottom row `Shift+J` — the
/// meaning is the same, only the spelling moved with the panel.
///
/// Shows the panel when it is hidden rather than refusing: the gesture is the
/// direct way to reach it, and a toast telling the user to press `Ctrl+B` and
/// then the same key again is a worse answer than just doing it.
///
/// An empty hosted panel is materialized first, so this is the same one
/// step as picking it from the manager. A live-loading box stays open
/// instead and keeps the keyboard.
pub(crate) fn focus_side_panel(app: &mut App) {
    let placement = app.panel_host.placement();
    // Reveal it: the gesture is the direct way to reach this edge, and a toast
    // telling the user to press `Ctrl+B` and then the same key again is a
    // worse answer than just doing it.
    app.panel_host.set_visible_at(placement, true);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
    if select_panel(app, app.panel_host.active()) == SelectOutcome::BoxOpen {
        return;
    }
    if !focus_panel(app) {
        app.messages_state
            .toasts
            .info("Side panel has nothing to show yet");
    }
}

/// Move keyboard focus back to the main buffer.
///
/// The gesture that reaches here is [`PanelPlacement::away`] — `Shift+H` on a
/// right column, `Shift+L` on a left one, `Shift+K` under the buffer.
///
/// This also re-arms the composer for the agent session, because the chat
/// target is derived from panel focus rather than latched — so there is no
/// way to leave the panel still pointed at chat.
pub(crate) fn focus_main_buffer(app: &mut App) {
    blur_panel(app);
}

/// Dock the panel on `placement`, show it, and remember the choice.
///
/// Applies immediately and confirms in one line, because "which side is this
/// on" is a question the frame answers badly: the user has to see the panel
/// move to believe it moved.
///
/// The *panel* is left alone — switching sides is not a request to re-select
/// what this edge holds. What does change is the other edge: the panel this
/// one displaces is moved across and the edge left behind restores what it
/// was showing, so a user who walks a panel to the other side does not lose
/// whatever was in the way.
pub(crate) fn set_panel_placement(app: &mut App, placement: PanelPlacement) {
    if app.panel_host.placement() == placement {
        // Still confirm. A keypress that appears to do nothing is read as a
        // dead key, and the honest "already there" is only useful if it is
        // visible.
        app.messages_state
            .toasts
            .info(&format!("Panel is already {}", placement.label()));
        return;
    }
    let from = app.panel_host.placement();
    // Swap first so the toast can describe what the vacated edge became,
    // rather than what it used to be.
    app.panel_host.dock(app.panel_host.active(), placement);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
    persist_panel_placement(app, placement);
    app.messages_state.toasts.info(&format!(
        "Panel moved to the {}; the {} is now {}",
        placement.label(),
        from.label(),
        describe_vacated(app, from)
    ));
}

/// Name what an edge shows after a panel left it, for the swap confirmation.
///
/// Says "collapsed" rather than "empty" when the edge had nothing before,
/// because that is the word `Ctrl+B` uses for the same state — the user
/// recognises one, and would have to guess the other.
fn describe_vacated(app: &App, placement: PanelPlacement) -> String {
    if app.panel_host.is_visible_at(placement) {
        app.panel_host.panel_at(placement).label().to_string()
    } else {
        "collapsed".to_string()
    }
}

/// Step to the next placement. The manager's single layout key.
///
/// Carries the current panel across rather than only re-pointing the
/// gestures, so `p` and "choose a side" are the same act: a user stepping
/// through layouts is trying the panel on another edge, not parking it on
/// one and leaving an empty column behind.
pub(crate) fn cycle_panel_placement(app: &mut App) {
    let from = app.panel_host.placement();
    let panel = app.panel_host.active();
    let next = app.panel_host.cycle_placement();
    app.panel_host.dock(panel, next);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
    persist_panel_placement(app, next);
    app.messages_state.toasts.info(&format!(
        "Panel moved to the {}; the {} is now {}",
        next.label(),
        from.label(),
        describe_vacated(app, from)
    ));
}

/// Write the panel placement back to the **global** config.
///
/// Same path and same reasoning as [`persist_panel_hotkeys`]: a layout is a
/// per-user preference, and `Config::save()` would confine it to one checkout
/// and could write it into a tracked file. A failure is reported but never
/// rolled back — the frame the user is looking at is already correct.
fn persist_panel_placement(app: &mut App, placement: PanelPlacement) {
    let mut config = match crate::config::schema::Config::load() {
        Ok(config) => config,
        Err(error) => {
            app.messages_state
                .toasts
                .error(&format!("Layout not saved: {error}"));
            return;
        }
    };
    config.panel_placement = Some(placement.as_str().to_string());
    if let Err(error) = config.save_global() {
        app.messages_state
            .toasts
            .error(&format!("Layout not saved: {error}"));
    }
}

/// Hide or show the panel the user is actually looking at (`Ctrl+B`).
///
/// Scoped to the *selected* pane, not to a hardcoded right column: the same
/// key closes a left column or a bottom row, because what it closes is the
/// panel the keyboard can reach and the frame is drawing. A single-panel
/// host means there is nothing to disambiguate between yet — the seam is
/// placed here so adding a second one does not mean finding every call site
/// that had assumed "the right one".
///
/// Showing is never refused: this is the only key that reveals a hidden
/// panel, so a toast asking the user to press it again would be a loop. An
/// empty hosted panel is materialized first, which makes this the same single
/// step as choosing it from the manager.
pub(crate) fn toggle_selected_pane(app: &mut App) {
    let placement = app.panel_host.placement();
    if app.panel_host.is_visible_at(placement) {
        app.panel_host.set_visible_at(placement, false);
        // Only the edge that had the keyboard gives it up. Collapsing a
        // different column while the user is driving this one would strand
        // keys in a panel that is no longer on screen.
        if app.panel_host.placement() == placement {
            blur_panel(app);
            app.sidebar.blur_sidebar();
        }
        app.ui_state.sidebar_visible = app.panel_host.any_visible();
        return;
    }
    app.panel_host.set_visible_at(placement, true);
    app.ui_state.sidebar_visible = app.panel_host.any_visible();
    if select_panel(app, app.panel_host.active()) == SelectOutcome::BoxOpen {
        return;
    }
    if !focus_panel(app) {
        app.messages_state
            .toasts
            .info("Side panel has nothing to show yet");
    }
    if app.panel_host.active() == SidePanelId::Session {
        app.sidebar.focus_sidebar();
    }
}

/// Route one key to the focused panel and apply any intent it emits.
///
/// Returns `true` when the panel consumed the key. A focused panel consumes
/// everything by contract — see `panels/mod.rs` for why a leak would submit
/// an agent turn.
/// Answer one key while yazi-style resize mode is armed, or while it is being
/// toggled. Returns whether the key was consumed.
///
/// Called from `App::on_key` ahead of the focused panel and ahead of the
/// binding map, for two reasons that make it impossible to place anywhere
/// else:
///
/// 1. A focused panel consumes every key it is shown, and `h`/`j`/`k`/`l` are
///    its cursor movements. Resizing below the panel would mean a panel that
///    silently ate the resize keys instead of moving.
/// 2. `Ctrl+N` has to work with the keyboard in the **main buffer** too. The
///    main buffer is the other half of the gesture — "make the chat column
///    slimmer" is `l` from the transcript, and it is the same border `h` grows
///    from the column's side.
///
/// Not routed through `TuiMsg`: every effect here is a synchronous frame
/// mutation, and routing it through the message bus would only add a second
/// spelling of "resize" for a key that has exactly one.
pub(crate) fn handle_panel_resize_key(app: &mut App, key: KeyEvent) -> bool {
    use crate::tui::panel_resize::{apply_resize, resize_edge_of, resolve_resize};
    use crossterm::event::{KeyCode, KeyModifiers};

    // Toggle. `Ctrl+N` rather than a bare letter because `n` is NewSession, and
    // `Ctrl+N` was that too — the remap to a bare `n` is what freed the chord.
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('n') {
        let armed = app.panel_host.toggle_resizing();
        announce_resize(app, armed);
        return true;
    }

    if !app.panel_host.is_resizing() {
        return false;
    }

    // Three ways out, because the mode is modal and a mode with one exit key
    // is a trap. `Enter` is here as the "commit" spelling: resizing is
    // immediate, so the reader reaches for it out of habit and it costs
    // nothing.
    if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
        app.panel_host.set_resizing(false);
        announce_resize(app, false);
        return true;
    }

    let Some(edge) = resize_edge_of(key) else {
        // Anything else is routed normally. Resize mode owns exactly four
        // keys, and claiming more than it acts on would make the mode feel
        // like a trap rather than a layer.
        return false;
    };

    let subject = app.panel_host.resize_subject();
    let occupied = app.panel_host.occupied_placements();
    // A resize key naming a frame edge is *consumed and ignored* rather than
    // passed through: letting `l` reach the panel's cursor movement on some
    // rows and resize a border on others is what makes a mode feel haunted.
    if let Some(delta) = resolve_resize(subject, edge, &occupied) {
        apply_resize(&mut app.ui_state.layout, delta);
    }
    true
}

/// Name the mode and the way out, on a toast the reader cannot miss.
///
/// Resize mode moves borders with no other visible feedback beyond the frame
/// itself, and the frame is exactly what the reader is watching — so without a
/// line naming the exit keys, "nothing is happening" and "I am in a mode"
/// look identical.
fn announce_resize(app: &mut App, armed: bool) {
    use crate::tui::panel_resize::ResizeSubject;
    if armed {
        let subject = match app.panel_host.resize_subject() {
            ResizeSubject::Main => "the LLM buffer",
            ResizeSubject::Panel(placement) => placement.label(),
        };
        let message = format!(
            "Resize {subject}: h/j/k/l drag its own borders outward · Esc or Enter to stop"
        );
        app.messages_state.toasts.info(&message);
    } else {
        app.messages_state
            .toasts
            .info("Resize off: h/j/k/l move the cursor again");
    }
}

/// Route one key to the focused panel. Runs after [`handle_panel_resize_key`],
/// which owns the modal resize layer.
pub(crate) fn handle_panel_key(app: &mut App, key: KeyEvent, action: Option<InputAction>) -> bool {
    if !app.panel_host.is_visible_at(app.panel_host.placement()) {
        return false;
    }
    // A focused panel must not swallow the only way back to the manager, so
    // the host answers the toggle itself. Committing to Chat needs a bound
    // project, which only this layer can resolve — so bind here, on the press
    // that opens the box, rather than on every keystroke.
    //
    // This is the one key a focused panel cannot own. With `Ctrl+Alt+W`
    // retired, `Shift+S` is the *only* spelling of the manager, so letting a
    // panel consume it would leave a user inside a panel with no way to
    // reach the switcher for it. Answering it here — ahead of the panel,
    // exactly like `Ctrl+B` and the directionals — is what keeps the
    // manager reachable from wherever the keyboard currently is.
    if action == Some(InputAction::OpenPanelManager) {
        // Toggle: pressing the key again with the manager up closes it,
        // rather than stacking a second copy of the same box.
        if app
            .focus_manager
            .has_dialog(crate::tui::components::component::DialogType::PanelManager)
        {
            app.close_dialog();
            return true;
        }
        sync_chat_panel_project(app);
        open_panel_manager(app);
        return true;
    }
    let outcome = {
        let App {
            panel_host,
            sidebar,
            chat,
            interactive_terminals,
            chat_panel_project,
            sidebar_area,
            ui_state,
            ..
        } = app;
        let focused = panel_host.is_focused();
        let mut ctx = panel_ctx(
            ui_state,
            sidebar,
            chat,
            interactive_terminals,
            chat_panel_project,
            *sidebar_area,
            focused,
            ui_state.input_mode,
        );
        panel_host.handle_key(key, action, &mut ctx)
    };
    let Some(outcome) = outcome else {
        return false;
    };
    // Leaving the panel by *any* route hands the composer its mode back:
    // the gesture lands in `blur_panel`, but the host blurs directly for
    // the `Esc`/`q` exits, so the restore has to happen here too or `Esc`
    // would drop the user into the composer in Normal whatever they were
    // doing before. `take()` makes this idempotent with `blur_panel`.
    restore_composer_mode_if_unfocused(app);
    sync_workspace_focus(app);
    match outcome {
        PanelKey::Intent(msg) => {
            app.process_msg(msg);
            true
        }
        PanelKey::Consumed => true,
        // The panel is a viewer and the key belongs to the composer. Routing
        // continues so the bottom prompt — which renders the active
        // `[BUILD]`/`[CHAT]` target — receives it.
        PanelKey::PassThrough => false,
    }
}

/// Whether the focused panel takes the keyboard in Insert mode.
///
/// See [`SidePanel::claims_insert_keys`](crate::tui::panels::SidePanel::claims_insert_keys).
/// The context is built and dropped here so the query stays a single explicit
/// grant; the panel never sees `&App`.
pub(crate) fn panel_claims_insert_keys(app: &mut App) -> bool {
    let App {
        panel_host,
        sidebar,
        chat,
        interactive_terminals,
        chat_panel_project,
        sidebar_area,
        ui_state,
        ..
    } = app;
    if !panel_host.is_focused() || !panel_host.is_visible_at(panel_host.placement()) {
        return false;
    }
    let mut ctx = panel_ctx(
        ui_state,
        sidebar,
        chat,
        interactive_terminals,
        chat_panel_project,
        *sidebar_area,
        true,
        InputMode::Insert,
    );
    panel_host.claims_insert_keys(&mut ctx)
}

/// Build the render/key context for the active panel.
///
/// Takes the fields it needs rather than `&mut App`: the host mutates
/// `panel_host` while the context borrows `sidebar`/`chat`, so the two
/// borrows must stay disjoint. Every field is an explicit grant — there is
/// no `&App` here on purpose, because a panel that could see the whole app
/// could reach a locator or a client, and the point of this boundary is
/// that it cannot.
fn panel_ctx<'a>(
    ui_state: &'a crate::tui::app::state::ui::UiState,
    sidebar: &'a mut crate::tui::components::sidebar::SidebarWidget,
    chat: &'a mut ChatState,
    terminal: &'a mut crate::tui::interactive_terminal::InteractiveTerminalController,
    chat_panel_project: &'a Option<String>,
    sidebar_area: Option<ratatui::layout::Rect>,
    focused: bool,
    input_mode: crate::tui::input::InputMode,
) -> PanelCtx<'a> {
    let theme: Arc<Theme> = Arc::clone(&ui_state.theme);
    PanelCtx {
        theme,
        // Display-only clock for composing-lease expiry. No I/O, and it
        // never feeds back into a request.
        now_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        session: sidebar,
        chat,
        terminal,
        vim_mode: ui_state.vim_mode,
        chat_project: chat_panel_project.as_deref(),
        area: sidebar_area.unwrap_or_default(),
        // Supplied by the caller: it holds `panel_host` mutably while the
        // context is alive, so the host's focus flag cannot be read here.
        focused,
        input_mode,
    }
}

/// Keep `WorkspaceFocus` reporting which side owns the keyboard.
///
/// The Workspace view renders a focus hint from this and its `Esc` arm
/// branches on it, so it is a view of the host's state rather than an
/// independent second focus owner.
pub(crate) fn sync_workspace_focus(app: &mut App) {
    let chat_focused = app.panel_host.is_focused() && app.panel_host.active() == SidePanelId::Chat;
    let focus = if chat_focused {
        crate::tui::app::state::WorkspaceFocus::Chat
    } else {
        crate::tui::app::state::WorkspaceFocus::Composer
    };
    app.workspace_focus = focus;
}

/// Drop the chat panel's per-view state and unbind it.
///
/// Called when the chat projection is cleared so a composer buffer cannot
/// outlive the data it was composed against.
pub(crate) fn reset_chat_panel(app: &mut App) {
    app.panel_host.chat_panel_mut().reset();
    app.panel_host.chat_panel_mut().set_bound(false);
}

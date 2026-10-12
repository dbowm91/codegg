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
use crate::tui::input::InputAction;
use crate::tui::panels::{PanelCtx, PanelDirection, PanelKey, SidePanelId};
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

/// Open the panel manager (`Ctrl+Alt+W`, and `Shift+S` in Normal mode).
///
/// A *box*, not an overlay drawn inside the column. The old grid selector
/// showed two unlabelled buttons in 30 columns and could not be filtered;
/// the manager names each panel, says what it is for, shows which digit is
/// already bound, and offers the same type-to-filter the rest of the TUI
/// uses. It also has somewhere to explain itself, which is what a switcher
/// for a growing registry needs.
pub(crate) fn open_panel_manager(app: &mut App) {
    if !app.ui_state.sidebar_visible {
        app.ui_state.sidebar_visible = true;
    }
    // Resolve the chat binding now, before the user can pick Chat: the host
    // has no app access and would otherwise find it unbound and refuse focus.
    sync_chat_panel_project(app);
    let dialog = crate::tui::components::dialogs::panel_manager::PanelManagerDialog::new(
        Arc::clone(&app.ui_state.theme),
        app.panel_hotkeys.clone(),
        app.panel_host.active(),
    );
    app.push_dialog(crate::tui::app::Dialog::PanelManager, Box::new(dialog));
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
    if !app.ui_state.sidebar_visible {
        app.ui_state.sidebar_visible = true;
    }
    if panel == SidePanelId::Chat {
        sync_chat_panel_project(app);
    }
    select_panel(app, panel);
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
pub(crate) fn select_panel(app: &mut App, id: SidePanelId) {
    if id == SidePanelId::Chat {
        sync_chat_panel_project(app);
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
}

/// Focus the active panel, if it can act. Reports why it could not rather
/// than leaving keys to a panel that would ignore them.
pub(crate) fn focus_panel(app: &mut App) -> bool {
    let focused = app.ui_state.sidebar_visible && app.panel_host.focus();
    sync_workspace_focus(app);
    focused
}

/// Return focus to the composer.
pub(crate) fn blur_panel(app: &mut App) {
    app.panel_host.blur();
    sync_workspace_focus(app);
}

/// Resolve one directional gesture (`Shift+←/→/↑/↓`, `Shift+H/J/K/L`).
///
/// This is the single meaning of those eight keys across every surface, and
/// the branch at the top is the whole design:
///
/// * **A dockable box is open** → the gesture *moves the box* into the side
///   panel and the box goes away. "I opened `/chat` and now I want it to keep
///   existing over there."
/// * **No box is open** → the gesture *selects a pane*: `→` takes the
///   keyboard into the side panel, `←` returns it to the main buffer, and
///   `↑`/`↓` cycle which panel type is loaded.
///
/// Reading the context here rather than at each call site is what keeps a box
/// and its panel from implementing the same rule twice and drifting. A box
/// calls this from its own Normal-mode handler (a modal short-circuits key
/// routing before the binding map runs); the main buffer arrives via the
/// [`InputAction`] arms in `App::on_key`.
pub(crate) fn move_directionally(app: &mut App, dir: PanelDirection) {
    if app.dock_target().is_some() {
        dock_open_box(app);
        return;
    }
    match dir {
        PanelDirection::Left => focus_main_buffer(app),
        PanelDirection::Right => focus_side_panel(app),
        // Only the vertical pair walks the panel list; in a two-pane layout
        // the horizontals already have a job and `panel_step` returns 0 for
        // them, so this is the single call site rather than a silent no-op.
        up_down => step_panel(app, up_down.panel_step()),
    }
}

/// Cycle the loaded panel type by `delta`, keeping focus where it was.
///
/// Selecting a panel does not seize the keyboard: `↑`/`↓` is how a user walks
/// the list while reading titles, and stealing the composer mid-scroll would
/// send their next keystroke to a panel they only looked at. Focus follows
/// only if it already had it.
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

/// Dock the open box into the side panel.
///
/// This is the multiplexer move: instead of "open the chat box, then separately
/// open the chat panel", the box is *moved* into the panel and the box goes
/// away. Both surfaces run the same [`crate::tui::chat_browser`], so nothing
/// is lost in the transfer — only the frame it draws in changes.
///
/// Adding the next box-capable surface is one `match` arm in
/// [`dock_target`](App::dock_target) plus one `SidePanel` impl; the close,
/// show, focus and binding-order logic below is shared by all of them.
pub(crate) fn dock_open_box(app: &mut App) {
    let Some(id) = app.dock_target() else {
        app.messages_state
            .toasts
            .warning("No panel for this box — open a panel-capable box first");
        return;
    };
    // Close first: `select_panel` reads sidebar visibility and focus, and a
    // box still mounted would keep claiming keys the panel is about to own.
    app.close_dialog();
    if !app.ui_state.sidebar_visible {
        app.ui_state.sidebar_visible = true;
    }
    select_panel(app, id);
    focus_panel(app);
}

/// Move keyboard focus into the side panel (`Shift+Right`).
///
/// Shows the column when it is hidden rather than refusing: the key is the
/// only way to reach the panel, and a toast telling the user to press
/// `Ctrl+B` and then the same key again is a worse answer than just doing it.
///
/// Binding is resolved first because the Chat panel refuses focus while
/// unbound — arming the target is part of arriving there, not a separate
/// step the user has to know about.
pub(crate) fn focus_side_panel(app: &mut App) {
    if !app.ui_state.sidebar_visible {
        app.ui_state.sidebar_visible = true;
    }
    sync_chat_panel_project(app);
    if !focus_panel(app) {
        app.messages_state
            .toasts
            .info("Side panel has nothing to show yet");
    }
}

/// Move keyboard focus back to the main buffer (`Shift+Left` / `Shift+H`).
///
/// This also re-arms the composer for the agent session, because the chat
/// target is derived from panel focus rather than latched — so there is no
/// way to leave the panel still pointed at chat.
pub(crate) fn focus_main_buffer(app: &mut App) {
    blur_panel(app);
}

/// Route one key to the focused panel and apply any intent it emits.
///
/// Returns `true` when the panel consumed the key. A focused panel consumes
/// everything by contract — see `panels/mod.rs` for why a leak would submit
/// an agent turn.
pub(crate) fn handle_panel_key(app: &mut App, key: KeyEvent, action: Option<InputAction>) -> bool {
    if !app.ui_state.sidebar_visible {
        return false;
    }
    // A focused panel must not swallow the only way back to the manager, so
    // the host answers the toggle itself. Committing to Chat needs a bound
    // project, which only this layer can resolve — so bind here, on the press
    // that opens the box, rather than on every keystroke.
    if action == Some(InputAction::TogglePanelSelector) {
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

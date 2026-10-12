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
use crate::tui::panels::{PanelCtx, PanelKey, SidePanelId};
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

/// Open the `Ctrl+Alt+W` panel switcher.
///
/// Refused when the side column is hidden: the switcher describes the
/// region it is switching, and with no region there is nothing to select
/// into.
///
/// Focus deliberately moves *only on commit*. Opening the overlay from the
/// composer and pressing `Esc` must leave the composer focused, not strand
/// the next keystroke in a panel the user never chose.
pub(crate) fn open_panel_selector(app: &mut App) {
    if !app.ui_state.sidebar_visible {
        app.messages_state
            .toasts
            .info("Side panel is hidden — Ctrl+B shows it");
        return;
    }
    // Resolve the chat binding now, before the host can commit to the Chat
    // panel: the host has no app access and would otherwise find it
    // unbound and refuse to focus it.
    sync_chat_panel_project(app);
    app.panel_host.open_selector();
    sync_workspace_focus(app);
}

/// Toggle the switcher from the action arm.
///
/// Reached when no panel holds focus, so the key never reaches the host's
/// own hotkey handling. When a panel *is* focused the host answers the same
/// action itself, keeping one owner for the toggle.
pub(crate) fn toggle_panel_selector(app: &mut App) {
    if app.panel_host.selector_open() {
        app.panel_host.cancel_selector();
        sync_workspace_focus(app);
    } else {
        open_panel_selector(app);
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

/// Move keyboard focus into the side panel (`Shift+Right` / `Shift+L`).
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
    // The host answers its own hotkey (a focused panel must not swallow the
    // only way back to the switcher), and committing to Chat needs a bound
    // project — which only this layer can resolve. So bind here, on the
    // press that opens the overlay, rather than on every keystroke.
    if !app.panel_host.selector_open() && action == Some(InputAction::TogglePanelSelector) {
        sync_chat_panel_project(app);
    }
    let outcome = {
        let App {
            panel_host,
            sidebar,
            chat,
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

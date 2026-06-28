//! UI state module.
//!
//! Contains [`UiState`] which manages UI-specific state that doesn't belong
//! to other state domains (session, prompt, messages, dialogs, agent).

use std::collections::HashMap;
use std::sync::Arc;

use ratatui::layout::Rect;
use tokio::sync::broadcast;

use crate::tts::Tts;
use crate::tui::app::types::Dialog;
use crate::tui::input::{InputAction, InputMode, KeybindConfig};
use crate::tui::layout::TuiLayout;
use crate::tui::route::RouteManager;
use crate::tui::theme::Theme;

/// Determines how the TUI operates with respect to resource ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// In-process mode: TUI owns DB, providers, scheduler, etc.
    Embedded,
    /// Socket/daemon mode: TUI connects to an external daemon via CoreClient.
    /// Does NOT own local resources — gets everything from the daemon.
    RemoteCore {
        /// The daemon endpoint URL
        endpoint: String,
    },
}

/// UI state including theme, layout, routes, and dialog management.
///
/// This state manages things that are purely UI-related:
/// - Theme and layout configuration
/// - Current route (Home/Session)
/// - Which dialog is open (if any)
/// - Command mode (for slash commands)
/// - Keybindings
pub struct UiState {
    /// Current color theme
    pub theme: Arc<Theme>,
    /// Layout manager
    pub layout: TuiLayout,
    /// Whether sidebar is visible
    pub sidebar_visible: bool,
    /// Auto-scroll messages to bottom
    pub auto_scroll: bool,
    /// Show reasoning/thinking sections
    pub show_thinking: bool,
    /// Show message timestamps
    pub show_timestamps: bool,
    /// Route manager for Home/Session navigation
    pub routes: RouteManager,
    /// Currently open dialog (None if closed)
    pub dialog: Dialog,
    /// Whether in command mode (slash prefix entered)
    pub command_mode: bool,
    /// Input mode (Insert/Normal - vim-style)
    pub input_mode: InputMode,
    /// Shutdown signal sender
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    /// Help dialog content lines
    pub help_lines: Vec<String>,
    /// Key bindings map
    pub bindings: HashMap<(crossterm::event::KeyModifiers, crossterm::event::KeyCode), InputAction>,
    /// Raw keybind config (for editing)
    pub keybinds: Option<KeybindConfig>,
    /// Whether vim mode is enabled (affects help text and normal mode bindings)
    pub vim_mode: bool,
    /// Operating mode (Embedded vs RemoteCore)
    pub mode: AppMode,
    pub remote_status: Option<String>,
    /// Event loop running flag
    pub running: bool,
    /// Timeline visible
    pub timeline_visible: bool,
    pub timeline_selected: usize,
    pub render_panic_count: usize,
    pub last_render_error: Option<String>,
    /// Text-to-speech engine
    pub tts: Tts,
    /// TTS enabled/disabled state
    pub tts_enabled: bool,
    /// Fullscreen mode (DEC 1049 alternate screen buffer)
    pub fullscreen: bool,
    /// Dirty region for partial redraw optimization
    pub dirty_regions: Vec<Rect>,
    /// Resize debounce timer
    pub resize_debounce: Option<std::time::Instant>,
    /// When true, TTS requests route through the daemon's NotificationRouter
    /// instead of speaking locally. Set in RemoteCore mode.
    pub tts_via_daemon: bool,
    pub diagnostics: crate::tui::app::state::diagnostics::TuiDiagnostics,
}

impl UiState {
    pub fn add_dirty_region(&mut self, region: Rect) {
        self.dirty_regions.push(region);
    }

    pub fn clear_dirty_regions(&mut self) {
        self.dirty_regions.clear();
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty_regions.is_empty()
    }
}

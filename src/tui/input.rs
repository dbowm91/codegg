//! Keyboard input handling and keybindings.
//!
//! This module maps terminal key events to application [`InputAction`]s.
//!
//! ## Keybinding Architecture
//!
//! ```text
//! KeyEvent ──► parse_key() ──► build_bindings() ──► HashMap
//!                                                   (KeyModifiers, KeyCode) -> InputAction
//! ```
//!
//! ## Default Bindings
//!
//! | Key | Action |
//! |-----|--------|
//! | Enter | Send |
//! | Shift+Enter | Newline |
//! | Esc, Ctrl+C | Cancel |
//! | ↑/↓ | NavigateUp, NavigateDown |
//! | Tab | SwitchAgent |
//! | Shift+Tab | TogglePermissionMode |
//! | Ctrl+L | SelectModel |
//! | Ctrl+K | ClearSession |
//! | Ctrl+N | NewSession |
//! | Ctrl+T | ToggleSidebar |
//! | Ctrl+W | CloseSession |
//! | Ctrl+S | StashPrompt |
//! | Ctrl+R | RestorePrompt |
//! | Ctrl+P / Ctrl+Shift+P | CycleModelForward/Backward |
//! | PgUp/PgDn | PageUp/PageDown |
//! | Ctrl+F | Search |
//!
//! **Note:** In Insert mode, bare character keys (no modifier) always produce
//! text input. Only modifier-key combos trigger actions. `/` at position 0
//! activates command mode via `on_char()` detection, not via keybinding.
//!
//! ## Custom Keybindings
//!
//! Keybindings can be configured via config file using snake_case action names:
//!
//! ```json,ignore
//! {
//!   "keybinds": {
//!     "ctrl+s": "stash_prompt",
//!     "ctrl+r": "restore_prompt"
//!   }
//! }
//! ```

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;

#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        tracing::debug!(target: "codegg::tui::input", "{}", format!($($arg)*));
    };
}

#[cfg(not(feature = "debug-logging"))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Insert,
    Normal,
}

impl InputMode {
    pub fn toggle(&mut self) {
        *self = match self {
            InputMode::Insert => InputMode::Normal,
            InputMode::Normal => InputMode::Insert,
        };
    }
}

/// Mode for which help entries are relevant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpMode {
    Insert,
    Normal,
    Command,
    Dialog,
}

/// A single help entry describing a keybinding.
pub struct HelpEntry {
    pub mode: HelpMode,
    pub key: &'static str,
    pub action: &'static str,
    pub condition: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    Send,
    Newline,
    Cancel,
    NavigateUp,
    NavigateDown,
    SwitchAgent,
    SelectModel,
    ClearSession,
    NewSession,
    ToggleSidebar,
    ToggleSection,
    CloseSession,
    Help,
    FocusPrompt,
    StashPrompt,
    RestorePrompt,
    CopyMessage,
    CycleModelForward,
    CycleModelBackward,
    ToggleReasoning,
    Quit,
    ExternalEditor,
    Char(char),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Search,
    SearchNext,
    SearchPrev,
    ClearSearch,
    Command,
    ToggleTts,
    StopTts,
    ToggleFullscreen,
    TogglePermissionMode,
    OpenDiff,
    GoToTop,
    GoToBottom,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ActionKey {
    Send,
    Newline,
    Cancel,
    NavigateUp,
    NavigateDown,
    SwitchAgent,
    SelectModel,
    ClearSession,
    NewSession,
    ToggleSidebar,
    ToggleSection,
    CloseSession,
    Help,
    FocusPrompt,
    StashPrompt,
    RestorePrompt,
    CopyMessage,
    CycleModelForward,
    CycleModelBackward,
    ToggleReasoning,
    Quit,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Search,
    SearchNext,
    SearchPrev,
    ClearSearch,
    Command,
    ToggleTts,
    StopTts,
    ToggleFullscreen,
    TogglePermissionMode,
    GoToTop,
    GoToBottom,
}

macro_rules! map_action {
    ($key:expr, $($variant:ident),+) => {
        match $key {
            $(ActionKey::$variant => InputAction::$variant),+
        }
    };
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KeybindConfig {
    #[serde(default)]
    pub bindings: HashMap<String, ActionKey>,
}

static DEFAULT_BINDINGS: Lazy<HashMap<(KeyModifiers, KeyCode), InputAction>> =
    Lazy::new(default_bindings_internal);

fn default_bindings_internal() -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    let mut map = HashMap::new();
    map.insert((KeyModifiers::SHIFT, KeyCode::Enter), InputAction::Newline);
    map.insert((KeyModifiers::NONE, KeyCode::Enter), InputAction::Send);
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('c')),
        InputAction::Cancel,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Esc), InputAction::Cancel);
    map.insert((KeyModifiers::NONE, KeyCode::Up), InputAction::NavigateUp);
    map.insert(
        (KeyModifiers::NONE, KeyCode::Down),
        InputAction::NavigateDown,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('j')),
        InputAction::NavigateDown,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('k')),
        InputAction::NavigateUp,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Tab), InputAction::SwitchAgent);
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Tab),
        InputAction::TogglePermissionMode,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('l')),
        InputAction::SelectModel,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('k')),
        InputAction::ClearSession,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('n')),
        InputAction::NewSession,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('t')),
        InputAction::ToggleSidebar,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('w')),
        InputAction::CloseSession,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('/')),
        InputAction::FocusPrompt,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Char('?')), InputAction::Help);
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('s')),
        InputAction::StashPrompt,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('r')),
        InputAction::RestorePrompt,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('p')),
        InputAction::CycleModelForward,
    );
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyCode::Char('P'),
        ),
        InputAction::CycleModelBackward,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Backspace),
        InputAction::Backspace,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Delete), InputAction::Delete);
    map.insert((KeyModifiers::NONE, KeyCode::Left), InputAction::Left);
    map.insert((KeyModifiers::NONE, KeyCode::Right), InputAction::Right);
    map.insert((KeyModifiers::NONE, KeyCode::Home), InputAction::Home);
    map.insert((KeyModifiers::NONE, KeyCode::End), InputAction::End);
    map.insert((KeyModifiers::NONE, KeyCode::PageUp), InputAction::PageUp);
    map.insert(
        (KeyModifiers::NONE, KeyCode::PageDown),
        InputAction::PageDown,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('g')),
        InputAction::GoToTop,
    );
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Char('G')),
        InputAction::GoToBottom,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('d')),
        InputAction::PageDown,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('u')),
        InputAction::PageUp,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('q')),
        InputAction::Quit,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('t')),
        InputAction::ToggleSidebar,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('f')),
        InputAction::Search,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('e')),
        InputAction::ExternalEditor,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('y')),
        InputAction::ToggleTts,
    );
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyCode::Char('Y'),
        ),
        InputAction::StopTts,
    );
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyCode::Char('F'),
        ),
        InputAction::ToggleFullscreen,
    );
    map
}

pub fn default_bindings() -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    DEFAULT_BINDINGS.clone()
}

fn vim_bindings_internal() -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    let mut map = HashMap::new();

    map.insert((KeyModifiers::NONE, KeyCode::Enter), InputAction::Send);
    map.insert((KeyModifiers::SHIFT, KeyCode::Enter), InputAction::Newline);
    map.insert((KeyModifiers::NONE, KeyCode::Esc), InputAction::Cancel);
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('c')),
        InputAction::Cancel,
    );

    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('j')),
        InputAction::NavigateDown,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('k')),
        InputAction::NavigateUp,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Char('h')), InputAction::Left);
    map.insert((KeyModifiers::NONE, KeyCode::Char('l')), InputAction::Right);
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('i')),
        InputAction::FocusPrompt,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char(':')),
        InputAction::Command,
    );

    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('g')),
        InputAction::GoToTop,
    );
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Char('G')),
        InputAction::GoToBottom,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('d')),
        InputAction::PageDown,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('u')),
        InputAction::PageUp,
    );

    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('n')),
        InputAction::NewSession,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Char('q')), InputAction::Quit);
    map.insert((KeyModifiers::NONE, KeyCode::Char('?')), InputAction::Help);

    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('t')),
        InputAction::ToggleSidebar,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Tab), InputAction::SwitchAgent);
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Tab),
        InputAction::TogglePermissionMode,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('f')),
        InputAction::Search,
    );

    map.insert(
        (KeyModifiers::NONE, KeyCode::Backspace),
        InputAction::Backspace,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Delete), InputAction::Delete);
    map.insert((KeyModifiers::NONE, KeyCode::Up), InputAction::NavigateUp);
    map.insert(
        (KeyModifiers::NONE, KeyCode::Down),
        InputAction::NavigateDown,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Left), InputAction::Left);
    map.insert((KeyModifiers::NONE, KeyCode::Right), InputAction::Right);
    map.insert((KeyModifiers::NONE, KeyCode::Home), InputAction::Home);
    map.insert((KeyModifiers::NONE, KeyCode::End), InputAction::End);
    map.insert((KeyModifiers::NONE, KeyCode::PageUp), InputAction::PageUp);
    map.insert(
        (KeyModifiers::NONE, KeyCode::PageDown),
        InputAction::PageDown,
    );

    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyCode::Char('F'),
        ),
        InputAction::ToggleFullscreen,
    );

    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('d')),
        InputAction::OpenDiff,
    );

    map
}

static VIM_BINDINGS: Lazy<HashMap<(KeyModifiers, KeyCode), InputAction>> =
    Lazy::new(vim_bindings_internal);

pub fn vim_bindings() -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    VIM_BINDINGS.clone()
}

/// Returns all default help entries across all modes.
pub fn default_help_entries() -> Vec<HelpEntry> {
    vec![
        // Insert mode: only modifier-based shortcuts (bare chars insert text)
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Enter",
            action: "Send prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Shift+Enter",
            action: "New line in prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Esc",
            action: "Switch to normal mode",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+L",
            action: "Model selector",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+K",
            action: "Clear session",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+N",
            action: "New session",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+T",
            action: "Toggle sidebar",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+W",
            action: "Close session",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Tab",
            action: "Switch agent",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+S",
            action: "Stash prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+R",
            action: "Restore prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+P",
            action: "Cycle model forward",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Shift+P",
            action: "Cycle model backward",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Y",
            action: "Toggle TTS (speak)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Shift+Y",
            action: "Stop TTS",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Shift+F",
            action: "Toggle fullscreen",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+F",
            action: "Search",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Q",
            action: "Quit",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "PgUp/PgDn",
            action: "Scroll viewport",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "/",
            action: "Type / (slash command at prompt start)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "?",
            action: "Type ?",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "@",
            action: "File completions (at prompt start)",
            condition: None,
        },
        // Normal mode: bare navigation/action keys
        HelpEntry {
            mode: HelpMode::Normal,
            key: "j/k",
            action: "Navigate up/down",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "g/G",
            action: "Go to top/bottom",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "?",
            action: "Help",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "i",
            action: "Focus prompt (insert mode)",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: ":",
            action: "Command mode",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "n",
            action: "New session",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "q",
            action: "Quit",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "d",
            action: "Open diff",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "h/l",
            action: "Move cursor left/right",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Esc",
            action: "Cancel / exit",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+C",
            action: "Quit / clear input",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Tab",
            action: "Switch agent",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+L",
            action: "Model selector",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+T",
            action: "Toggle sidebar",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+S",
            action: "Stash prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+R",
            action: "Restore prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+P",
            action: "Cycle model forward",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+Shift+P",
            action: "Cycle model backward",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+F",
            action: "Search",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+D/U",
            action: "Page down/up",
            condition: None,
        },
        // Command mode
        HelpEntry {
            mode: HelpMode::Command,
            key: "/",
            action: "Start slash command",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "Tab",
            action: "Complete",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "Esc",
            action: "Cancel command",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "Up/Down",
            action: "History navigation",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "Enter",
            action: "Execute command",
            condition: None,
        },
        // Shell commands
        HelpEntry {
            mode: HelpMode::Command,
            key: "!cmd",
            action: "Run shell command",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "!!cmd",
            action: "Run and promote output",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/shell-list",
            action: "List recent shell commands",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/shell-show <id>",
            action: "Show shell command details",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/shell-include <id>",
            action: "Include output in context",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/shell-rerun <id>",
            action: "Rerun shell command",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/shell-kill <id>",
            action: "Kill running shell command",
            condition: None,
        },
        // Dialog mode (common across dialogs)
        HelpEntry {
            mode: HelpMode::Dialog,
            key: "Esc",
            action: "Close/cancel",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Dialog,
            key: "Enter",
            action: "Accept/confirm",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Dialog,
            key: "↑/↓ or j/k",
            action: "Navigate",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Dialog,
            key: "Tab",
            action: "Next field",
            condition: Some("some dialogs"),
        },
    ]
}

/// Returns help entries filtered by the given mode.
pub fn help_entries_for_mode(mode: HelpMode) -> Vec<HelpEntry> {
    default_help_entries()
        .into_iter()
        .filter(|e| e.mode == mode)
        .collect()
}

/// Build mode-aware help lines for display.
///
/// When `vim_mode` is true, normal-mode entries include vim-specific bindings.
/// The `active_mode` parameter determines which section is shown first/prominently.
pub fn build_help_lines(vim_mode: bool, active_mode: HelpMode) -> Vec<String> {
    let all_entries = default_help_entries();
    let mut lines = Vec::new();

    // Helper to format a section
    let mut add_section = |mode: HelpMode, header: &str| {
        let entries: Vec<&HelpEntry> = all_entries
            .iter()
            .filter(|e| e.mode == mode)
            .filter(|e| {
                // Filter out vim-specific entries when vim mode is off
                if !vim_mode {
                    if let Some(cond) = e.condition {
                        return cond != "vim mode";
                    }
                }
                true
            })
            .collect();

        if entries.is_empty() {
            return;
        }

        lines.push(header.to_string());
        lines.push("".to_string());
        for entry in &entries {
            let condition_note = match entry.condition {
                Some("vim mode") => " (vim mode)",
                Some(_) => "",
                None => "",
            };
            lines.push(format!(
                "  {:<16} {}{}",
                entry.key, entry.action, condition_note
            ));
        }
        lines.push("".to_string());
    };

    // Show active mode first, then others
    match active_mode {
        HelpMode::Insert => {
            add_section(HelpMode::Insert, "Insert Mode (text input)");
            add_section(HelpMode::Normal, "Normal Mode (Esc to enter)");
            add_section(HelpMode::Command, "Command Mode");
            add_section(HelpMode::Dialog, "Dialog Mode");
        }
        HelpMode::Normal => {
            add_section(HelpMode::Normal, "Normal Mode");
            add_section(HelpMode::Insert, "Insert Mode (text input)");
            add_section(HelpMode::Command, "Command Mode");
            add_section(HelpMode::Dialog, "Dialog Mode");
        }
        HelpMode::Command => {
            add_section(HelpMode::Command, "Command Mode");
            add_section(HelpMode::Insert, "Insert Mode (text input)");
            add_section(HelpMode::Normal, "Normal Mode");
            add_section(HelpMode::Dialog, "Dialog Mode");
        }
        HelpMode::Dialog => {
            add_section(HelpMode::Dialog, "Dialog Mode");
            add_section(HelpMode::Insert, "Insert Mode (text input)");
            add_section(HelpMode::Normal, "Normal Mode");
            add_section(HelpMode::Command, "Command Mode");
        }
    }

    // Remove trailing empty line
    if lines.last() == Some(&"".to_string()) {
        lines.pop();
    }

    lines
}

pub fn build_bindings(
    overrides: Option<&KeybindConfig>,
    vim_mode: bool,
) -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    let mut bindings = if vim_mode {
        vim_bindings()
    } else {
        default_bindings()
    };

    if let Some(cfg) = overrides {
        for (key_str, action) in &cfg.bindings {
            if let Some((mods, code)) = parse_key(key_str) {
                let action = map_action!(
                    action,
                    Send,
                    Newline,
                    Cancel,
                    NavigateUp,
                    NavigateDown,
                    SwitchAgent,
                    SelectModel,
                    ClearSession,
                    NewSession,
                    ToggleSidebar,
                    ToggleSection,
                    CloseSession,
                    Help,
                    FocusPrompt,
                    StashPrompt,
                    RestorePrompt,
                    CopyMessage,
                    CycleModelForward,
                    CycleModelBackward,
                    ToggleReasoning,
                    Quit,
                    Backspace,
                    Delete,
                    Left,
                    Right,
                    Home,
                    End,
                    PageUp,
                    PageDown,
                    Search,
                    SearchNext,
                    SearchPrev,
                    ClearSearch,
                    Command,
                    ToggleTts,
                    StopTts,
                    ToggleFullscreen,
                    TogglePermissionMode,
                    GoToTop,
                    GoToBottom
                );
                bindings.insert((mods, code), action);
            }
        }
    }
    bindings
}

fn parse_key(s: &str) -> Option<(KeyModifiers, KeyCode)> {
    let parts: Vec<&str> = s.split('+').collect();
    let mut mods = KeyModifiers::NONE;
    let code_str = parts.last()?;

    for part in &parts[..parts.len() - 1] {
        match *part {
            "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
            "shift" => mods |= KeyModifiers::SHIFT,
            "alt" => mods |= KeyModifiers::ALT,
            _ => {}
        }
    }

    let code = match *code_str {
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        c if c.len() == 1 => KeyCode::Char(c.chars().next().unwrap()),
        _ => return None,
    };
    Some((mods, code))
}

pub fn handle_event(event: Event) -> Option<InputAction> {
    handle_event_with_bindings_moded(event, None, InputMode::Insert)
}

pub fn handle_event_with_bindings(
    event: Event,
    bindings: Option<&HashMap<(KeyModifiers, KeyCode), InputAction>>,
) -> Option<InputAction> {
    handle_event_with_bindings_moded(event, bindings, InputMode::Insert)
}

pub fn handle_event_with_bindings_moded(
    event: Event,
    bindings: Option<&HashMap<(KeyModifiers, KeyCode), InputAction>>,
    mode: InputMode,
) -> Option<InputAction> {
    match event {
        Event::Key(key) => handle_key_with_bindings(key, bindings, mode),
        _ => None,
    }
}

fn is_printable_char(c: char) -> bool {
    !c.is_control()
}

fn is_text_modifier(modifiers: KeyModifiers) -> bool {
    modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT
}

fn handle_key_with_bindings(
    key: KeyEvent,
    bindings: Option<&HashMap<(KeyModifiers, KeyCode), InputAction>>,
    mode: InputMode,
) -> Option<InputAction> {
    let map = bindings.unwrap_or(&DEFAULT_BINDINGS);

    let key_tuple = (key.modifiers, key.code);
    debug_log!(
        "key event: modifiers={:?}, code={:?}, mode={:?}",
        key.modifiers,
        key.code,
        mode
    );

    match mode {
        InputMode::Insert => {
            // In Insert mode, bare character bindings (NONE + Char) are skipped
            // so users can type freely. Only modifier-key combos and special keys
            // trigger actions. Slash detection at position 0 is handled by on_char().
            let is_bare_char = matches!(key.code, KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE && is_printable_char(c));

            if let Some(action) = map.get(&key_tuple) {
                if is_bare_char {
                    debug_log!("insert mode: skipping bare char binding: {:?}", action);
                } else {
                    debug_log!("insert mode: matched binding: {:?}", action);
                    return Some(action.clone());
                }
            }
            if let Some(_user_bindings) = bindings {
                if let Some(action) = DEFAULT_BINDINGS.get(&key_tuple) {
                    if is_bare_char {
                        debug_log!("insert mode: skipping bare char default: {:?}", action);
                    } else {
                        debug_log!("insert mode: fallback to default: {:?}", action);
                        return Some(action.clone());
                    }
                }
            }
            // No binding found - check if this is a printable char with only NONE or SHIFT modifier
            if let KeyCode::Char(c) = key.code {
                if is_printable_char(c) && is_text_modifier(key.modifiers) {
                    debug_log!("insert mode: passing char as Char action: {:?}", c);
                    return Some(InputAction::Char(c));
                }
            }
            debug_log!("insert mode: key not handled");
            None
        }
        InputMode::Normal => {
            if let Some(action) = map.get(&key_tuple) {
                debug_log!("normal mode: matched binding: {:?}", action);
                return Some(action.clone());
            }
            if let Some(_user_bindings) = bindings {
                if let Some(action) = DEFAULT_BINDINGS.get(&key_tuple) {
                    debug_log!("normal mode: fallback to default: {:?}", action);
                    return Some(action.clone());
                }
            }
            debug_log!("normal mode: key not handled");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn test_shift_char_insert_mode() {
        // Shift + 'A' should insert 'A' in insert mode
        let key = make_key(KeyCode::Char('A'), KeyModifiers::SHIFT);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('A')));
    }

    #[test]
    fn test_shift_punctuation_insert_mode() {
        // Shift + '!' should insert '!' in insert mode
        let key = make_key(KeyCode::Char('!'), KeyModifiers::SHIFT);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('!')));

        // Shift + '?' should insert '?'
        let key = make_key(KeyCode::Char('?'), KeyModifiers::SHIFT);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('?')));

        // Shift + '_' should insert '_'
        let key = make_key(KeyCode::Char('_'), KeyModifiers::SHIFT);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('_')));
    }

    #[test]
    fn test_ctrl_p_returns_cycle_model_forward() {
        let key = make_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::CycleModelForward));
    }

    #[test]
    fn test_ctrl_shift_p_returns_cycle_model_backward() {
        let key = make_key(
            KeyCode::Char('P'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::CycleModelBackward));
    }

    #[test]
    fn test_shift_enter_returns_newline() {
        let key = make_key(KeyCode::Enter, KeyModifiers::SHIFT);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Newline));
    }

    #[test]
    fn test_shift_tab_returns_toggle_permission_mode() {
        let key = make_key(KeyCode::Tab, KeyModifiers::SHIFT);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::TogglePermissionMode));
    }

    #[test]
    fn test_custom_shift_binding_wins_over_char() {
        use std::collections::HashMap;
        let mut custom_bindings = HashMap::new();
        custom_bindings.insert((KeyModifiers::SHIFT, KeyCode::Char('A')), InputAction::Help);
        let key = make_key(KeyCode::Char('A'), KeyModifiers::SHIFT);
        let result = handle_key_with_bindings(key, Some(&custom_bindings), InputMode::Insert);
        assert_eq!(result, Some(InputAction::Help));
    }

    #[test]
    fn test_bare_slash_inserts_char_in_insert_mode() {
        // Bare '/' should insert '/' in insert mode (not trigger FocusPrompt)
        let key = make_key(KeyCode::Char('/'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('/')));
    }

    #[test]
    fn test_bare_question_inserts_char_in_insert_mode() {
        // Bare '?' should insert '?' in insert mode (not trigger Help)
        let key = make_key(KeyCode::Char('?'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('?')));
    }

    #[test]
    fn test_bare_g_inserts_char_in_insert_mode() {
        // Bare 'g' should insert 'g' in insert mode (not trigger GoToTop)
        let key = make_key(KeyCode::Char('g'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('g')));
    }

    #[test]
    fn test_bare_j_inserts_char_in_insert_mode() {
        // Bare 'j' should insert 'j' in insert mode (not trigger NavigateDown)
        let key = make_key(KeyCode::Char('j'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('j')));
    }

    #[test]
    fn test_bare_k_inserts_char_in_insert_mode() {
        // Bare 'k' should insert 'k' in insert mode (not trigger NavigateUp)
        let key = make_key(KeyCode::Char('k'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('k')));
    }

    #[test]
    fn test_bare_chars_trigger_in_normal_mode() {
        // In normal mode, bare chars should trigger their bound actions
        let key = make_key(KeyCode::Char('g'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Normal);
        assert_eq!(result, Some(InputAction::GoToTop));

        let key = make_key(KeyCode::Char('?'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Normal);
        assert_eq!(result, Some(InputAction::Help));

        let key = make_key(KeyCode::Char('j'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Normal);
        assert_eq!(result, Some(InputAction::NavigateDown));

        let key = make_key(KeyCode::Char('k'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Normal);
        assert_eq!(result, Some(InputAction::NavigateUp));
    }

    #[test]
    fn test_ctrl_combo_still_works_in_insert_mode() {
        // Ctrl combinations should still trigger actions in insert mode
        let key = make_key(KeyCode::Char('l'), KeyModifiers::CONTROL);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::SelectModel));
    }

    #[test]
    fn test_custom_bare_char_binding_ignored_in_insert_mode() {
        // Custom bindings for bare chars should be ignored in insert mode
        use std::collections::HashMap;
        let mut custom = HashMap::new();
        custom.insert((KeyModifiers::NONE, KeyCode::Char('x')), InputAction::Help);
        let key = make_key(KeyCode::Char('x'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, Some(&custom), InputMode::Insert);
        assert_eq!(result, Some(InputAction::Char('x')));
    }

    #[test]
    fn test_custom_bare_char_binding_works_in_normal_mode() {
        // Custom bindings for bare chars should work in normal mode
        use std::collections::HashMap;
        let mut custom = HashMap::new();
        custom.insert((KeyModifiers::NONE, KeyCode::Char('x')), InputAction::Help);
        let key = make_key(KeyCode::Char('x'), KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, Some(&custom), InputMode::Normal);
        assert_eq!(result, Some(InputAction::Help));
    }

    #[test]
    fn test_ctrl_l_returns_select_model_in_insert_mode() {
        let key = make_key(KeyCode::Char('l'), KeyModifiers::CONTROL);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::SelectModel));
    }

    #[test]
    fn test_ctrl_t_returns_toggle_sidebar_in_insert_mode() {
        let key = make_key(KeyCode::Char('t'), KeyModifiers::CONTROL);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::ToggleSidebar));
    }

    #[test]
    fn test_esc_returns_cancel_in_insert_mode() {
        let key = make_key(KeyCode::Esc, KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Cancel));
    }

    // --- Help builder tests (Phase 5) ---

    #[test]
    fn test_help_builder_includes_insert_mode_modifier_shortcuts() {
        let entries = help_entries_for_mode(HelpMode::Insert);
        let keys: Vec<&str> = entries.iter().map(|e| e.key).collect();
        assert!(keys.contains(&"Ctrl+L"), "missing Ctrl+L in insert help");
        assert!(keys.contains(&"Ctrl+T"), "missing Ctrl+T in insert help");
        assert!(keys.contains(&"Ctrl+P"), "missing Ctrl+P in insert help");
        assert!(keys.contains(&"Ctrl+S"), "missing Ctrl+S in insert help");
        assert!(keys.contains(&"Ctrl+R"), "missing Ctrl+R in insert help");
        assert!(keys.contains(&"Ctrl+Y"), "missing Ctrl+Y in insert help");
        assert!(
            keys.contains(&"Ctrl+Shift+F"),
            "missing Ctrl+Shift+F in insert help"
        );
    }

    #[test]
    fn test_help_builder_qualifies_bare_printable_keys_by_mode() {
        // In insert mode, bare '?' should say "Type ?" not "Help"
        let entries = help_entries_for_mode(HelpMode::Insert);
        let q_entry = entries.iter().find(|e| e.key == "?");
        assert!(q_entry.is_some(), "insert mode should document '?'");
        assert!(
            q_entry.unwrap().action.contains("Type"),
            "insert mode '?' should say 'Type ?', got: {}",
            q_entry.unwrap().action
        );

        // In normal mode, bare '?' should say "Help"
        let entries = help_entries_for_mode(HelpMode::Normal);
        let q_entry = entries.iter().find(|e| e.key == "?");
        assert!(q_entry.is_some(), "normal mode should document '?'");
        assert_eq!(q_entry.unwrap().action, "Help");

        // In insert mode, bare '/' should say "Type /" not "Focus prompt"
        let entries = help_entries_for_mode(HelpMode::Insert);
        let slash_entry = entries.iter().find(|e| e.key == "/");
        assert!(slash_entry.is_some(), "insert mode should document '/'");
        assert!(
            slash_entry.unwrap().action.contains("Type"),
            "insert mode '/' should say 'Type /', got: {}",
            slash_entry.unwrap().action
        );
    }

    #[test]
    fn test_build_help_lines_insert_mode_shows_insert_first() {
        let lines = build_help_lines(false, HelpMode::Insert);
        // First non-empty content line should be in the Insert Mode section
        let insert_header = lines.iter().position(|l| l.contains("Insert Mode"));
        let normal_header = lines.iter().position(|l| l.contains("Normal Mode"));
        assert!(insert_header.is_some(), "should have Insert Mode section");
        assert!(normal_header.is_some(), "should have Normal Mode section");
        assert!(
            insert_header.unwrap() < normal_header.unwrap(),
            "Insert Mode section should come before Normal Mode"
        );
    }

    #[test]
    fn test_build_help_lines_normal_mode_shows_normal_first() {
        let lines = build_help_lines(false, HelpMode::Normal);
        let insert_header = lines.iter().position(|l| l.contains("Insert Mode"));
        let normal_header = lines.iter().position(|l| l.contains("Normal Mode"));
        assert!(insert_header.is_some());
        assert!(normal_header.is_some());
        assert!(
            normal_header.unwrap() < insert_header.unwrap(),
            "Normal Mode section should come before Insert Mode"
        );
    }

    #[test]
    fn test_build_help_lines_vim_mode_includes_vim_keys() {
        let lines = build_help_lines(true, HelpMode::Normal);
        let text = lines.join("\n");
        assert!(
            text.contains("vim mode"),
            "vim help should mention vim mode"
        );
        assert!(
            text.contains("i"),
            "vim help should include 'i' for focus prompt"
        );
        assert!(
            text.contains(":"),
            "vim help should include ':' for command mode"
        );
    }

    #[test]
    fn test_build_help_lines_no_vim_hides_vim_only_keys() {
        let lines = build_help_lines(false, HelpMode::Normal);
        let text = lines.join("\n");
        // When vim mode is off, vim-specific entries like "i" (focus prompt)
        // and ":" (command mode) should not appear with "vim mode" condition
        let normal_section_start = text.find("Normal Mode").unwrap();
        let normal_section = &text[normal_section_start..];
        // "i" as a standalone key in the normal section should not have "(vim mode)" suffix
        // when vim mode is disabled
        assert!(
            !normal_section.contains("(vim mode)"),
            "non-vim help should not show vim mode conditions"
        );
    }

    #[test]
    fn test_build_help_lines_command_mode_section() {
        let lines = build_help_lines(false, HelpMode::Command);
        let text = lines.join("\n");
        assert!(
            text.contains("Command Mode"),
            "should have Command Mode section"
        );
        assert!(text.contains("Start slash command"));
        assert!(text.contains("Complete"));
    }

    #[test]
    fn test_build_help_lines_dialog_mode_section() {
        let lines = build_help_lines(false, HelpMode::Dialog);
        let text = lines.join("\n");
        assert!(
            text.contains("Dialog Mode"),
            "should have Dialog Mode section"
        );
        assert!(text.contains("Close/cancel"));
        assert!(text.contains("Accept/confirm"));
    }

    #[test]
    fn test_help_entries_for_mode_filters_correctly() {
        let insert_entries = help_entries_for_mode(HelpMode::Insert);
        assert!(insert_entries.iter().all(|e| e.mode == HelpMode::Insert));

        let normal_entries = help_entries_for_mode(HelpMode::Normal);
        assert!(normal_entries.iter().all(|e| e.mode == HelpMode::Normal));

        let command_entries = help_entries_for_mode(HelpMode::Command);
        assert!(command_entries.iter().all(|e| e.mode == HelpMode::Command));

        let dialog_entries = help_entries_for_mode(HelpMode::Dialog);
        assert!(dialog_entries.iter().all(|e| e.mode == HelpMode::Dialog));
    }
}

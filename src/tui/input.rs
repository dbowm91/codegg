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
use std::fs::OpenOptions;

#[cfg(feature = "debug-logging")]
use std::io::Write;
#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("codegg_debug.log") {
            let _ = writeln!(file, "[INPUT-DEBUG] {}", format!($($arg)*));
        }
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
    map.insert((KeyModifiers::NONE, KeyCode::Char('g')), InputAction::GoToTop);
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

    map.insert((KeyModifiers::NONE, KeyCode::Char('g')), InputAction::GoToTop);
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
}

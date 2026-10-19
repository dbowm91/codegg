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
//! | Tab | ToggleComposerMode (Session/Task) |
//! | Ctrl+A | SwitchAgent |
//! | Shift+Tab | TogglePermissionMode |
//! | Ctrl+L | SelectModel |
//! | Ctrl+K | ClearSession |
//! | n | NewSession (Normal only) |
//! | Ctrl+N | Toggle panel resize mode |
//! | Ctrl+T | ToggleTasksList (todos + subagents + jobs) |
//! | Shift+N / Shift+P | Next / previous subagent tab (main buffer) |
//! | Shift+X | Close subagent tab (main buffer) |
//! | Ctrl+B | ToggleSidebar |
//! | Ctrl+W | CloseSession |
//! | Ctrl+S | StashPrompt |
//! | Ctrl+R | RestorePrompt |
//! | Ctrl+P / Ctrl+Alt+P | CycleModelForward/Backward |
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
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Stop the running turn, or resume a stopped one (`x`, Normal mode
    /// only).
    ///
    /// One key for both directions because the state already says which one
    /// applies: a stopped agent resumes, a working one stops. Stopping is the
    /// destructive direction, so it takes a confirmed second press inside a
    /// short window — see `turn_control::toggle_stop_or_resume`.
    ToggleAgentRun,
    ToggleSidebar,
    FocusSidebar,
    /// Open the side-panel manager (`Shift+S`), Normal mode only.
    ///
    /// The manager used to have a second key, `Ctrl+Alt+W`, so it could be
    /// reached mid-sentence. That chord is retired: the manager is a
    /// Normal-mode surface exactly like the directionals that drive the same
    /// panels, so one key per surface is both consistent and one less chord
    /// to decode in a legacy terminal. It is answered *before* a focused
    /// panel sees the key, so switching panels is reachable from inside a
    /// panel rather than only from the main buffer.
    OpenPanelManager,
    /// Directional pane gestures, Normal mode only.
    ///
    /// `Shift+Left/Right/Up/Down` and the vim `Shift+H/J/K/L` twins all
    /// resolve here, and every one of them routes through the *same*
    /// contextual dispatcher ([`crate::tui::commands::panels::move_directionally`])
    /// rather than carrying its own meaning:
    ///
    /// * a dockable box is open → the box is **moved** into the side panel
    ///   and the box goes away. The direction names the panel's side, which
    ///   is right today and left when the layout grows a second column;
    /// * no box is open → the direction **selects a pane**: `Right` takes
    ///   the keyboard into the side panel, `Left` returns it to the main
    ///   buffer, and `Up`/`Down` cycle which panel type is loaded.
    ///
    /// One gesture family with two meanings is the multiplexer contract: the
    /// key says *which way*, the surface underneath says *what is there*.
    /// Resolving it in one place is what keeps a box and its panel from
    /// disagreeing about what `L` does.
    DirectionLeft,
    DirectionRight,
    DirectionUp,
    DirectionDown,
    /// Expand the unified tasks overlay: todos, then in-flight
    /// subagents, then jobs the session is waiting on.
    ///
    /// Distinct from the `/tasks` command, which governs *schedules*.
    /// That overlap is deliberate but the help text must not call a
    /// scheduled run a "task" beside a live subagent.
    ToggleTasksList,
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
    /// Explicit, unconditional exit (Ctrl+C).
    ///
    /// Separate from [`InputAction::Cancel`] because `Esc` must never quit:
    /// it is the reflex key for "get me out of whatever is on screen", and
    /// binding it to exit meant an extra keypress in a dialog — or a stray
    /// `Esc` after a turn finished — tore down the whole session.
    ForceQuit,
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
    OpenProjectPicker,
    NextProjectTab,
    PreviousProjectTab,
    CloseProjectTab,
    /// Move to the next subagent tab in the main buffer (`Shift+N`).
    ///
    /// Cycle order runs `Chat` → subagent tabs in the order they were opened,
    /// and wraps. A no-op with no subagent tabs open: the main conversation
    /// stays selected rather than the key becoming visibly dead.
    NextSubagentTab,
    /// Move to the previous subagent tab (`Shift+P`). Mirrors
    /// [`InputAction::NextSubagentTab`] in both directions and wrapping.
    PreviousSubagentTab,
    /// Close the active subagent tab (`Shift+X`), returning to the main
    /// conversation. Never closes the main buffer itself, and cancels
    /// nothing — the run keeps going whether or not its tab is open.
    CloseSubagentTab,
    /// Project Work Orders M003 + C001: toggle the prompt composer between
    /// Session and Task submission modes. C001 owns bare Tab at root
    /// prompt focus (`Session <-> Task`); `SwitchAgent` lives on `Ctrl+A`
    /// (portable, distinct from Tab/0x09) and remains configurable.
    /// `Ctrl+G` stays a backward-compatible alias. Modal-local Tab focus
    /// consumes Tab before root routing, so sheets/dialogs never toggle
    /// the composer underneath.
    ToggleComposerMode,
    /// Project Work Orders M004: open the global Workspace dashboard
    /// (bounded per-project activity rows). The dashboard is an
    /// overlay: entering preserves the active session/tab and `Esc`
    /// returns without reloading unrelated state.
    OpenWorkspaceDashboard,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
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
    /// Stop the running turn, or resume a stopped one (`x`, Normal mode
    /// only).
    ///
    /// One key for both directions because the state already says which one
    /// applies: a stopped agent resumes, a working one stops. Stopping is the
    /// destructive direction, so it takes a confirmed second press.
    ToggleAgentRun,
    ToggleSidebar,
    FocusSidebar,
    /// Open the side-panel manager (`Shift+S`, Normal mode only).
    OpenPanelManager,
    /// Directional pane gestures. See
    /// [`InputAction::DirectionLeft`] — all four share one contextual
    /// dispatcher.
    DirectionLeft,
    DirectionRight,
    DirectionUp,
    DirectionDown,
    /// Expand the unified tasks overlay: todos, then in-flight
    /// subagents, then jobs the session is waiting on.
    ///
    /// Distinct from the `/tasks` command, which governs *schedules*.
    /// That overlap is deliberate but the help text must not call a
    /// scheduled run a "task" beside a live subagent.
    ToggleTasksList,
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
    ForceQuit,
    ExternalEditor,
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
    OpenProjectPicker,
    NextProjectTab,
    PreviousProjectTab,
    CloseProjectTab,
    NextSubagentTab,
    PreviousSubagentTab,
    CloseSubagentTab,
    ToggleComposerMode,
    OpenWorkspaceDashboard,
}

/// Declarative metadata shared by the keybinding editor and discovery/help
/// surfaces.  `ActionKey::all()` is the exhaustive configurable-action list;
/// `InputAction::Char` remains intentionally non-configurable because it is
/// ordinary prompt text rather than an application action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDescriptor {
    pub key: ActionKey,
    pub label: &'static str,
    pub description: &'static str,
}

impl ActionKey {
    pub const fn all() -> &'static [Self] {
        &[
            Self::Send,
            Self::Newline,
            Self::Cancel,
            Self::NavigateUp,
            Self::NavigateDown,
            Self::SwitchAgent,
            Self::SelectModel,
            Self::ClearSession,
            Self::NewSession,
            Self::ToggleAgentRun,
            Self::ToggleSidebar,
            Self::FocusSidebar,
            Self::ToggleSection,
            Self::CloseSession,
            Self::Help,
            Self::FocusPrompt,
            Self::StashPrompt,
            Self::RestorePrompt,
            Self::CopyMessage,
            Self::CycleModelForward,
            Self::CycleModelBackward,
            Self::ToggleReasoning,
            Self::Quit,
            Self::ForceQuit,
            Self::ExternalEditor,
            Self::Backspace,
            Self::Delete,
            Self::Left,
            Self::Right,
            Self::Home,
            Self::End,
            Self::PageUp,
            Self::PageDown,
            Self::Search,
            Self::SearchNext,
            Self::SearchPrev,
            Self::ClearSearch,
            Self::Command,
            Self::ToggleTts,
            Self::StopTts,
            Self::ToggleFullscreen,
            Self::TogglePermissionMode,
            Self::OpenDiff,
            Self::GoToTop,
            Self::GoToBottom,
            Self::OpenProjectPicker,
            Self::NextProjectTab,
            Self::PreviousProjectTab,
            Self::CloseProjectTab,
            Self::NextSubagentTab,
            Self::PreviousSubagentTab,
            Self::CloseSubagentTab,
            Self::ToggleComposerMode,
            Self::OpenWorkspaceDashboard,
            Self::ToggleTasksList,
        ]
    }

    pub const fn to_input_action(self) -> InputAction {
        match self {
            Self::Send => InputAction::Send,
            Self::Newline => InputAction::Newline,
            Self::Cancel => InputAction::Cancel,
            Self::NavigateUp => InputAction::NavigateUp,
            Self::NavigateDown => InputAction::NavigateDown,
            Self::SwitchAgent => InputAction::SwitchAgent,
            Self::SelectModel => InputAction::SelectModel,
            Self::ClearSession => InputAction::ClearSession,
            Self::NewSession => InputAction::NewSession,
            Self::ToggleAgentRun => InputAction::ToggleAgentRun,
            Self::ToggleSidebar => InputAction::ToggleSidebar,
            Self::FocusSidebar => InputAction::FocusSidebar,
            Self::OpenPanelManager => InputAction::OpenPanelManager,
            Self::DirectionLeft => InputAction::DirectionLeft,
            Self::DirectionRight => InputAction::DirectionRight,
            Self::DirectionUp => InputAction::DirectionUp,
            Self::DirectionDown => InputAction::DirectionDown,
            Self::ToggleSection => InputAction::ToggleSection,
            Self::CloseSession => InputAction::CloseSession,
            Self::Help => InputAction::Help,
            Self::FocusPrompt => InputAction::FocusPrompt,
            Self::StashPrompt => InputAction::StashPrompt,
            Self::RestorePrompt => InputAction::RestorePrompt,
            Self::CopyMessage => InputAction::CopyMessage,
            Self::CycleModelForward => InputAction::CycleModelForward,
            Self::CycleModelBackward => InputAction::CycleModelBackward,
            Self::ToggleReasoning => InputAction::ToggleReasoning,
            Self::Quit => InputAction::Quit,
            Self::ForceQuit => InputAction::ForceQuit,
            Self::ExternalEditor => InputAction::ExternalEditor,
            Self::Backspace => InputAction::Backspace,
            Self::Delete => InputAction::Delete,
            Self::Left => InputAction::Left,
            Self::Right => InputAction::Right,
            Self::Home => InputAction::Home,
            Self::End => InputAction::End,
            Self::PageUp => InputAction::PageUp,
            Self::PageDown => InputAction::PageDown,
            Self::Search => InputAction::Search,
            Self::SearchNext => InputAction::SearchNext,
            Self::SearchPrev => InputAction::SearchPrev,
            Self::ClearSearch => InputAction::ClearSearch,
            Self::Command => InputAction::Command,
            Self::ToggleTts => InputAction::ToggleTts,
            Self::StopTts => InputAction::StopTts,
            Self::ToggleFullscreen => InputAction::ToggleFullscreen,
            Self::TogglePermissionMode => InputAction::TogglePermissionMode,
            Self::OpenDiff => InputAction::OpenDiff,
            Self::GoToTop => InputAction::GoToTop,
            Self::GoToBottom => InputAction::GoToBottom,
            Self::OpenProjectPicker => InputAction::OpenProjectPicker,
            Self::NextProjectTab => InputAction::NextProjectTab,
            Self::PreviousProjectTab => InputAction::PreviousProjectTab,
            Self::CloseProjectTab => InputAction::CloseProjectTab,
            Self::NextSubagentTab => InputAction::NextSubagentTab,
            Self::PreviousSubagentTab => InputAction::PreviousSubagentTab,
            Self::CloseSubagentTab => InputAction::CloseSubagentTab,
            Self::ToggleComposerMode => InputAction::ToggleComposerMode,
            Self::OpenWorkspaceDashboard => InputAction::OpenWorkspaceDashboard,
            Self::ToggleTasksList => InputAction::ToggleTasksList,
        }
    }

    /// Canonical [`ActionKey`] for a resolved [`InputAction`], or `None` for
    /// an action that is not addressable as a binding (a typed character).
    ///
    /// This is the single place the two enums are reconciled. The help table
    /// advertises prose and the binding table stores actions, so without one
    /// shared naming authority the two drift apart silently — which is exactly
    /// how a live `Ctrl+T` row kept advertising "Toggle sidebar" for two days
    /// behind a green suite: the old guard only asserted that the key resolved
    /// to *something*. `documented_keys_resolve_in_the_active_map` now resolves
    /// an advertised key through the real keymaps and compares the advertised
    /// prose against [`ActionKey::label`] for whatever came back.
    pub fn from_input_action(action: &InputAction) -> Option<Self> {
        let candidate = match action {
            InputAction::Char(_) => return None,
            InputAction::Send => Self::Send,
            InputAction::Newline => Self::Newline,
            InputAction::Cancel => Self::Cancel,
            InputAction::NavigateUp => Self::NavigateUp,
            InputAction::NavigateDown => Self::NavigateDown,
            InputAction::SwitchAgent => Self::SwitchAgent,
            InputAction::SelectModel => Self::SelectModel,
            InputAction::ClearSession => Self::ClearSession,
            InputAction::NewSession => Self::NewSession,
            InputAction::ToggleAgentRun => Self::ToggleAgentRun,
            InputAction::ToggleSidebar => Self::ToggleSidebar,
            InputAction::FocusSidebar => Self::FocusSidebar,
            InputAction::OpenPanelManager => Self::OpenPanelManager,
            InputAction::DirectionLeft => Self::DirectionLeft,
            InputAction::DirectionRight => Self::DirectionRight,
            InputAction::DirectionUp => Self::DirectionUp,
            InputAction::DirectionDown => Self::DirectionDown,
            InputAction::ToggleTasksList => Self::ToggleTasksList,
            InputAction::ToggleSection => Self::ToggleSection,
            InputAction::CloseSession => Self::CloseSession,
            InputAction::Help => Self::Help,
            InputAction::FocusPrompt => Self::FocusPrompt,
            InputAction::StashPrompt => Self::StashPrompt,
            InputAction::RestorePrompt => Self::RestorePrompt,
            InputAction::CopyMessage => Self::CopyMessage,
            InputAction::CycleModelForward => Self::CycleModelForward,
            InputAction::CycleModelBackward => Self::CycleModelBackward,
            InputAction::ToggleReasoning => Self::ToggleReasoning,
            InputAction::Quit => Self::Quit,
            InputAction::ForceQuit => Self::ForceQuit,
            InputAction::ExternalEditor => Self::ExternalEditor,
            InputAction::Backspace => Self::Backspace,
            InputAction::Delete => Self::Delete,
            InputAction::Left => Self::Left,
            InputAction::Right => Self::Right,
            InputAction::Home => Self::Home,
            InputAction::End => Self::End,
            InputAction::PageUp => Self::PageUp,
            InputAction::PageDown => Self::PageDown,
            InputAction::Search => Self::Search,
            InputAction::SearchNext => Self::SearchNext,
            InputAction::SearchPrev => Self::SearchPrev,
            InputAction::ClearSearch => Self::ClearSearch,
            InputAction::Command => Self::Command,
            InputAction::ToggleTts => Self::ToggleTts,
            InputAction::StopTts => Self::StopTts,
            InputAction::ToggleFullscreen => Self::ToggleFullscreen,
            InputAction::TogglePermissionMode => Self::TogglePermissionMode,
            InputAction::OpenDiff => Self::OpenDiff,
            InputAction::GoToTop => Self::GoToTop,
            InputAction::GoToBottom => Self::GoToBottom,
            InputAction::OpenProjectPicker => Self::OpenProjectPicker,
            InputAction::NextProjectTab => Self::NextProjectTab,
            InputAction::PreviousProjectTab => Self::PreviousProjectTab,
            InputAction::CloseProjectTab => Self::CloseProjectTab,
            InputAction::NextSubagentTab => Self::NextSubagentTab,
            InputAction::PreviousSubagentTab => Self::PreviousSubagentTab,
            InputAction::CloseSubagentTab => Self::CloseSubagentTab,
            InputAction::ToggleComposerMode => Self::ToggleComposerMode,
            InputAction::OpenWorkspaceDashboard => Self::OpenWorkspaceDashboard,
        };
        Some(candidate)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Send => "Send",
            Self::Newline => "Newline",
            Self::Cancel => "Cancel",
            Self::NavigateUp => "Navigate up",
            Self::NavigateDown => "Navigate down",
            Self::SwitchAgent => "Switch agent",
            Self::SelectModel => "Select model",
            Self::ClearSession => "Clear session",
            Self::NewSession => "New session",
            Self::ToggleAgentRun => "Stop / resume agent (Ctrl+Alt+X)",
            Self::ToggleSidebar => "Toggle sidebar",
            Self::FocusSidebar => "Focus sidebar",
            Self::OpenPanelManager => "Open panel manager (Shift+S)",
            Self::DirectionLeft => "Pane left",
            Self::DirectionRight => "Pane right",
            Self::DirectionUp => "Previous panel",
            Self::DirectionDown => "Next panel",
            Self::ToggleSection => "Toggle section",
            Self::CloseSession => "Close session",
            Self::Help => "Show help",
            Self::FocusPrompt => "Focus prompt (Ctrl+Alt+E)",
            Self::StashPrompt => "Stash prompt",
            Self::RestorePrompt => "Restore prompt",
            Self::CopyMessage => "Copy message",
            Self::CycleModelForward => "Cycle model forward",
            Self::CycleModelBackward => "Cycle model backward",
            Self::ToggleReasoning => "Toggle reasoning",
            Self::Quit => "Quit",
            Self::ForceQuit => "Force quit",
            Self::ExternalEditor => "External editor",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Left => "Move left",
            Self::Right => "Move right",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "Page up",
            Self::PageDown => "Page down",
            Self::Search => "Search",
            Self::SearchNext => "Search next",
            Self::SearchPrev => "Search previous",
            Self::ClearSearch => "Clear search",
            Self::Command => "Command mode",
            Self::ToggleTts => "Toggle TTS",
            Self::StopTts => "Stop TTS",
            Self::ToggleFullscreen => "Toggle fullscreen",
            Self::TogglePermissionMode => "Toggle permission mode",
            Self::OpenDiff => "Open diff (Ctrl+Alt+D)",
            Self::GoToTop => "Go to top",
            Self::GoToBottom => "Go to bottom",
            Self::OpenProjectPicker => "Open project picker (Shift+F)",
            Self::NextProjectTab => "Next project tab",
            Self::PreviousProjectTab => "Previous project tab",
            Self::CloseProjectTab => "Close project tab",
            Self::NextSubagentTab => "Next subagent tab (Shift+N)",
            Self::PreviousSubagentTab => "Previous subagent tab (Shift+P)",
            Self::CloseSubagentTab => "Close subagent tab (Shift+X)",
            Self::ToggleComposerMode => "Toggle composer mode (Session/Task)",
            Self::OpenWorkspaceDashboard => "Open Workspace view (selected-project chat)",
            Self::ToggleTasksList => "Toggle tasks (todos, subagents, jobs) (Ctrl+T)",
        }
    }

    pub const fn descriptor(self) -> ActionDescriptor {
        ActionDescriptor {
            key: self,
            label: self.label(),
            description: self.label(),
        }
    }

    pub fn descriptors() -> Vec<ActionDescriptor> {
        Self::all().iter().copied().map(Self::descriptor).collect()
    }
}

impl InputAction {
    pub fn is_configurable(&self) -> bool {
        !matches!(self, Self::Char(_))
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KeybindConfig {
    #[serde(default)]
    pub bindings: HashMap<String, ActionKey>,
}

static DEFAULT_BINDINGS: Lazy<HashMap<(KeyModifiers, KeyCode), InputAction>> =
    Lazy::new(default_bindings_internal);

/// Actions that exist only in Normal mode.
///
/// The binding map is shared across modes, so a `Shift`-qualified entry would
/// otherwise claim the key in Insert mode too — stealing the caret movement
/// that `Shift+Left`/`Shift+Right` performs while composing and stealing the
/// letter itself for `Shift+H`/`Shift+L`. `handle_key_with_bindings` consults
/// this so those keys stay ordinary text/caret input in Insert.
///
/// **An action whose only spelling is a `Ctrl` chord must not be listed here.**
/// Such a chord cannot be composed as text, so it is already unambiguous in
/// Insert; listing it would take the action out of the mode the app boots in.
/// `ToggleTasksList` is the live example: it was gated here solely because
/// `Shift+T` was one of its spellings, and `Ctrl+T` — its only spelling now —
/// is why the entry is gone rather than the bug being documented.
pub(crate) fn is_normal_only(action: &InputAction) -> bool {
    matches!(
        action,
        InputAction::DirectionLeft
            | InputAction::DirectionRight
            | InputAction::DirectionUp
            | InputAction::DirectionDown
            // `Shift+S` is a letter the user is typing most of the time,
            // so the manager must not claim it mid-word. This used to be a
            // two-key arrangement with a chord that worked in Insert; the
            // chord is retired and this is now the only spelling, which
            // makes the mode gate the whole reason the key is safe.
            // (`Shift+F` and `Shift+W` are deliberately **not** listed here.
            // Both actions also own a `Ctrl` chord — `Ctrl+\` for the picker,
            // `Ctrl+O` for Workspace — and `is_normal_only` is keyed on the
            // action, so listing them here would have silently taken the
            // chord out of Insert mode too. Built-in shifted capitals are
            // handled in the Insert branch instead, at the key.)
            | InputAction::OpenPanelManager
            // `Shift+N` / `Shift+P` / `Shift+X` claim three more uppercase
            // letters, and `N` and `X` are among the most common capitals in
            // ordinary prose. Without these entries Insert mode would read a
            // capital `N` mid-word as "next subagent tab" instead of composing
            // it — the same class of bug the `Shift+S` and `Shift+T` entries
            // above prevent.
            | InputAction::NextSubagentTab
            | InputAction::PreviousSubagentTab
            | InputAction::CloseSubagentTab // `ToggleAgentRun` used to be listed here because its only
                                            // spelling was the bare letter `x`, and a bare letter is skipped
                                            // in Insert mode by the `is_bare_char` rule above — the listing
                                            // was belt-and-braces for a key the user types constantly. The
                                            // composer now claims `x` (ADR-0042) and the action's only
                                            // spelling is the `Ctrl+Alt+X` chord, so the entry is gone rather
                                            // than the bug being documented: a chord cannot be composed as
                                            // text, so it is already unambiguous in Insert mode.
    )
}

/// `Shift`-qualified pane directions: `←/→/↑/↓` and the vim `H/J/K/L` twins.
///
/// Shift-qualified on purpose. The bare arrows already scroll the transcript
/// (`Up`/`Down`) and move the caret (`Left`/`Right`), and the bare letters
/// `j`/`k` are bound to the same navigation; claiming any of them would take
/// away an existing verb. Shift narrows the gesture to "I am moving between
/// surfaces", which is a different intent from "I am moving within one".
///
/// The vertical pair used to be deliberately left unbound, reserved for a
/// future stacked-pane layout. It is bound now because it is no longer inert:
/// with a two-pane layout, `↑`/`↓` cycle which panel type is loaded in the
/// side column. That is the same gesture a stacked layout would use to move
/// between rows, so the key is now earning its place instead of advertising
/// something the UI cannot do.
///
/// Each letter is bound twice. Terminals disagree about how to encode a
/// shifted letter: many report `Char('H') + SHIFT`, others a bare `Char('H')`
/// with no modifier at all. Binding both is safe because uppercase `H`/`J`/
/// `K`/`L` are otherwise unbound in Normal mode, and `is_normal_only` keeps
/// Insert mode treating them as text. This is the same legacy-encoding hazard
/// that made every `Ctrl+Shift+X` chord undecodable — see
/// `tests/modifier_chords.rs`.
fn insert_panel_direction_bindings(map: &mut HashMap<(KeyModifiers, KeyCode), InputAction>) {
    for (key_code, action) in [
        (KeyCode::Left, InputAction::DirectionLeft),
        (KeyCode::Right, InputAction::DirectionRight),
        (KeyCode::Up, InputAction::DirectionUp),
        (KeyCode::Down, InputAction::DirectionDown),
    ] {
        map.insert((KeyModifiers::SHIFT, key_code), action);
    }
    // Uppercase letters only. `Char('l')` is a bare `l`, which is a
    // withinsurface vim verb, not the directional gesture.
    for (letter, action) in [
        ('H', InputAction::DirectionLeft),
        ('J', InputAction::DirectionDown),
        ('K', InputAction::DirectionUp),
        ('L', InputAction::DirectionRight),
    ] {
        map.insert((KeyModifiers::SHIFT, KeyCode::Char(letter)), action.clone());
        map.insert((KeyModifiers::NONE, KeyCode::Char(letter)), action);
    }
    // The panel manager also answers to `Shift+S`. Bound under both encodings
    // for the same legacy-terminal reason as the vim letters, and normal-only
    // so it never claims an `S` mid-word.
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Char('S')),
        InputAction::OpenPanelManager,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('S')),
        InputAction::OpenPanelManager,
    );
    // The project picker answers to `Shift+F` in Normal mode only, for
    // the same reason: `F` is typed text in Insert. Bound under both
    // encodings like `Shift+S` because terminals disagree on whether a
    // shifted letter arrives with the SHIFT modifier. `Ctrl+\` (and bare
    // `\` in vim Normal) remain the chords that work in both modes.
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Char('F')),
        InputAction::OpenProjectPicker,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('F')),
        InputAction::OpenProjectPicker,
    );
    // `Shift+T` used to open the unified tasks overlay under both encodings.
    // It is retired: a capital `T` mid-word is text the user is actively
    // typing, so the action had to be `is_normal_only`, and the app boots in
    // Insert — the first press composed a literal `T` and the overlay looked
    // broken. `Ctrl+T` now owns the overlay and needs no mode gate, because a
    // CONTROL chord is never composable text. Re-adding a shifted capital here
    // would require re-adding the gate, and re-adding the gate would make the
    // overlay unreachable from Insert again.
    // `Shift+N` / `Shift+P` / `Shift+X` drive the subagent tab set in the main
    // buffer: next, previous, close. All three are uppercase letters that were
    // unbound in every keymap, and all three are bound under both encodings for
    // the same legacy reason as `Shift+S`/`Shift+F`/`Shift+T` above.
    //
    // They are separate letters rather than a reused chord on purpose. These
    // names the buffer itself, adjacent to `Tab` for project tabs — but they
    // are a different axis entirely (subagents inside one session, versus
    // projects beside it), so reusing `Ctrl+Alt+N`/`Ctrl+Alt+B` would have
    // collapsed two independent navigations onto one gesture and made the
    // project-tab muscle memory fire a subagent switch instead.
    for (letter, action) in [
        ('N', InputAction::NextSubagentTab),
        ('P', InputAction::PreviousSubagentTab),
        ('X', InputAction::CloseSubagentTab),
    ] {
        map.insert((KeyModifiers::SHIFT, KeyCode::Char(letter)), action.clone());
        map.insert((KeyModifiers::NONE, KeyCode::Char(letter)), action);
    }
}

fn default_bindings_internal() -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    let mut map = HashMap::new();
    insert_panel_direction_bindings(&mut map);
    map.insert((KeyModifiers::SHIFT, KeyCode::Enter), InputAction::Newline);
    map.insert((KeyModifiers::NONE, KeyCode::Enter), InputAction::Send);
    // Ctrl+C is the unconditional exit; Esc only backs out of whatever is on
    // screen and never terminates the app.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('c')),
        InputAction::ForceQuit,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Esc), InputAction::Cancel);
    // `q` quits from Normal mode. Insert mode skips bare-char bindings, so
    // this costs nothing while typing.
    map.insert((KeyModifiers::NONE, KeyCode::Char('q')), InputAction::Quit);
    // `x` stops a running turn and resumes a stopped one. It used to be a
    // bare `x`, which the composer now claims for Helix line-select
    // (ADR-0042), so the action moved to a chord that is not a letter.
    //
    // `Ctrl+Alt+X` is the legacy-safe chord shape: the terminal delivers
    // `ESC` + `0x18`, which crossterm reports as `Char('x')` with
    // CONTROL|ALT — the same family as `Ctrl+Alt+B` and `Ctrl+Alt+N`. A
    // bare `Ctrl+X` would be a single byte and `Ctrl+Alt+X` degrades to
    // nothing rather than to a different action, so a stray press cannot
    // stop a turn the user did not mean to stop.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('x'),
        ),
        InputAction::ToggleAgentRun,
    );
    // `d` opens the diff view. The composer claims `d` for Helix
    // delete-selection (ADR-0042), so the action moved to a chord that is
    // not a letter. `Ctrl+Alt+D` keeps the letter, so the gesture stays
    // recognisable, and the bare `Ctrl+D` twin keeps its own meaning
    // (page down) rather than aliasing this one.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('d'),
        ),
        InputAction::OpenDiff,
    );
    // `i` focuses the prompt. The composer claims `i` for Helix
    // insert-before-caret (ADR-0042), so the action moved to a chord that is
    // not a letter. `Ctrl+Alt+I` is **not** that chord: the terminal
    // delivers `ESC` + `0x09`, which crossterm reports as `Tab` with ALT,
    // so the verb would be unreachable on every terminal. The chord that
    // works is `Ctrl+Alt+E` — a legacy-safe letter whose `ESC` + `0x05`
    // arrives as `Char('e')` with CONTROL|ALT — and it is bound in the vim
    // map too, so the action is reachable whichever map is active.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('e'),
        ),
        InputAction::FocusPrompt,
    );
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
    // `h` and `l` move the composer caret, which is exactly Helix `h` and
    // `l`. The vim map has always bound them; this map did not, and the
    // asymmetry had teeth: ADR-0042 and the composer plan both record `h`
    // and `l` as "no conflict — the existing action already moves the
    // composer caret", so the composer claim table left them unclaimed on
    // the strength of a premise that was only true in the vim map. In the
    // default configuration the composer's Normal-mode caret therefore had
    // no letter that could move it: `h` and `l` were unbound, and `w`, `b`
    // and `e` are claimed by the composer and consumed as pending verbs.
    //
    // The binding is the same one the arrow keys already use, so `h`/`l`
    // behave exactly like `Left`/`Right` — including in the surfaces that
    // own the keyboard before the binding map is consulted. A caret motion
    // is a Helix-parity capability rather than a keymap flavour, so it
    // belongs in both maps; `scripts/check_tui_composer_key_authority.py`
    // rule G9 fails if the two maps ever disagree about a caret motion
    // again.
    map.insert((KeyModifiers::NONE, KeyCode::Char('h')), InputAction::Left);
    map.insert((KeyModifiers::NONE, KeyCode::Char('l')), InputAction::Right);
    map.insert(
        (KeyModifiers::NONE, KeyCode::Tab),
        InputAction::ToggleComposerMode,
    );
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
    // `Ctrl+N` is deliberately NOT bound to NewSession. New session is the bare
    // `n` below — bare, so Insert mode still types the letter — and `Ctrl+N`
    // is the chord that arms yazi-style panel resize mode
    // (`commands::panels::handle_panel_resize_key`). A chord is the right
    // spelling for a modal frame gesture: it cannot collide with a panel verb
    // the way a bare letter would, and it stays out of the composer.
    //
    // It is deliberately absent from `default_help_entries` too, and that list
    // is *auditable* — `documented_keys_resolve_in_the_active_map` asserts
    // every advertised key resolves to an `InputAction`. Resize mode is a frame
    // gesture rather than a composable action, so it names itself on the
    // composer line when armed (and in `docs/tui.md`) instead of pretending to
    // be a binding.
    // Ctrl+T opens the unified tasks overlay: todos, then in-flight
    // subagents, then jobs the session is waiting on. One chord, three
    // groups, and a group with no rows is omitted rather than drawn empty.
    //
    // This replaced a two-key arrangement (`Ctrl+T` for the compact todo
    // strip, `Shift+T` for the overlay) that was unreachable in practice:
    // `Shift+T` is spelled as a capital, so it was `is_normal_only`, and the
    // app boots in Insert — the first press composed a literal `T`. Because
    // `Ctrl+T` carries CONTROL it is never composable text, so the action
    // needs no mode gate at all and the overlay is reachable from the mode
    // the user is actually in. The sidebar stayed on Ctrl+B so both panels
    // remain independently reachable.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('t')),
        InputAction::ToggleTasksList,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('b')),
        InputAction::ToggleSidebar,
    );
    // Normal-mode space opens help (the `/help` affordance). Insert mode is
    // unaffected: bare printable keys remain prompt text there.
    map.insert((KeyModifiers::NONE, KeyCode::Char(' ')), InputAction::Help);
    // ── Chord-selection contract ────────────────────────────────────────
    //
    // Multi-modifier chords must be built from `{CONTROL, ALT}` only. A bare
    // `Ctrl+<letter>` is unusable because every free bare Ctrl letter is a
    // terminal control character (Ctrl+H backspace, Ctrl+I tab, Ctrl+J
    // newline, Ctrl+M enter, Ctrl+V literal-next, Ctrl+Z suspend) and binding
    // one steals the terminal's own job.
    //
    // `Ctrl+Shift+<letter>` is *also* unusable, for a subtler reason: without
    // keyboard-enhancement flags (never pushed — see `tui/terminal.rs`) the
    // terminal is in legacy xterm encoding, where `Ctrl+Alt+W` and `Ctrl+W`
    // are the same byte 0x17. crossterm therefore reports `Char('w')` with
    // CONTROL only, and a `(CONTROL|SHIFT, Char('W'))` binding is
    // unreachable — worse, the key silently falls through to whichever action
    // owns the bare-Ctrl twin. That is how the panel switcher ended up firing
    // `CloseSession`.
    //
    // `Ctrl+Alt+<letter>` has no such collision: legacy encoding sends `ESC`
    // followed by the single control byte, so crossterm reports CONTROL|ALT
    // and the pair is unambiguous against both bare `Ctrl+W` and bare
    // `Alt+W`. `tests/modifier_chords.rs` asserts the whole set stays
    // disjoint from their unshifted twins.
    //
    // Sidebar focus keeps a reachable chord now that space is help.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('s'),
        ),
        InputAction::FocusSidebar,
    );
    // The side-panel manager used to answer a second key, `Ctrl+Alt+W`,
    // here so it could be reached while composing. It is retired: the
    // manager is a Normal-mode surface like the directionals that drive
    // the same panels, and one key per surface is one less thing to
    // remember and one fewer chord to get wrong in a legacy terminal.
    // `Shift+S` (`OpenPanelManager`) is the only spelling now.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('w')),
        InputAction::CloseSession,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('a')),
        InputAction::SwitchAgent,
    );
    // `/` opens the command palette from Normal mode, and it **stays**.
    //
    // The composer does not claim `/`, so this spelling has no conflict with
    // the claim table: ADR-0042's conflict table names the *vim* map's `i` as
    // the focus-prompt conflict, and the re-spelling exists because the
    // composer claims that letter. `/` is not a claimed key, and the plan's
    // invariant — "a letter the composer does not claim must resolve to the
    // same app action it resolves to today" — requires it to keep resolving
    // to `FocusPrompt`.
    //
    // It is also load-bearing beyond the action: `/` is the gesture that
    // enters command mode, and command mode is what makes the rest of a
    // typed slash command land as text. Removing it took away "type
    // `/panels` from Normal mode and press Enter" — the only way back into
    // the panel manager from a focused panel, which owns every key by
    // contract (`tests/tui_side_panels.rs`).
    //
    // The focus-prompt **capability** moved to `Ctrl+Alt+E` in both maps, and
    // the vim map's `i` spelling is gone, which is the spelling the ADR
    // actually named.
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
    // Ctrl+Alt+P cycles backward. Deliberately `P` and not `M`: bare `Ctrl+M` is
    // byte 0x0D, i.e. Enter, so `Ctrl+Alt+M` would be `ESC` + CR and is not a
    // chord a legacy terminal can be relied on to disambiguate.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('p'),
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
    // `Shift+G` used to be bound here, but `is_bare_char` only suppresses
    // bindings carrying `KeyModifiers::NONE` — a `SHIFT` binding still matched,
    // so capital `G` scrolled the viewport instead of being typed into the
    // prompt. Any binding that would steal printable text moves to a CONTROL
    // chord: `Ctrl+End` reaches the same action and leaves `G` typeable.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::End),
        InputAction::GoToBottom,
    );
    map.insert((KeyModifiers::CONTROL, KeyCode::Home), InputAction::GoToTop);
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
    // Ctrl+T opens the unified tasks overlay: todos, then in-flight
    // subagents, then jobs the session is waiting on. One chord, three
    // groups, and a group with no rows is omitted rather than drawn empty.
    //
    // This replaced a two-key arrangement (`Ctrl+T` for the compact todo
    // strip, `Shift+T` for the overlay) that was unreachable in practice:
    // `Shift+T` is spelled as a capital, so it was `is_normal_only`, and the
    // app boots in Insert — the first press composed a literal `T`. Because
    // `Ctrl+T` carries CONTROL it is never composable text, so the action
    // needs no mode gate at all and the overlay is reachable from the mode
    // the user is actually in. The sidebar stayed on Ctrl+B so both panels
    // remain independently reachable.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('t')),
        InputAction::ToggleTasksList,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('b')),
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
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('y'),
        ),
        InputAction::StopTts,
    );
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('f'),
        ),
        InputAction::ToggleFullscreen,
    );
    // Ctrl+Alt+n next project tab. A letter chord rather than the bracket
    // pair the chord contract rules out; see the CONTROL|ALT note above.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('n'),
        ),
        InputAction::NextProjectTab,
    );
    // Ctrl+Alt+b previous project tab
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('b'),
        ),
        InputAction::PreviousProjectTab,
    );
    // Alt+w close project tab
    map.insert(
        (KeyModifiers::ALT, KeyCode::Char('w')),
        InputAction::CloseProjectTab,
    );
    // C001 root composer ownership: bare Tab cycles Session<->Task at
    // root prompt focus in both Insert and Normal maps. `SwitchAgent`
    // migrates to Ctrl+A (0x01, portable and distinct from Tab/0x09).
    // Terminal-encoding audit: Ctrl+I (0x09 == Tab), Ctrl+H (0x08 ==
    // Backspace), Ctrl+M (0x0D == Enter), and Ctrl+J (0x0A) are all
    // normalized to Tab/Enter/Backspace on common terminals and are
    // never used as the replacement. Ctrl+G (0x07) stays a
    // backward-compatible alias for ToggleComposerMode.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('g')),
        InputAction::ToggleComposerMode,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('a')),
        InputAction::SwitchAgent,
    );
    // Ctrl+O opens the global Workspace dashboard (M004). Unbound
    // before: no default used Ctrl+O (the keybind-audit test pins
    // this), so the dashboard hotkey collides with nothing.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('o')),
        InputAction::OpenWorkspaceDashboard,
    );
    map
}

pub fn default_bindings() -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    DEFAULT_BINDINGS.clone()
}

fn vim_bindings_internal() -> HashMap<(KeyModifiers, KeyCode), InputAction> {
    let mut map = HashMap::new();
    insert_panel_direction_bindings(&mut map);

    map.insert((KeyModifiers::NONE, KeyCode::Enter), InputAction::Send);
    map.insert((KeyModifiers::SHIFT, KeyCode::Enter), InputAction::Newline);
    map.insert((KeyModifiers::NONE, KeyCode::Esc), InputAction::Cancel);
    // Ctrl+C exits; Esc never does. See `default_bindings_internal`.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('c')),
        InputAction::ForceQuit,
    );

    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('j')),
        InputAction::NavigateDown,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('k')),
        InputAction::NavigateUp,
    );
    // Stop / resume the running turn. The default map's bare `x` is gone:
    // the composer claims `x` for Helix line-select (ADR-0042), so the
    // action moved to a chord that is not a letter. The chord is in **both**
    // maps, so the capability does not depend on which map is active.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('x'),
        ),
        InputAction::ToggleAgentRun,
    );
    map.insert((KeyModifiers::NONE, KeyCode::Char('h')), InputAction::Left);
    map.insert((KeyModifiers::NONE, KeyCode::Char('l')), InputAction::Right);
    // The vim map's bare `i` (focus prompt) and bare `d` (open diff) are
    // gone: the composer claims both letters for Helix insert-before-caret
    // and delete-selection (ADR-0042). The actions moved to chords that are
    // not letters, and the chords are bound in **both** maps so the
    // capability is reachable whichever map is active.
    //
    // `Ctrl+Alt+I` would be the mnemonic spelling, but the terminal delivers
    // it as `ESC` + `0x09`, which crossterm reports as `Tab` with ALT — the
    // verb would be unreachable on every terminal. `Ctrl+Alt+E` is the
    // deliverable alternative (`ESC` + `0x05` arrives as `Char('e')` with
    // CONTROL|ALT), and "E" reads as "e\nter" the command palette.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('e'),
        ),
        InputAction::FocusPrompt,
    );
    // Vim normal mode equivalent of the documented "Space a" concept.
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('a')),
        InputAction::FocusSidebar,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char(':')),
        InputAction::Command,
    );

    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('g')),
        InputAction::GoToTop,
    );
    // Sidebar focus in Normal mode. The Insert map already binds `Space`, but
    // the vim map did not, so sidebar focus was unreachable whenever
    // `vim_mode=true` even though the help advertised it unconditionally.
    map.insert((KeyModifiers::NONE, KeyCode::Char(' ')), InputAction::Help);
    // Sidebar focus keeps a reachable chord now that space is help. See the
    // CONTROL|ALT chord contract in `default_bindings_internal` for why this
    // is not `Ctrl+Alt+S`.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('s'),
        ),
        InputAction::FocusSidebar,
    );
    // The side-panel switcher no longer has a `Ctrl+Alt+W` binding here or
    // in the Insert map: `Shift+S` is the single spelling, and it is
    // already inserted into every map by `insert_panel_direction_bindings`.
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

    // Ctrl+T opens the unified tasks overlay: todos, then in-flight
    // subagents, then jobs the session is waiting on. One chord, three
    // groups, and a group with no rows is omitted rather than drawn empty.
    //
    // This replaced a two-key arrangement (`Ctrl+T` for the compact todo
    // strip, `Shift+T` for the overlay) that was unreachable in practice:
    // `Shift+T` is spelled as a capital, so it was `is_normal_only`, and the
    // app boots in Insert — the first press composed a literal `T`. Because
    // `Ctrl+T` carries CONTROL it is never composable text, so the action
    // needs no mode gate at all and the overlay is reachable from the mode
    // the user is actually in. The sidebar stayed on Ctrl+B so both panels
    // remain independently reachable.
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('t')),
        InputAction::ToggleTasksList,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('b')),
        InputAction::ToggleSidebar,
    );
    map.insert(
        (KeyModifiers::NONE, KeyCode::Tab),
        InputAction::ToggleComposerMode,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('a')),
        InputAction::SwitchAgent,
    );
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
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('f'),
        ),
        InputAction::ToggleFullscreen,
    );

    // The vim map's bare `d` (open diff) is gone: the composer claims `d`
    // for Helix delete-selection (ADR-0042). The action moved to a chord
    // that is not a letter, and the chord is bound in both maps.
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('d'),
        ),
        InputAction::OpenDiff,
    );

    // \ also opens the project picker in vim normal mode.
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('\\')),
        InputAction::OpenProjectPicker,
    );
    // } next project tab (vim normal)
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('}')),
        InputAction::NextProjectTab,
    );
    // { previous project tab (vim normal)
    map.insert(
        (KeyModifiers::NONE, KeyCode::Char('{')),
        InputAction::PreviousProjectTab,
    );
    // Q close project tab (vim normal, uppercase to not conflict with q=quit)
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Char('Q')),
        InputAction::CloseProjectTab,
    );
    // Ctrl+Alt+n / Ctrl+Alt+b also in vim mode (same chords as insert mode)
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('n'),
        ),
        InputAction::NextProjectTab,
    );
    map.insert(
        (
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyCode::Char('b'),
        ),
        InputAction::PreviousProjectTab,
    );
    // Alt+w close project tab (same as insert mode)
    map.insert(
        (KeyModifiers::ALT, KeyCode::Char('w')),
        InputAction::CloseProjectTab,
    );
    // Ctrl+G toggles the Task composer (same as insert mode; bare Tab
    // owns the composer in both modes, Ctrl+A owns SwitchAgent).
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('g')),
        InputAction::ToggleComposerMode,
    );
    // W opens the global Workspace dashboard in vim normal mode
    // (mnemonic: Workspace; Q stays close-tab, q stays quit). Ctrl+O
    // matches insert mode for consistency.
    map.insert(
        (KeyModifiers::SHIFT, KeyCode::Char('W')),
        InputAction::OpenWorkspaceDashboard,
    );
    map.insert(
        (KeyModifiers::CONTROL, KeyCode::Char('o')),
        InputAction::OpenWorkspaceDashboard,
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
            action: "Newline in prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Esc",
            // One action, several effects: `Esc` is always `Cancel`, and
            // what it cancels depends on what is on screen. Naming the action
            // keeps the row honest without flattening the useful detail.
            action: "Cancel — switch to normal mode",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+L",
            action: "Select model",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+E",
            action: "Open prompt in external editor",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+End",
            action: "Go to bottom",
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
            key: "Ctrl+T",
            action: "Toggle tasks (todos, subagents, jobs)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+B",
            action: "Toggle sidebar",
            condition: None,
        },
        // The focus moves are Normal-mode gestures by contract. They are
        // listed under Insert as well because the app boots into Insert and
        // a user reading only that section is exactly who would try `Shift+→`
        // from the composer and conclude the panel is unreachable.
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Shift+→ / Shift+L",
            action: "Focus side panel (normal mode)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Shift+← / Shift+H",
            action: "Focus main buffer (normal mode)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Space",
            action: "Show help",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Alt+S",
            action: "Focus sidebar",
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
            action: "Toggle composer mode (Session/Task)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+A",
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
            key: "Ctrl+Alt+P",
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
            key: "Ctrl+Alt+Y",
            action: "Stop TTS",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Alt+F",
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
            key: "Ctrl+C",
            action: "Force quit — unwind like Esc, then exit",
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
            key: "Ctrl+Alt+N",
            action: "Next project tab",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+Alt+B",
            action: "Previous project tab",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Alt+W",
            action: "Close project tab",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+G",
            action: "Toggle composer mode (Session/Task, alias)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Insert,
            key: "Ctrl+O",
            action: "Open Workspace view (selected-project chat)",
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
            // The composer claims `j` and `k` when its buffer has more than
            // one line (ADR-0042), so transcript scroll is what is left of
            // the pair on a single-line buffer.
            condition: Some("single-line composer buffer"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Space",
            action: "Show help",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "a",
            action: "Focus sidebar (agent tree)",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Enter",
            // One key, three outcomes by row kind: an agent-tree row opens the
            // run-detail dialog, a subagent row opens its transcript as a main
            // buffer tab, and a job row opens nothing (it has no transcript).
            // A job row is deliberately inert rather than routed somewhere
            // arbitrary — see `a_job_row_is_read_only_and_never_reaches_the_composer`.
            action: "Inspect sidebar selection (a subagent row opens its transcript tab)",
            condition: Some("sidebar focus"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "←/→",
            action: "Collapse/expand sidebar row",
            condition: Some("sidebar focus"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "h/l",
            action: "Collapse/expand sidebar row",
            condition: Some("vim mode, sidebar focus"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Esc",
            action: "Cancel — leave sidebar focus",
            condition: Some("sidebar focus"),
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
            action: "Show help",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+Alt+E",
            action: "Focus prompt (insert mode)",
            condition: None,
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
            key: "Ctrl+Alt+D",
            action: "Open diff",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+Alt+X",
            action: "Stop / resume agent (toggle the running turn)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "\\",
            action: "Open project picker",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "}",
            action: "Next project tab",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "{",
            action: "Previous project tab",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Q",
            action: "Close project tab",
            condition: Some("vim mode"),
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+Alt+N",
            action: "Next project tab",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+Alt+B",
            action: "Previous project tab",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Alt+W",
            action: "Close project tab",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+G",
            action: "Toggle composer mode (Session/Task, alias)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+O",
            action: "Open Workspace view (selected-project chat)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "W",
            action: "Open Workspace view (selected-project chat)",
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
            action: "Cancel — clear draft / close popup (never exits)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+C",
            action: "Force quit — unwind like Esc, then exit",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Tab",
            action: "Toggle composer mode (Session/Task)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+A",
            action: "Switch agent",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+L",
            action: "Select model",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+T",
            action: "Toggle tasks (todos, subagents, jobs)",
            condition: None,
        },
        // The Editor route claims `Ctrl+T` for its file tree and never reaches
        // the binding table (`commands::editor` intercepts it above the
        // resolver), so the same key means two different things depending on
        // which screen is up. That is survivable only while help says so —
        // D-C8 keeps the editor's long-established tree toggle rather than
        // moving it here, and records the difference instead.
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+T",
            action: "Toggle the file tree instead of the tasks overlay",
            condition: Some("editor route"),
        },
        // `Ctrl+S` stashes the composed prompt everywhere except the Editor
        // route, where the buffer owns it as *save*. The override is
        // documented here (D-C8) rather than left for a user to discover:
        // a chord that means two different things depending on the screen is
        // only acceptable while help says so out loud.
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Ctrl+S",
            action: "Stash prompt (saves the buffer on the Editor route)",
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
            key: "Ctrl+Alt+P",
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
        HelpEntry {
            mode: HelpMode::Command,
            key: "/terminal",
            action: "Open an interactive terminal in a box (Shift+L docks it into the panel)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/terminal-clear",
            action: "Clear a terminal's scrollback and screen; the process keeps running",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+L / Shift+Right",
            action: "Pane right — dock the open box into the side panel, or select a pane when none is open",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+Left / Shift+H",
            action: "Pane left — return the keyboard to the main buffer",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+Up / Shift+Down",
            action: "Next panel — cycle which panel is loaded in the side column",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+S",
            action: "Open panel manager — pick which panel the side column holds, and bind its Ctrl+1..Ctrl+0 hotkey",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+F",
            action: "Open project picker; /projects also works mid-prompt",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+N",
            action: "Next subagent tab in the main buffer (cycles through the conversation)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+P",
            action: "Previous subagent tab in the main buffer",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Normal,
            key: "Shift+X",
            action: "Close subagent tab — the run keeps going either way",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/panels",
            action: "Same side-panel manager, reachable while composing — the gesture above is the letter S in Insert",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/terminal-create <cmd>",
            action: "Create an interactive workspace terminal",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/terminal-list",
            action: "List interactive terminals in this workspace",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/terminal-attach <handle>",
            action: "Attach to an interactive terminal",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/terminal-detach [handle]",
            action: "Detach (process keeps running)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/terminal-terminate [handle]",
            action: "Terminate an interactive terminal process",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/collaborators",
            action:
                "Show live presence for the active project (ephemeral; membership admin is /team)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/collaborators refresh",
            action: "Refresh collaborator presence",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/team",
            action:
                "Manage project membership and chat access (Owner; LocalOwner issues device tokens)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/observe <session-id>",
            action: "Follow another session read-only",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/stop-observing",
            action: "Stop following the observed session",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/chat",
            action: "Show project chat for the active project",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/chat-send <text>",
            action: "Send a chat message (bare input while observing)",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Command,
            key: "/chat-history",
            action: "Reload the project chat window",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Dialog,
            key: "i (terminal view)",
            action: "Focus terminal: keystrokes go to the process",
            condition: None,
        },
        HelpEntry {
            mode: HelpMode::Dialog,
            key: "Esc (terminal focused)",
            action: "Leave terminal focus (never submits)",
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
                // Hide an entry only when its condition is unsatisfiable in
                // the active configuration. A condition mentioning vim mode
                // must match on the substring, because compound conditions
                // such as "vim mode, sidebar focus" also gate on vim mode.
                if !vim_mode {
                    if let Some(cond) = e.condition {
                        return !cond.contains("vim mode");
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
            // Surface every condition, not just the vim one. Dropping notes
            // silently made entries such as "h/l ... (sidebar focus)" render
            // as unconditional, so users applied them outside sidebar focus.
            let condition_note = match entry.condition {
                Some(cond) if !cond.is_empty() => format!(" ({cond})"),
                _ => String::new(),
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
                let action = action.to_input_action();
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

pub(crate) fn handle_key_with_bindings(
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
            // A text-modifier letter (NONE or SHIFT + printable Char) bound
            // only by the *default* map is typed as text: this is what keeps
            // default `Shift+S`/`Shift+F` typeable while leaving `Ctrl+\`
            // working in both modes. An explicit user binding for the same
            // key still fires (custom bindings win over text).
            let is_text_key = matches!(key.code, KeyCode::Char(c)
                if is_text_modifier(key.modifiers) && is_printable_char(c));
            // A **built-in** text-modifier letter is composed text, not an
            // action, even when the caller passed its own resolved map. This
            // used to be gated on `is_default_map` (i.e. `bindings.is_none()`),
            // which is dead in the real app: `App::on_key` always passes
            // `Some(&self.ui_state.bindings)`. So typing a capital `F`
            // mid-word opened the project picker instead of composing, in the
            // default configuration — `Shift+F` had been waved through as
            // safe by a rule that never ran.
            //
            // "Built-in" means the binding ships in either of the two maps the app
            // can be configured with — it is checked by comparing the action
            // against both, not by map identity, which keeps the
            // neighbouring arm's promise intact: an *explicit user binding*
            // still wins over text. `VIM_BINDINGS` matters as much as
            // `DEFAULT_BINDINGS` here: `Shift+G` exists only in the vim map,
            // and the regression test that used to cover it passed
            // `bindings: None`, so it exercised the one path where the old
            // gate fired and stayed green while the real path was broken.
            // The lowercase letters stay on `is_normal_only`, which is the
            // right tool for `x`/`ToggleAgentRun` because that one binds no
            // modifier at all.
            let is_builtin_text_key = is_text_key
                && (map.get(&key_tuple) == DEFAULT_BINDINGS.get(&key_tuple)
                    || map.get(&key_tuple) == VIM_BINDINGS.get(&key_tuple));

            if let Some(action) = map.get(&key_tuple) {
                if is_bare_char || is_normal_only(action) || is_builtin_text_key {
                    debug_log!("insert mode: skipping bare char binding: {:?}", action);
                } else {
                    debug_log!("insert mode: matched binding: {:?}", action);
                    return Some(action.clone());
                }
            }
            if let Some(_user_bindings) = bindings {
                if let Some(action) = DEFAULT_BINDINGS.get(&key_tuple) {
                    if is_bare_char || is_normal_only(action) || is_text_key {
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
    use std::collections::BTreeSet;

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn c001_bare_tab_owns_composer_cycle_in_both_modes() {
        // Root prompt focus: bare Tab toggles Session<->Task in both
        // Insert and Normal maps without touching InputMode.
        let tab = make_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(
            handle_key_with_bindings(tab, None, InputMode::Insert),
            Some(InputAction::ToggleComposerMode)
        );
        assert_eq!(
            handle_key_with_bindings(tab, None, InputMode::Normal),
            Some(InputAction::ToggleComposerMode)
        );
        // Ctrl+G alias produces the identical action.
        let ctrl_g = make_key(KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert_eq!(
            handle_key_with_bindings(ctrl_g, None, InputMode::Insert),
            Some(InputAction::ToggleComposerMode)
        );
        assert_eq!(
            handle_key_with_bindings(ctrl_g, None, InputMode::Normal),
            Some(InputAction::ToggleComposerMode)
        );
        // SwitchAgent remains reachable on its migrated binding.
        let ctrl_a = make_key(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(
            handle_key_with_bindings(ctrl_a, None, InputMode::Insert),
            Some(InputAction::SwitchAgent)
        );
        assert_eq!(
            handle_key_with_bindings(ctrl_a, None, InputMode::Normal),
            Some(InputAction::SwitchAgent)
        );
        // No duplicate bare-Tab owner after normalization.
        for bindings in [default_bindings(), vim_bindings()] {
            let tab_actions: Vec<_> = bindings
                .iter()
                .filter(|((mods, code), _)| *mods == KeyModifiers::NONE && *code == KeyCode::Tab)
                .map(|(_, action)| action.clone())
                .collect();
            assert_eq!(tab_actions, vec![InputAction::ToggleComposerMode]);
            // Replacement is never a Tab-normalized control (Ctrl+I/H/M/J).
            for forbidden in ['i', 'h', 'm', 'j'] {
                assert_ne!(
                    bindings.get(&(KeyModifiers::CONTROL, KeyCode::Char(forbidden))),
                    Some(&InputAction::SwitchAgent),
                    "SwitchAgent must not use terminal-ambiguous Ctrl+{forbidden}"
                );
            }
        }
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
    fn test_ctrl_alt_p_returns_cycle_model_backward() {
        let key = make_key(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::CycleModelBackward));
    }

    #[test]
    fn ctrl_alt_m_is_not_a_chord() {
        // Bare `Ctrl+M` is byte 0x0D (Enter), so `Ctrl+Alt+M` is `ESC` + CR
        // and is not a chord a legacy terminal can disambiguate. Model cycling
        // backward therefore lives on `Ctrl+Alt+P`.
        let key = make_key(
            KeyCode::Char('m'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_ne!(
            handle_key_with_bindings(key, None, InputMode::Insert),
            Some(InputAction::CycleModelBackward),
            "Ctrl+Alt+M must not resolve to model cycling; use Ctrl+Alt+P"
        );
    }

    #[test]
    fn the_retired_ctrl_alt_w_no_longer_opens_the_panel_manager() {
        // `Ctrl+Alt+W` was retired in favour of `Shift+S`. It must not fall
        // through to anything either: the Control chord it shares is gone,
        // and silently resolving to `CloseSession` would be the worst
        // possible reading of a key the help no longer advertises.
        let key = make_key(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        for mode in [InputMode::Insert, InputMode::Normal] {
            assert_ne!(
                handle_key_with_bindings(key, None, mode),
                Some(InputAction::OpenPanelManager),
                "the retired chord must not reach the panel manager in {mode:?}"
            );
        }
    }

    #[test]
    fn the_panel_manager_is_reachable_from_shift_s_in_normal_only() {
        // Both encodings, because terminals disagree about whether a
        // shifted letter carries the SHIFT flag.
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
            let key = make_key(KeyCode::Char('S'), modifiers);
            assert_eq!(
                handle_key_with_bindings(key, None, InputMode::Normal),
                Some(InputAction::OpenPanelManager),
                "the panel manager must be reachable in Normal mode"
            );
            // In Insert it stays the letter S.
            assert!(
                !matches!(
                    handle_key_with_bindings(key, None, InputMode::Insert),
                    Some(InputAction::OpenPanelManager)
                ),
                "the panel manager must not steal the letter S mid-word"
            );
        }
    }

    #[test]
    fn bare_ctrl_w_still_closes_the_session() {
        // The chord fix must not change what the bare key does: `Ctrl+W`
        // still owns CloseSession, and the retired chord sharing that
        // letter must not change it either.
        let key = make_key(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(
            handle_key_with_bindings(key, None, InputMode::Insert),
            Some(InputAction::CloseSession)
        );
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

    /// Shift+F opens the picker in Normal mode and stays typeable text in Insert mode.
    #[test]
    fn shift_f_opens_picker_in_normal_only() {
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
            let key = make_key(KeyCode::Char('F'), modifiers);
            assert_eq!(
                handle_key_with_bindings(key, None, InputMode::Normal),
                Some(InputAction::OpenProjectPicker),
                "Shift+F must open the picker in Normal (modifiers={modifiers:?})"
            );
        }
        let shifted = make_key(KeyCode::Char('F'), KeyModifiers::SHIFT);
        assert_eq!(
            handle_key_with_bindings(shifted, None, InputMode::Insert),
            Some(InputAction::Char('F')),
            "Shift+F must type text while composing"
        );
        let chord = make_key(KeyCode::Char('\\'), KeyModifiers::CONTROL);
        assert_ne!(
            handle_key_with_bindings(chord, None, InputMode::Insert),
            Some(InputAction::OpenProjectPicker),
            "Ctrl+\\ must not open the picker in Insert mode"
        );
        assert_ne!(
            handle_key_with_bindings(chord, None, InputMode::Normal),
            Some(InputAction::OpenProjectPicker),
            "Ctrl+\\ must not open the picker in Normal mode"
        );
    }

    #[test]
    fn normal_space_opens_help_and_insert_space_remains_text() {
        let key = make_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(
            handle_key_with_bindings(key, None, InputMode::Normal),
            Some(InputAction::Help)
        );
        assert_eq!(
            handle_key_with_bindings(key, None, InputMode::Insert),
            Some(InputAction::Char(' '))
        );
    }

    #[test]
    fn sidebar_focus_keeps_a_reachable_chord() {
        // Space moved to help, so focus must stay reachable on a chord that
        // does not collide with a terminal control character — or with the
        // bare-Ctrl twin, which is why this is Ctrl+Alt+S.
        let key = make_key(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(
            handle_key_with_bindings(key, None, InputMode::Normal),
            Some(InputAction::FocusSidebar)
        );
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

    /// `Ctrl+T` is the single key that answers "what is this session waiting
    /// on" — todos, subagents, and jobs together.
    ///
    /// It must work from Insert mode specifically. The app boots in Insert, so
    /// a chord that needs a prior `Esc` is a chord most users never reach; the
    /// surface it opens used to be behind `Shift+T`, which was Normal-only and
    /// therefore composited a literal `T` on first press.
    #[test]
    fn ctrl_t_opens_the_tasks_overlay_in_both_modes() {
        for mode in [InputMode::Insert, InputMode::Normal] {
            let key = make_key(KeyCode::Char('t'), KeyModifiers::CONTROL);
            assert_eq!(
                handle_key_with_bindings(key, None, mode),
                Some(InputAction::ToggleTasksList),
                "Ctrl+T must open the tasks overlay in {mode:?}"
            );
        }
    }

    /// Regression: `Shift+T` is retired, so a capital `T` is text again.
    ///
    /// This is the test that fails if someone re-binds the shifted capital
    /// *and* relaxes `is_normal_only` — the combination silently breaks the
    /// composer for every word containing a `T`, which is most of them. The
    /// two facts are coupled on purpose and must be changed together.
    #[test]
    fn capital_t_composes_text_in_insert_mode() {
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::NONE] {
            let key = make_key(KeyCode::Char('T'), modifiers);
            assert_eq!(
                handle_key_with_bindings(key, None, InputMode::Insert),
                Some(InputAction::Char('T')),
                "a capital T must compose text, not open the overlay \
                 (modifiers {modifiers:?})"
            );
        }
    }

    #[test]
    fn lowercase_t_is_not_the_tasks_overlay() {
        // The chord is the only spelling: a bare `t` is a prompt character in
        // both modes and must never open the overlay.
        for mode in [InputMode::Insert, InputMode::Normal] {
            let key = make_key(KeyCode::Char('t'), KeyModifiers::NONE);
            assert_ne!(
                handle_key_with_bindings(key, None, mode),
                Some(InputAction::ToggleTasksList),
                "lowercase t must not open the tasks overlay in {mode:?}"
            );
        }
    }

    /// `Ctrl+T` must be the *only* spelling that opens the pending-work
    /// overlay.
    ///
    /// D-C7 retired the in-place todo-strip toggle rather than leaving it
    /// reachable by a second, undiscoverable spelling. If a future change adds
    /// another chord for `ToggleTasksList` — or re-binds a shifted capital to
    /// it — the composer breaks again for whichever letter it claimed, and the
    /// binding table is the only place that is visible.
    #[test]
    fn ctrl_t_is_the_only_spelling_of_the_tasks_overlay() {
        for vim_mode in [false, true] {
            let map = build_bindings(None, vim_mode);
            let spellings: Vec<(KeyModifiers, KeyCode)> = map
                .iter()
                .filter(|(_, action)| **action == InputAction::ToggleTasksList)
                .map(|(key, _)| *key)
                .collect();
            assert_eq!(
                spellings,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('t'))],
                "vim_mode={vim_mode}: the overlay must have exactly one spelling, \\
                 Ctrl+T; found {spellings:?}"
            );
        }
        // The compact todo strip is not an action at all any more: it is a row
        // derived from the todo list, and the overlay is its full view. The
        // action keeps its configurable name so existing keybind configs keep
        // loading.
        assert_eq!(
            ActionKey::from_input_action(&InputAction::ToggleTasksList),
            Some(ActionKey::ToggleTasksList),
            "the tasks overlay keeps its configurable action name"
        );
    }

    #[test]
    fn ctrl_b_toggles_sidebar_in_both_maps_and_modes() {
        for insert in [false, true] {
            let key = make_key(KeyCode::Char('b'), KeyModifiers::CONTROL);
            let mode = if insert {
                InputMode::Insert
            } else {
                InputMode::Normal
            };
            assert_eq!(
                handle_key_with_bindings(key, None, mode),
                Some(InputAction::ToggleSidebar),
                "Ctrl+B must toggle the sidebar (insert={insert})"
            );
        }
    }

    #[test]
    fn ctrl_t_is_not_still_bound_to_the_sidebar() {
        // The sidebar moved to Ctrl+B so Ctrl+T could own the todo list; a
        // stale duplicate mapping would silently undo that.
        for insert in [false, true] {
            let key = make_key(KeyCode::Char('t'), KeyModifiers::CONTROL);
            let mode = if insert {
                InputMode::Insert
            } else {
                InputMode::Normal
            };
            assert_ne!(
                handle_key_with_bindings(key, None, mode),
                Some(InputAction::ToggleSidebar),
                "Ctrl+T must not still reach ToggleSidebar (insert={insert})"
            );
        }
    }

    #[test]
    fn test_esc_returns_cancel_in_insert_mode() {
        let key = make_key(KeyCode::Esc, KeyModifiers::NONE);
        let result = handle_key_with_bindings(key, None, InputMode::Insert);
        assert_eq!(result, Some(InputAction::Cancel));
    }

    #[test]
    fn configurable_action_catalog_is_exhaustive() {
        let descriptors = ActionKey::descriptors();
        assert_eq!(descriptors.len(), ActionKey::all().len());
        assert!(descriptors.iter().all(|descriptor| {
            descriptor.key.to_input_action().is_configurable()
                && !descriptor.label.is_empty()
                && !descriptor.description.is_empty()
        }));
        assert!(!InputAction::Char('x').is_configurable());
    }

    /// `ActionKey::all()` is a hand-maintained list, so a new configurable
    /// variant added to the enum but forgotten there would compile fine and
    /// silently disappear from the keybind editor. Cross-check the list against
    /// the serde round-trip of every action name the editor accepts.
    #[test]
    fn action_key_all_lists_every_configurable_action() {
        let all = ActionKey::all();
        let mut listed: BTreeSet<String> = BTreeSet::new();
        for key in all {
            assert!(
                listed.insert(serde_json::to_string(key).expect("ActionKey serializes")),
                "duplicate ActionKey in all(): {key:?}"
            );
        }
        assert_eq!(listed.len(), all.len(), "ActionKey::all() has duplicates");

        // Every configurable `InputAction` must appear in `all()`. Compare the
        // number of distinct actions the list yields against the full set of
        // descriptors, which is derived from the enum itself.
        let from_all: BTreeSet<String> = all
            .iter()
            .map(|k| format!("{:?}", k.to_input_action()))
            .collect();
        assert_eq!(
            from_all.len(),
            ActionKey::descriptors().len(),
            "ActionKey::all() does not cover every configurable action"
        );
    }

    /// A built-in `Shift+<letter>` binding must never fire while the user is
    /// composing. A capital letter is text in the middle of a word, so firing
    /// an action swallows the character and hijacks the prompt.
    ///
    /// This is written as a behavioural sweep over **both** keymaps rather
    /// than as a handful of named cases because the failure is not specific.
    /// `Shift+F` (project picker) shipped broken this way in the *default*
    /// configuration, because the guard that was supposed to prevent it was
    /// gated on `bindings.is_none()` and `App::on_key` never passes `None`.
    /// `Shift+W` (Workspace) had the same hole. Naming them would have caught
    /// only the second, and only after someone found the first by hand.
    #[test]
    fn builtin_shift_letters_are_composed_text_in_insert_mode() {
        let mut seen: BTreeSet<char> = BTreeSet::new();
        for vim_mode in [false, true] {
            let map = build_bindings(None, vim_mode);
            for letter in 'A'..='Z' {
                if !map.contains_key(&(KeyModifiers::SHIFT, KeyCode::Char(letter))) {
                    continue;
                }
                seen.insert(letter);
                let resolved = handle_key_with_bindings(
                    KeyEvent::new(KeyCode::Char(letter), KeyModifiers::SHIFT),
                    Some(&map),
                    InputMode::Insert,
                );
                assert!(
                    matches!(resolved, Some(InputAction::Char(_)) | None),
                    "Shift+{letter} must compose in Insert (vim_mode={vim_mode}), \
                     got {resolved:?}"
                );
            }
        }
        // The sweep must not be vacuous: assert it actually inspected the
        // mnemonic letters, so dropping a binding cannot make it pass.
        // `G` is here because it is vim-only — it is precisely the letter a
        // default-map-only check would have missed.
        assert_eq!(
            seen,
            ['F', 'G', 'H', 'J', 'K', 'L', 'N', 'P', 'Q', 'S', 'W', 'X']
                .into_iter()
                .collect::<BTreeSet<char>>(),
            "the sweep must observe every built-in Shift-letter mnemonic"
        );
    }

    /// The sweep above must not have been bought by making text unwinnable.
    /// An explicit user binding for a shifted capital is still an explicit
    /// request, and it must still fire in Insert.
    #[test]
    fn a_user_binding_for_a_shift_letter_still_fires_in_insert_mode() {
        let mut map = build_bindings(None, false);
        map.insert(
            (KeyModifiers::SHIFT, KeyCode::Char('Q')),
            InputAction::ToggleSidebar,
        );
        assert_eq!(
            handle_key_with_bindings(
                KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT),
                Some(&map),
                InputMode::Insert,
            ),
            Some(InputAction::ToggleSidebar),
            "a custom Shift+<letter> binding must still win over composed text"
        );
    }

    /// Regression: `Shift+G` was bound to `GoToBottom` in the Insert map, but
    /// bare-char suppression only skips `KeyModifiers::NONE` bindings, so
    /// capital `G` scrolled the viewport instead of reaching the prompt.
    ///
    /// Worth keeping narrower than it looks: it passes `bindings: None`, so it
    /// only ever exercises the `is_default_map` path. That was enough while
    /// the guard was gated on it, and silently insufficient once it was not —
    /// the vim map, where `Shift+G` actually lives, went untested the whole
    /// time. `builtin_shift_letters_are_composed_text_in_insert_mode` is the
    /// one that passes a real map.
    #[test]
    fn shift_g_inserts_char_in_insert_mode() {
        let key = make_key(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(
            handle_key_with_bindings(key, None, InputMode::Insert),
            Some(InputAction::Char('G'))
        );
    }

    /// `GoToBottom` must remain reachable after moving it off `Shift+G`.
    #[test]
    fn go_to_bottom_reachable_from_insert_map() {
        let bindings = build_bindings(None, false);
        assert!(
            bindings.values().any(|a| *a == InputAction::GoToBottom),
            "GoToBottom lost its default binding in the Insert map"
        );
    }

    /// Invariant **I6**: every advertised chord resolves to the action it
    /// advertises, in the mode it is advertised under.
    ///
    /// The previous version of this guard asserted only that an advertised key
    /// resolved to *something*. That is what let a live `Ctrl+T` row keep
    /// advertising "Toggle sidebar" — the sidebar moved to `Ctrl+B` — while
    /// the key actually performed the todo toggle. A key that is bound to the
    /// wrong thing is exactly as broken as a key that is unbound, and the
    /// guard could not tell the difference.
    ///
    /// The comparison is against [`ActionKey::label`] — the same canonical
    /// naming authority that already produces `ActionDescriptor` — rather than
    /// against a second hand-kept table. Asserting against a second list would
    /// relocate the drift instead of removing it.
    ///
    /// Two exemptions, both deliberate:
    ///
    /// - A **conditional** entry (`condition: Some(..)`) describes behaviour
    ///   that depends on where focus is — "vim mode", "sidebar focus", "some
    ///   dialogs" — so its prose is deliberately more specific than the
    ///   action's name. It is still required to resolve.
    /// - A key that resolves to [`InputAction::Char`] is *typing*, not an
    ///   advertised action; `/` and `?` are documented as text.
    #[test]
    fn documented_keys_resolve_in_the_active_map() {
        // Collect every violation before asserting. A guard that stops at the
        // first mismatch turns a five-row help table into five runs, which is
        // how a check like this quietly stops being run.
        let mut unbound: Vec<String> = Vec::new();
        let mut mislabelled: Vec<String> = Vec::new();
        for mode in [HelpMode::Normal, HelpMode::Insert] {
            for vim_mode in [false, true] {
                let active = build_bindings(None, vim_mode);
                for entry in help_entries_for_mode(mode) {
                    // Same gating `build_help_lines` applies: an entry whose
                    // condition mentions vim mode cannot fire without it.
                    if !vim_mode && entry.condition.is_some_and(|c| c.contains("vim mode")) {
                        continue;
                    }
                    let Some((modifiers, code)) = parse_key(&entry.key.to_lowercase()) else {
                        // Descriptive/compound labels ("j/k", "h/l or ...") are
                        // not a single physical key and cannot be audited here.
                        continue;
                    };
                    // Terminals differ in how they report an uppercase letter:
                    // some send `Char('W')` with SHIFT, others `Char('w')` with
                    // SHIFT, and the maps store either form. Try the plausible
                    // spellings so this audits the binding table rather than
                    // terminal trivia.
                    let spellings = key_spellings(modifiers, code);
                    let input_mode = match mode {
                        HelpMode::Normal => InputMode::Normal,
                        _ => InputMode::Insert,
                    };
                    let mut resolved: Vec<InputAction> = Vec::new();
                    for (mods, candidate) in spellings {
                        if let Some(action) = handle_key_with_bindings(
                            KeyEvent::new(candidate, mods),
                            Some(&active),
                            input_mode,
                        ) {
                            if !resolved.contains(&action) {
                                resolved.push(action);
                            }
                        }
                    }
                    if resolved.is_empty() {
                        unbound.push(format!(
                            "{mode:?} {:?} ({:?}, vim_mode={vim_mode}) is unbound",
                            entry.key, entry.action
                        ));
                        continue;
                    }
                    // Conditional rows describe behaviour that depends on
                    // where focus is, so their prose is deliberately more
                    // specific than the action's name. They still had to
                    // resolve, which is checked above.
                    if entry.condition.is_some() {
                        continue;
                    }
                    if !resolved.iter().any(|a| advertises(entry.action, a)) {
                        mislabelled.push(format!(
                            "{mode:?} {:?} advertises {:?} but performs {} \
                             (vim_mode={vim_mode})",
                            entry.key,
                            entry.action,
                            resolved
                                .iter()
                                .map(|a| format!("{a:?}"))
                                .collect::<Vec<_>>()
                                .join(" or "),
                        ));
                    }
                }
            }
        }
        assert!(
            unbound.is_empty() && mislabelled.is_empty(),
            "help rows that do not match the binding table:\n  unbound: {:#?}\n  \
             mislabelled: {:#?}",
            unbound,
            mislabelled
        );
    }

    /// The spellings a shifted letter may plausibly arrive as.
    ///
    /// Some terminals send `Char('W') + SHIFT`, others `Char('w') + SHIFT`, and
    /// legacy shifted-capital bindings are stored under `NONE` as well as
    /// `SHIFT`. Auditing one spelling would audit terminal trivia instead of
    /// the binding table.
    fn key_spellings(modifiers: KeyModifiers, code: KeyCode) -> Vec<(KeyModifiers, KeyCode)> {
        let mut spellings: Vec<(KeyModifiers, KeyCode)> = vec![(modifiers, code)];
        if let KeyCode::Char(c) = code {
            if c.is_ascii_alphabetic() {
                let upper = KeyCode::Char(c.to_ascii_uppercase());
                spellings.push((modifiers, upper));
                if !modifiers.contains(KeyModifiers::SHIFT) {
                    spellings.push((modifiers | KeyModifiers::SHIFT, upper));
                }
            }
        }
        spellings
    }

    /// Whether `advertised` help prose names `action`.
    ///
    /// Compares on the canonical label with any trailing key hint stripped
    /// ("Open panel manager (Shift+S)" -> "Open panel manager") so the help
    /// table is free to spell the hint out or leave it to the key column
    /// beside it. Substring rather than equality: rows legitimately carry a
    /// qualifier ("Newline in prompt", not "Newline").
    fn advertises(advertised: &str, action: &InputAction) -> bool {
        // A typed character is text, not an advertised action.
        let Some(key) = ActionKey::from_input_action(action) else {
            return true;
        };
        let label = key.label();
        let core = label.split(" (").next().unwrap_or(label).trim();
        advertised
            .to_ascii_lowercase()
            .contains(&core.to_ascii_lowercase())
    }

    /// Proves the I6 assertion is not vacuous.
    ///
    /// A guard that cannot fail is worse than no guard, because it reads as
    /// coverage. `check_daemon_cwd_usage.py` sets the precedent in this repo:
    /// prove the check fails on a deliberately wrong input rather than
    /// asserting that it exists.
    #[test]
    fn i6_advertisement_check_rejects_a_mislabelled_action() {
        // The real defect this guard now catches: `Ctrl+T` performs the tasks
        // toggle, and this is the sidebar's label.
        assert!(
            !advertises("Toggle sidebar", &InputAction::ToggleTasksList),
            "advertises() must reject prose naming a different action"
        );
        assert!(
            advertises(
                "Toggle tasks (todos, subagents, jobs)",
                &InputAction::ToggleTasksList
            ),
            "advertises() must accept prose naming the action"
        );
        // Typed characters are exempt by construction, not by omission.
        assert!(advertises("Type ?", &InputAction::Char('?')));
    }

    #[test]
    fn custom_bindings_round_trip_new_editor_actions() {
        let config: KeybindConfig = serde_json::from_str(
            r#"{"bindings":{"ctrl+e":"external_editor","ctrl+o":"open_diff"}}"#,
        )
        .expect("action keys deserialize");
        let bindings = build_bindings(Some(&config), false);
        assert_eq!(
            bindings.get(&(KeyModifiers::CONTROL, KeyCode::Char('e'))),
            Some(&InputAction::ExternalEditor)
        );
        assert_eq!(
            bindings.get(&(KeyModifiers::CONTROL, KeyCode::Char('o'))),
            Some(&InputAction::OpenDiff)
        );
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
            keys.contains(&"Ctrl+Alt+F"),
            "missing Ctrl+Alt+F in insert help"
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

        // In normal mode, bare '?' names the action it performs. The canonical
        // string is `ActionKey::Help`'s label, so the help table and the
        // binding table cannot call the same key by two different names.
        let entries = help_entries_for_mode(HelpMode::Normal);
        let q_entry = entries.iter().find(|e| e.key == "?");
        assert!(q_entry.is_some(), "normal mode should document '?'");
        assert_eq!(
            q_entry.unwrap().action,
            ActionKey::Help.label(),
            "normal mode '?' must use the canonical label"
        );

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

//! Pluggable side-panel host.
//!
//! The right-hand column used to be one widget chosen by an `if` in
//! `render_sidebar`: the session sidebar, or — on the Workspace route —
//! project chat. That made "swap the panel" impossible to express, and it
//! meant every panel re-implemented focus, key routing, and its own title
//! bar independently.
//!
//! This module introduces the two pieces that make the region a *host*:
//!
//! * [`SidePanelId`] — the closed set of panels the host can show. Adding a
//!   panel is one enum variant plus one [`SidePanel`] impl; there is no
//!   per-panel `if` left in the render path.
//! * [`SidePanelHost`] — which panel is active, whether it holds keyboard
//!   focus. Which panel is loaded is chosen by the manager box
//!   (`Shift+S`), which is a *dialog* rather than an overlay
//!   drawn inside the column — a box can show what it is about to change,
//!   which a 30-column overlay never could.
//!
//! ## Authority
//!
//! A panel is *presentation plus local intent*, exactly like a `Component`
//! dialog. It renders from already-resolved frontend state, and every
//! mutation it triggers is an emitted [`TuiMsg`] that `App::process_msg`
//! routes into an existing command seam. No panel opens a client, reads a
//! project directory, or decides that an operation succeeded — the same
//! contract `ChatDialog` and `SidebarWidget` already follow.
//!
//! ## Key ownership
//!
//! A focused panel owns *navigation*: arrows, `j`/`k`, paging, channel
//! switching and the panel's own verbs. It consumes every one of those, so a
//! stray `j` can never be read as a keystroke somewhere else.
//!
//! A focused panel does **not** own text. The chat panel is a viewer — it
//! renders messages and has no composer of its own — so a printable key and
//! `Enter` return [`PanelKey::PassThrough`] and land in the bottom composer.
//! That is safe now in a way it was not when this module was written: the
//! composer is the only input surface and it renders an explicit
//! `[BUILD]`/`[CHAT]` target badge, so what happens to the key is decided by
//! a value the user can see before they type, not by which widget happened to
//! be focused. The old rule ("a leak would submit an agent turn") is now a
//! rule about *displayed state* instead of a rule about swallowing keys.
//!
//! `PassThrough` is only legal for a panel that holds no draft of its own. A
//! panel with a text buffer must consume everything, or falling text would be
//! edited in one buffer while rendered in another.

pub mod chat;
pub mod help;
pub mod session;
pub mod terminal;

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};

use crate::tui::app::state::ChatState;
use crate::tui::app::TuiMsg;
use crate::tui::components::sidebar::SidebarWidget;
use crate::tui::input::InputAction;
use crate::tui::theme::Theme;

/// Composer badge for the main agent buffer, shown when no panel owns the
/// keyboard.
///
/// A side panel is a *destination* the user can move into and out of; the main
/// transcript is one too, and it is the one they are in most of the time. It
/// gets a name for the same reason the panels do — the composer's badge has to
/// name every surface it can target, or it names none of them.
pub const MAIN_BUFFER_TITLE: &str = "LLM";

/// A direction the user can move in.
///
/// The four directions are **one gesture family with two meanings**, resolved
/// in a single place ([`crate::tui::commands::panels::move_directionally`]):
/// a dockable box is open, so the direction moves that box into the panel;
/// otherwise the direction selects a pane. A box and its panel therefore
/// cannot disagree about what `L` means, which is the whole point of having
/// one seam instead of a per-surface rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PanelDirection {
    /// How far this direction moves through the loaded panel types.
    ///
    /// Vertical only. In a two-pane layout, `←`/`→` move the *keyboard*
    /// between panes while `↑`/`↓` walk the panel list — each direction does
    /// the one thing that still changes something. When the layout grows a
    /// second column this becomes pure geometry again.
    pub fn panel_step(self) -> i8 {
        match self {
            Self::Up => -1,
            Self::Down => 1,
            Self::Left | Self::Right => 0,
        }
    }
}

/// Resolve a `Shift`-qualified directional gesture: `←/→/↑/↓`, or the vim
/// `H`/`J`/`K`/`L` twins.
///
/// One matcher, shared by every surface. A modal short-circuits key routing
/// before the binding map is consulted, so a box has to recognise the gesture
/// in its own key handler; the main buffer reaches the same actions *through*
/// that handler's `InputAction`. Sharing the matcher is what makes the box and
/// its panel one surface instead of two implementations that happen to agree
/// today.
///
/// Returns `None` for everything else, deliberately including bare arrows and
/// bare lowercase letters: those are within-surface navigation verbs and must
/// keep meaning that.
pub fn direction_of(key: KeyEvent) -> Option<PanelDirection> {
    // A chord is a different gesture. `Ctrl+L` selects a model; letting it
    // also move a pane would make one keystroke do two unrelated things.
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    // Arrows need the qualifier; letters carry it in the character itself.
    let shifted = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Left if shifted => Some(PanelDirection::Left),
        KeyCode::Right if shifted => Some(PanelDirection::Right),
        KeyCode::Up if shifted => Some(PanelDirection::Up),
        KeyCode::Down if shifted => Some(PanelDirection::Down),
        // Uppercase only: a terminal may report `Shift+L` as `Char('L')`
        // with or without SHIFT, but `Char('l')` is a bare `l` — a
        // within-surface vim verb, never the directional gesture.
        KeyCode::Char('H') => Some(PanelDirection::Left),
        KeyCode::Char('J') => Some(PanelDirection::Down),
        KeyCode::Char('K') => Some(PanelDirection::Up),
        KeyCode::Char('L') => Some(PanelDirection::Right),
        _ => None,
    }
}

/// One swappable side panel.
///
/// Adding a panel is an enum variant in [`SidePanelId`], an entry in
/// [`SidePanelId::ALL`], and one impl of this trait. Nothing else in the
/// render or key path needs to know it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidePanelId {
    /// Session projection: goal, plan, todos, file changes, tool programs,
    /// agent runs, convergences. Delegates to [`SidebarWidget`], which owns
    /// its own logical-row focus model.
    Session,
    /// Project chat: browse, compose, edit, reply, redact, mark-read, page,
    /// and switch channels without leaving the side column.
    Chat,
    /// The interactive terminal, driven from the side column with the same
    /// Viewing/Focused split the box uses.
    Terminal,
    /// The keybinding reference. Hosts the same [`HelpDialog`] the box does,
    /// so a filter typed here behaves identically to one typed there.
    Help,
}

impl SidePanelId {
    /// Every registered panel, in manager order.
    pub const ALL: &'static [SidePanelId] = &[
        SidePanelId::Session,
        SidePanelId::Chat,
        SidePanelId::Terminal,
        SidePanelId::Help,
    ];

    /// Short label shown in the title bar and the manager.
    pub fn label(self) -> &'static str {
        match self {
            SidePanelId::Session => "Session",
            SidePanelId::Chat => "Chat",
            SidePanelId::Terminal => "Terminal",
            SidePanelId::Help => "Help",
        }
    }

    /// The composer's destination badge for this panel, e.g. `[CHAT]`.
    ///
    /// Distinct from [`label`](Self::label): that one is prose for the title
    /// bar, this one is the uppercase token the bottom input uses to answer
    /// "what does Enter send this to?". Every panel needs one — a destination
    /// the user cannot name is a destination they cannot trust.
    pub fn title(self) -> &'static str {
        match self {
            SidePanelId::Session => "SESSIONS",
            SidePanelId::Chat => "CHAT",
            SidePanelId::Terminal => "TERMINAL",
            SidePanelId::Help => "HELP",
        }
    }

    /// Position in [`SidePanelId::ALL`].
    pub fn index(self) -> usize {
        match self {
            SidePanelId::Session => 0,
            SidePanelId::Chat => 1,
            SidePanelId::Terminal => 2,
            SidePanelId::Help => 3,
        }
    }

    /// Inverse of [`SidePanelId::index`], wrapping.
    pub fn from_index(index: usize) -> SidePanelId {
        let len = SidePanelId::ALL.len();
        if len == 0 {
            return SidePanelId::Session;
        }
        SidePanelId::ALL[index % len]
    }

    /// Short blurb shown under the manager entry.
    pub fn hint(self) -> &'static str {
        match self {
            SidePanelId::Session => "goal · todos · files",
            SidePanelId::Chat => "messages · channels",
            SidePanelId::Terminal => "shell · commands",
            SidePanelId::Help => "keys · filters",
        }
    }
}

/// Result of routing one key to the active panel.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelKey {
    /// The panel consumed the key and wants this intent applied by
    /// `App::process_msg`, which routes it into an existing command seam.
    Intent(TuiMsg),
    /// The panel consumed the key; it changed presentation only.
    Consumed,
    /// The panel does not own this key — it belongs to the composer.
    ///
    /// Only legal for a panel that holds no draft. See "Key ownership" in the
    /// module docs for why this is now safe where it was not before.
    PassThrough,
}

/// Read-only view of the frontend state a panel may render from, plus the
/// writable handles a panel legitimately needs (the session sidebar it
/// delegates to, and the chat state whose draft mirrors the composer).
///
/// Deliberately narrow: there is no `CoreClient` here, and no path or
/// locator the panel could turn into ambient project authority.
pub struct PanelCtx<'a> {
    pub theme: Arc<Theme>,
    /// Display-only clock for composing-lease expiry. No I/O.
    pub now_ms: i64,
    /// Session projection; the Session panel delegates its focus model here.
    pub session: &'a mut SidebarWidget,
    /// Shared chat projection — the same one `/chat` reads, never a second
    /// cache.
    pub chat: &'a mut ChatState,
    /// The interactive terminal controller — the same views, focus flags and
    /// VT state the `/terminal` box drives.
    ///
    /// Granted for the same reason `chat` is: a panel must be able to render
    /// the surface it hosts, and a second copy of the parser would disagree
    /// with the box about what is on screen. It holds no client and no
    /// locator, so it grants no authority — only presentation state.
    pub terminal: &'a mut crate::tui::interactive_terminal::InteractiveTerminalController,
    /// Global vim setting, so a panel that lists mode-gated keys (Help)
    /// describes the mode actually in force.
    pub vim_mode: bool,
    /// Project the chat panel is bound to, if any. Resolved by the command
    /// layer from the active tab or the Workspace selection; a panel never
    /// picks a project itself.
    pub chat_project: Option<&'a str>,
    /// Region the host was given this frame, for viewport-sized paging.
    pub area: Rect,
    /// Whether the host currently holds keyboard focus.
    pub focused: bool,
    /// The main composer's real input mode.
    ///
    /// The key resolver is invoked for panel keys in the panel's own mode,
    /// not the app's, so without this the host cannot tell a `Shift+H` meant
    /// as "type a capital H while composing" from one meant as "move focus".
    /// `DirectionLeft`/`DirectionRight`/… are Normal-mode gestures by
    /// contract, and this is what enforces that even while a panel is focused.
    pub input_mode: crate::tui::input::InputMode,
}

/// A swappable side panel. See [`SidePanelId`] for the extension procedure.
/// Read-only context for [`SidePanel::composer_hints`].
///
/// Deliberately narrower than [`PanelCtx`]: a hint is display-only, and
/// taking the full panel context would force the caller to hand out
/// `&mut` borrows of the sidebar and chat state just to read a string. The
/// narrow shape also makes the seam cheap enough to call during render.
#[derive(Debug, Clone, Copy)]
pub struct PanelHints<'a> {
    /// Whether the panel currently owns the keyboard.
    pub focused: bool,
    /// Daemon project id the chat panel is bound to, if any.
    pub chat_project: Option<&'a str>,
}

pub trait SidePanel {
    fn id(&self) -> SidePanelId;

    /// Render into `area`, which the host has already cleared of the
    /// previous panel's pixels.
    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect);

    /// Route one key. Called only while the host holds focus, and must
    /// consume every key — see the module docs for why.
    ///
    /// `action` is the binding-resolved form of the same key. Panels that
    /// predate the host and are driven by the shared keybind tables (the
    /// session sidebar) consume it so their behavior is unchanged by the
    /// move; panels that own a bespoke keymap (chat) ignore it and read
    /// `key` directly.
    fn handle_key(
        &mut self,
        key: KeyEvent,
        action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> PanelKey;

    /// Whether the panel claims one of the host's exit keys
    /// (`Esc`/`q`) before the host drops focus.
    ///
    /// A panel with an input mode uses the key to leave that mode — keeping
    /// its buffer — instead, because `q` while composing is a letter, not a
    /// command. The host drops focus only once the panel declines.
    fn owns_exit_key(&self, _key: KeyEvent) -> bool {
        false
    }

    /// The keys that apply to *this* panel, rendered on the shared hint line
    /// directly above the composer while it holds the keyboard.
    ///
    /// This is the generalisation seam. A panel declares the verbs it owns
    /// and the user reads them next to the input they modify, instead of
    /// every surface growing its own footer — and adding the next panel is
    /// still one enum variant, one `SidePanelId::ALL` entry, and one impl.
    /// `None` means "no hints"; the default keeps an existing panel's
    /// behavior unchanged.
    fn composer_hints(&self, _hints: &PanelHints<'_>) -> Option<String> {
        None
    }

    /// Called when this panel becomes active. Default: nothing.
    fn on_activate(&mut self, _ctx: &mut PanelCtx<'_>) {}

    /// Called when this panel stops being active. Default: nothing.
    fn on_deactivate(&mut self, _ctx: &mut PanelCtx<'_>) {}
}

/// Where the main composer delivers its text on Enter.
///
/// The composer is the single input surface for both destinations, so the
/// only thing standing between "I meant this for the agent" and "I sent my
/// prompt to the team chat" is whether the user can see which one is armed.
/// This is a *derived* value rather than stored state: the panel host
/// already knows which panel is active and whether it holds the keyboard,
/// and a second copy of that fact could drift out of sync with the badge
/// that is supposed to be reporting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputTarget {
    /// The agent session. The default, and what the composer targets
    /// whenever no panel owns the keyboard.
    #[default]
    Build,
    /// The project chat. Armed only while the Chat panel itself holds
    /// focus, so moving focus away re-arms Build automatically.
    Chat,
}

impl InputTarget {
    /// The badge label rendered next to `[NOR]`/`[INS]` in the composer.
    pub fn label(self) -> &'static str {
        match self {
            InputTarget::Build => "BUILD",
            InputTarget::Chat => "CHAT",
        }
    }
}

/// Which panel the side column shows, and whether it holds focus.
pub struct SidePanelHost {
    active: SidePanelId,
    focused: bool,
    session: session::SessionPanel,
    chat: chat::ChatPanel,
    terminal: terminal::TerminalPanel,
    help: help::HelpPanel,
}

impl Default for SidePanelHost {
    fn default() -> Self {
        Self::new()
    }
}

impl SidePanelHost {
    pub fn new() -> Self {
        Self {
            // Session is the default because it is the panel every route
            // can render; chat needs a bound project and degrades to an
            // explicit "no project selected" until one exists.
            active: SidePanelId::Session,
            focused: false,
            session: session::SessionPanel,
            chat: chat::ChatPanel::default(),
            terminal: terminal::TerminalPanel::default(),
            help: help::HelpPanel::default(),
        }
    }

    pub fn active(&self) -> SidePanelId {
        self.active
    }

    pub fn is_focused(&self) -> bool {
        self.focused && !self.panel_is_empty()
    }

    /// Where the composer sends while this host owns the state.
    ///
    /// Only the Chat panel redirects delivery; every other panel leaves the
    /// composer aimed at the agent session, so focusing Session cannot
    /// silently retarget a half-typed prompt.
    pub fn input_target(&self) -> InputTarget {
        if self.is_focused() && self.active == SidePanelId::Chat {
            InputTarget::Chat
        } else {
            InputTarget::Build
        }
    }

    /// Badge title for whatever currently owns the composer.
    ///
    /// Derived from the same focus state as [`input_target`](Self::input_target)
    /// rather than stored, so the badge cannot drift from the surface it is
    /// reporting. Note this is *narrower* than the delivery target: selecting
    /// the Session panel still delivers to the agent, but the badge reads
    /// `[SESSIONS]` because that is the panel the keyboard is now in — the
    /// user needs to know which panel they are driving, and both facts are
    /// true at once.
    pub fn destination_title(&self) -> &'static str {
        if self.is_focused() {
            self.active.title()
        } else {
            MAIN_BUFFER_TITLE
        }
    }

    /// The contextual key line for whatever currently owns the keyboard.
    ///
    /// `None` when no panel is focused: the composer is aimed at the agent
    /// session, and its hints belong to the session surface, not to a panel
    /// the user is not in.
    /// Selected-message index of the chat panel.
    ///
    /// Exposed so a caller can describe chat position (and so the parity
    /// tests can observe the cursor without reaching through a private
    /// panel field). Meaningless while the chat panel is not the active one.
    pub fn chat_cursor(&self) -> usize {
        self.chat.cursor()
    }

    pub fn composer_hints(&self, chat_project: Option<&str>) -> Option<String> {
        if !self.is_focused() {
            return None;
        }
        let hints = PanelHints {
            focused: true,
            chat_project,
        };
        // Read-only dispatch: a hint must not require an exclusive borrow of
        // the panel, or the composer row could not ask for it during render.
        match self.active {
            SidePanelId::Session => self.session.composer_hints(&hints),
            SidePanelId::Chat => self.chat.composer_hints(&hints),
            SidePanelId::Terminal => self.terminal.composer_hints(&hints),
            SidePanelId::Help => self.help.composer_hints(&hints),
        }
    }

    /// Chat panel's composer buffer, so a keystroke can mirror it into the
    /// shared per-project draft.
    pub fn chat_panel_mut(&mut self) -> &mut chat::ChatPanel {
        &mut self.chat
    }

    /// Drop the chat panel's per-view state.
    ///
    /// Called when the chat projection is cleared (revocation, project
    /// switch) so the composer buffer cannot survive the data it was
    /// composed against.
    pub fn reset_chat_panel(&mut self) {
        self.chat = chat::ChatPanel::default();
    }

    /// The terminal panel, so the command layer can point it at a handle and
    /// learn when that handle goes away. Panels stay free of app access; this
    /// is the same narrow grant the chat panel already has.
    pub fn terminal_panel_mut(&mut self) -> &mut terminal::TerminalPanel {
        &mut self.terminal
    }

    /// The help panel, so parity tests can read the filter and mode the panel
    /// is showing without having to drive a render.
    pub fn help_panel(&self) -> &help::HelpPanel {
        &self.help
    }

    /// The handle the terminal panel is showing, if any. Used to resolve
    /// "whichever terminal the user is looking at" for a command that takes
    /// an optional handle.
    pub fn terminal_handle(&self) -> Option<&str> {
        self.terminal.handle()
    }

    /// Adopt `handle` in the terminal panel. Called after a terminal is
    /// created or attached so docking `/terminal` shows something.
    pub fn show_terminal(&mut self, handle: Option<&str>) {
        self.terminal.show(handle);
    }

    /// Drop the terminal panel's handle when its view disappears.
    pub fn forget_terminal(&mut self, handle: &str) {
        self.terminal.forget(handle);
    }

    /// True when the active panel has nothing to show. Focus is refused
    /// rather than stranding keys in a panel that cannot act on them.
    fn panel_is_empty(&self) -> bool {
        match self.active {
            SidePanelId::Session => false,
            SidePanelId::Chat => self.chat.is_unbound(),
            // The terminal panel is always actionable: `i` takes the keyboard
            // of a terminal that exists, or starts a shell when there is
            // none, so refusing focus would only strand the keyboard on the
            // one surface that can fix it.
            SidePanelId::Terminal => self.terminal.is_empty(),
            // Help always has rows, so it can always take focus.
            SidePanelId::Help => false,
        }
    }

    fn panel(&mut self) -> &mut dyn SidePanel {
        match self.active {
            SidePanelId::Session => &mut self.session,
            SidePanelId::Chat => &mut self.chat,
            SidePanelId::Terminal => &mut self.terminal,
            SidePanelId::Help => &mut self.help,
        }
    }

    /// Switch to `id`. The previous panel is deactivated and the new one
    /// activated; focus follows only if it already had it, so selecting a
    /// panel from an unfocused state does not silently capture the composer.
    pub fn select(&mut self, id: SidePanelId, ctx: &mut PanelCtx<'_>) {
        if id == self.active {
            self.panel().on_activate(ctx);
            return;
        }
        self.panel().on_deactivate(ctx);
        self.active = id;
        // Activate *before* asking whether the panel is empty. A panel that
        // adopts its content on activation — the Terminal panel adopts the
        // controller's active handle — would otherwise be judged empty for
        // exactly one frame, refuse focus, and the first `/terminal` dock
        // would land unfocused.
        self.panel().on_activate(ctx);
        // A panel that still cannot act (chat with no bound project, no
        // terminal yet) must not appear to hold focus.
        if self.panel_is_empty() {
            self.focused = false;
        }
    }

    /// Step `delta` panels, wrapping. Selecting from an unfocused host
    /// leaves it unfocused; `Shift+S` or a focus action drives focus
    /// explicitly.
    pub fn step(&mut self, delta: i8, ctx: &mut PanelCtx<'_>) {
        let len = SidePanelId::ALL.len();
        if len == 0 {
            return;
        }
        let current = self.active.index() as i64;
        let next = (current + i64::from(delta)).rem_euclid(len as i64) as usize;
        let was_focused = self.focused;
        self.select(SidePanelId::from_index(next), ctx);
        self.focused = was_focused && !self.panel_is_empty();
    }

    /// Take keyboard focus. Returns `false` when the active panel cannot
    /// act, leaving focus with the composer.
    pub fn focus(&mut self) -> bool {
        if self.panel_is_empty() {
            self.focused = false;
            return false;
        }
        self.focused = true;
        true
    }

    /// Return keyboard focus to the composer.
    pub fn blur(&mut self) {
        self.focused = false;
    }

    /// Route one key while the host holds focus.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> Option<PanelKey> {
        if self.panel_is_empty() {
            return None;
        }
        // `Ctrl+B` hides the column this panel lives in, so it is the only
        // key that must work *from* inside a focused panel. Answering it here
        // is what stops the toggle from working exactly once: the first
        // press focuses a panel, and because a focused panel consumes every
        // key, the second press was swallowed by the panel that press had
        // just focused — leaving the column stuck open and the keyboard
        // stranded. The intent is routed through `App::process_msg` so the
        // toggle has exactly one implementation.
        if action == Some(InputAction::ToggleSidebar) {
            return Some(PanelKey::Intent(TuiMsg::ToggleSidebar));
        }
        // The directions are answered here, before the panel, for the same
        // reason: they are the only way out of a focused panel, and a panel
        // that swallowed them would trap the keyboard. Answered as
        // `MoveDirection` rather than as a resolved focus change so the
        // contextual dispatcher still sees the whole picture — including the
        // fact that no box is open, which is what makes this a pane move.
        if ctx.input_mode == crate::tui::input::InputMode::Normal {
            let dir = match action {
                Some(InputAction::DirectionLeft) => Some(PanelDirection::Left),
                Some(InputAction::DirectionRight) => Some(PanelDirection::Right),
                Some(InputAction::DirectionUp) => Some(PanelDirection::Up),
                Some(InputAction::DirectionDown) => Some(PanelDirection::Down),
                _ => None,
            };
            if let Some(dir) = dir {
                return Some(PanelKey::Intent(TuiMsg::MoveDirection(dir)));
            }
        }
        if !self.focused {
            return None;
        }
        // `Esc`/`q` are the universal exits and are handled by the host so
        // they do not have to be special-cased per panel. The panel gets
        // first refusal: a chat panel in Compose mode uses `Esc` to return
        // to Normal with its buffer intact before focus leaves.
        if is_exit_key(key) {
            if self.panel().owns_exit_key(key) {
                self.panel().handle_key(key, action, ctx);
            } else {
                self.blur();
            }
            return Some(PanelKey::Consumed);
        }
        Some(self.panel().handle_key(key, action, ctx))
    }

    /// Paint the active panel.
    pub fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        self.panel().render(ctx, buf, area);
    }
}

/// Whether a key is one of the host-level exits that returns focus to the
/// composer.
///
/// Both are unmodified single keys. Modified variants are excluded so a
/// `Ctrl+Q`-style shortcut, or a modifier the terminal reports alongside a
/// printable key, cannot drop focus out from under a panel mid-edit.
fn is_exit_key(key: KeyEvent) -> bool {
    if key.modifiers != KeyModifiers::NONE {
        return false;
    }
    matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_indexes_round_trip() {
        for (index, id) in SidePanelId::ALL.iter().copied().enumerate() {
            assert_eq!(id.index(), index);
            assert_eq!(SidePanelId::from_index(index), id);
            assert!(!id.label().is_empty());
        }
    }

    #[test]
    fn from_index_wraps_instead_of_panicking() {
        let len = SidePanelId::ALL.len();
        assert_eq!(SidePanelId::from_index(len), SidePanelId::ALL[0]);
        assert_eq!(SidePanelId::from_index(len + 3), SidePanelId::ALL[3 % len]);
    }

    // The directional matcher itself is exercised end-to-end in
    // `tests/tui_panel_docking.rs`; what matters *here* is that the registry
    // stays internally consistent.
    #[test]
    fn the_registry_is_internally_consistent() {
        assert!(
            SidePanelId::ALL.len() >= 2,
            "a one-panel registry makes the vertical direction inert"
        );
        for panel in SidePanelId::ALL.iter().copied() {
            assert!(!panel.label().is_empty());
            assert!(!panel.title().is_empty());
            assert!(!panel.hint().is_empty());
        }
    }
}

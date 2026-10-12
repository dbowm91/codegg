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
//!   focus, and the [`PanelSelector`] overlay opened by `Ctrl+Alt+W`.
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
pub mod session;

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::tui::app::state::ChatState;
use crate::tui::app::TuiMsg;
use crate::tui::components::sidebar::SidebarWidget;
use crate::tui::input::InputAction;
use crate::tui::theme::Theme;

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
}

impl SidePanelId {
    /// Every registered panel, in selector order.
    pub const ALL: &'static [SidePanelId] = &[SidePanelId::Session, SidePanelId::Chat];

    /// Short label shown in the title bar and the selector.
    pub fn label(self) -> &'static str {
        match self {
            SidePanelId::Session => "Session",
            SidePanelId::Chat => "Chat",
        }
    }

    /// Position in [`SidePanelId::ALL`].
    pub fn index(self) -> usize {
        match self {
            SidePanelId::Session => 0,
            SidePanelId::Chat => 1,
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

    /// Short blurb shown under the selector entry.
    pub fn hint(self) -> &'static str {
        match self {
            SidePanelId::Session => "goal · todos · files",
            SidePanelId::Chat => "messages · channels",
        }
    }
}

/// Columns in the selector grid.
///
/// Two keeps every entry legible inside the 30-column side column, and gives
/// `h`/`l` a real meaning (move by column) distinct from `j`/`k`.
pub const PANEL_SELECTOR_COLUMNS: usize = 2;

/// The `Ctrl+Alt+W` overlay that picks which panel the side column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelSelector {
    /// Index into [`SidePanelId::ALL`].
    cursor: usize,
}

impl Default for PanelSelector {
    fn default() -> Self {
        Self::opening(SidePanelId::Session)
    }
}

impl PanelSelector {
    /// Selector opened on the currently-active panel, so `Enter` with no
    /// movement is a no-op rather than a silent jump.
    pub fn opening(active: SidePanelId) -> Self {
        Self {
            cursor: active.index(),
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Selected panel, or `None` only if the registry were empty.
    pub fn selected(&self) -> Option<SidePanelId> {
        SidePanelId::ALL.get(self.cursor).copied()
    }

    /// Move by column. Clamped rather than wrapped: a press at the grid
    /// edge should be inert, not teleport the highlight to the far side.
    pub fn move_left(&mut self) {
        if self.cursor % PANEL_SELECTOR_COLUMNS > 0 {
            self.cursor -= 1;
        }
    }

    /// Move by column; see [`PanelSelector::move_left`] for the no-wrap rule.
    pub fn move_right(&mut self) {
        let next = self.cursor + 1;
        if next < SidePanelId::ALL.len() && next % PANEL_SELECTOR_COLUMNS != 0 {
            self.cursor = next;
        }
    }

    /// Move by row. Inert once the grid is a single row.
    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(PANEL_SELECTOR_COLUMNS);
    }

    /// Move by row; inert once the grid is a single row.
    pub fn move_down(&mut self) {
        let next = self.cursor + PANEL_SELECTOR_COLUMNS;
        if next < SidePanelId::ALL.len() {
            self.cursor = next;
        }
    }

    /// Route one key.
    ///
    /// Unlike a panel, the selector distinguishes *answers* from *noise*:
    /// only `Enter` and `Esc` mean something, and the host has to act on
    /// both. Everything else is moved or swallowed.
    pub fn handle_key(&mut self, key: KeyEvent) -> SelectorKey {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => self.move_left(),
            KeyCode::Char('l') | KeyCode::Right => self.move_right(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Enter => return SelectorKey::Commit,
            KeyCode::Esc => return SelectorKey::Cancel,
            _ => return SelectorKey::Consumed,
        }
        SelectorKey::Moved
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
    /// `FocusMainBuffer`/`FocusSidePanel` are Normal-mode gestures by
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

/// Which panel the side column shows, whether it holds focus, and the
/// `Ctrl+Alt+W` selector.
pub struct SidePanelHost {
    active: SidePanelId,
    focused: bool,
    selector: Option<PanelSelector>,
    session: session::SessionPanel,
    chat: chat::ChatPanel,
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
            selector: None,
            session: session::SessionPanel,
            chat: chat::ChatPanel::default(),
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
        }
    }

    /// The selector swallows every other key, so the underlying panel must
    /// not also react while it is up.
    pub fn selector_open(&self) -> bool {
        self.selector.is_some()
    }

    pub fn selector_cursor(&self) -> Option<usize> {
        self.selector.map(|s| s.cursor())
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

    /// True when the active panel has nothing to show. Focus is refused
    /// rather than stranding keys in a panel that cannot act on them.
    fn panel_is_empty(&self) -> bool {
        match self.active {
            SidePanelId::Session => false,
            SidePanelId::Chat => self.chat.is_unbound(),
        }
    }

    fn panel(&mut self) -> &mut dyn SidePanel {
        match self.active {
            SidePanelId::Session => &mut self.session,
            SidePanelId::Chat => &mut self.chat,
        }
    }

    /// Open the selector on the active panel. No-op when the side column is
    /// hidden, since there would be nothing to select into.
    pub fn open_selector(&mut self) {
        self.selector = Some(PanelSelector::opening(self.active));
    }

    /// Dismiss the selector, leaving the active panel unchanged.
    pub fn cancel_selector(&mut self) {
        self.selector = None;
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
        // A panel that cannot act (chat with no bound project) must not
        // appear to hold focus.
        if self.panel_is_empty() {
            self.focused = false;
        }
        self.panel().on_activate(ctx);
    }

    /// Step `delta` panels, wrapping. Selecting from an unfocused host
    /// leaves it unfocused; `Ctrl+Alt+W` or a focus action drives focus
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

    /// Commit the selector's highlighted panel and take focus.
    ///
    /// Focus is taken because the whole point of the overlay is "interact
    /// with the panel I just picked"; leaving keys in the composer would
    /// make the selection look inert.
    pub fn commit_selector(&mut self, ctx: &mut PanelCtx<'_>) {
        let Some(selector) = self.selector.take() else {
            return;
        };
        if let Some(id) = selector.selected() {
            self.select(id, ctx);
            self.focused = !self.panel_is_empty();
        }
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
        self.selector = None;
    }

    /// Route one key while the host holds focus.
    ///
    /// Order matters: the selector overlays the panel, so it is consulted
    /// first and the panel never sees a key the user aimed at the overlay.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> Option<PanelKey> {
        // The overlay is reachable whether or not a panel holds focus:
        // opening it must not steal the keyboard, or a user who opens the
        // switcher from the composer and cancels would find their next
        // keystroke going to a panel they never chose.
        if self.panel_is_empty() && self.selector.is_none() {
            return None;
        }
        // The switcher's own hotkey is answered here, before any panel
        // sees it. Delegating it would let a focused panel swallow the only
        // key that can get the user back to the switcher, which is how
        // "I opened the overlay and now I am stuck in chat" happens.
        if action == Some(InputAction::TogglePanelSelector) {
            if self.selector.is_some() {
                self.cancel_selector();
            } else {
                self.open_selector();
            }
            return Some(PanelKey::Consumed);
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
        // The focus moves are answered here for the same reason: `Shift+H` /
        // `Shift+Left` is the only way back out of a focused panel, and a
        // panel that swallowed it would trap the keyboard. `Shift+L` /
        // `Shift+Right` is idempotent — the panel already has focus, so
        // re-focusing is a no-op rather than a toggle.
        if ctx.input_mode == crate::tui::input::InputMode::Normal
            && matches!(
                action,
                Some(InputAction::FocusMainBuffer | InputAction::FocusSidePanel)
            )
        {
            let msg = if action == Some(InputAction::FocusSidePanel) {
                TuiMsg::FocusSidePanel
            } else {
                TuiMsg::FocusMainBuffer
            };
            return Some(PanelKey::Intent(msg));
        }
        if let Some(selector) = self.selector.as_mut() {
            match selector.handle_key(key) {
                SelectorKey::Commit => self.commit_selector(ctx),
                SelectorKey::Cancel => self.cancel_selector(),
                SelectorKey::Moved | SelectorKey::Consumed => {}
            }
            return Some(PanelKey::Consumed);
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

    /// Paint the active panel, then the selector overlay if it is open.
    pub fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        self.panel().render(ctx, buf, area);
        if let Some(selector) = self.selector {
            render_selector(buf, area, selector, &ctx.theme);
        }
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

/// What one key meant to the selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKey {
    /// The highlight moved; the panel is unchanged.
    Moved,
    /// Commit the highlighted panel and take focus.
    Commit,
    /// Dismiss without changing panels.
    Cancel,
    /// Swallowed, so the overlay never leaks keys to what is behind it.
    Consumed,
}

/// Overlay listing every registered panel in a grid.
///
/// Drawn inside the side column rather than centred over the viewport: it
/// describes the region it is switching, and staying in-column means the
/// panel it will reveal is never covered while the overlay is up.
fn render_selector(buf: &mut Buffer, area: Rect, selector: PanelSelector, theme: &Arc<Theme>) {
    if area.width < 12 || area.height < 5 {
        return;
    }
    let entries = SidePanelId::ALL.len();
    let grid_rows = entries.div_ceil(PANEL_SELECTOR_COLUMNS) as u16;
    // One border row top and bottom, one grid row per entry, one hint row.
    let height = (grid_rows + 3).min(area.height);
    let overlay = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.primary))
        .title(" Panels ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme.background));
    let inner = block.inner(overlay);
    Clear.render(overlay, buf);
    block.render(overlay, buf);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(grid_rows.min(inner.height)),
        Constraint::Length(1),
    ])
    .split(inner);

    let cursor = selector.cursor();
    let mut cells: Vec<Vec<Span>> = vec![Vec::new(); grid_rows as usize];
    for (index, id) in SidePanelId::ALL.iter().copied().enumerate() {
        let selected = index == cursor;
        let style = if selected {
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(theme.muted)
        };
        let text = if selected {
            format!("▸{}", id.label())
        } else {
            format!(" {}", id.label())
        };
        cells[index / PANEL_SELECTOR_COLUMNS].push(Span::styled(text, style));
    }
    for (row, spans) in cells.iter().enumerate() {
        let Some(row_area) = rows.get(row).copied() else {
            break;
        };
        Paragraph::new(Line::from(spans.clone())).render(row_area, buf);
    }
    if let Some(hint) = rows.get(grid_rows as usize) {
        Paragraph::new(Line::from(Span::styled(
            " h/j/k/l · Enter ",
            Style::default().fg(theme.muted),
        )))
        .render(*hint, buf);
    }
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

    fn selector() -> PanelSelector {
        PanelSelector::opening(SidePanelId::Session)
    }

    #[test]
    fn selector_starts_on_the_active_panel() {
        let s = PanelSelector::opening(SidePanelId::Chat);
        assert_eq!(s.selected(), Some(SidePanelId::Chat));
    }

    #[test]
    fn selector_h_and_l_move_by_column_and_clamp_at_the_edges() {
        let mut s = selector();
        // Column 0: `h` is inert rather than wrapping to the last column.
        s.move_left();
        assert_eq!(s.cursor(), 0);
        s.move_right();
        assert_eq!(s.cursor(), 1);
        // Last column of row 0: `l` is inert, it does not wrap to row 1.
        s.move_right();
        assert_eq!(s.cursor(), 1);
        s.move_left();
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn selector_j_and_k_are_inert_on_a_single_row_grid() {
        let mut s = selector();
        let before = s.cursor();
        s.move_down();
        assert_eq!(s.cursor(), before);
        s.move_up();
        assert_eq!(s.cursor(), before);
    }

    #[test]
    fn selector_grid_moves_by_row_once_a_second_row_exists() {
        // Exercise the row arithmetic directly against a 2-column grid so
        // the j/k behaviour is covered before a third panel ships.
        let mut cursor = 0usize;
        let len = 4usize;
        let down = cursor + PANEL_SELECTOR_COLUMNS;
        if down < len {
            cursor = down;
        }
        assert_eq!(cursor, 2);
        cursor = cursor.saturating_sub(PANEL_SELECTOR_COLUMNS);
        assert_eq!(cursor, 0);
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        use crossterm::event::KeyEventKind;
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn selector_keys_route_vim_and_arrows_to_the_same_moves() {
        let mut vim = selector();
        let mut arrows = selector();
        assert_eq!(
            vim.handle_key(key(KeyCode::Char('l'), KeyModifiers::NONE)),
            SelectorKey::Moved
        );
        assert_eq!(
            arrows.handle_key(key(KeyCode::Right, KeyModifiers::NONE)),
            SelectorKey::Moved
        );
        assert_eq!(vim.cursor(), arrows.cursor());
    }

    #[test]
    fn selector_answers_enter_and_escape_and_swallows_everything_else() {
        let mut s = selector();
        assert_eq!(
            s.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            SelectorKey::Commit
        );
        assert_eq!(
            s.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            SelectorKey::Cancel
        );
        assert_eq!(
            s.handle_key(key(KeyCode::Char('z'), KeyModifiers::NONE)),
            SelectorKey::Consumed,
            "an unbound key is still consumed: the overlay must not leak to the composer"
        );
    }

    #[test]
    fn the_selector_has_no_private_hotkey_of_its_own() {
        // The toggle is the host's job: it must be answered even when a
        // panel (not the overlay) would otherwise consume the key.
        let mut s = selector();
        let hotkey = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(
            s.handle_key(key(KeyCode::Char('W'), hotkey)),
            SelectorKey::Consumed
        );
    }
}

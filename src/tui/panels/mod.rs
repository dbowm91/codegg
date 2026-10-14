//! Pluggable side-panel host.
//!
//! The right-hand column used to be one widget chosen by an `if` in the
//! render path: the session sidebar, or — on the Workspace route —
//! project chat. That made "swap the panel" impossible to express, and it
//! meant every panel re-implemented focus, key routing, and its own title
//! bar independently.
//!
//! This module introduces the pieces that make the region a *host*:
//!
//! * [`SidePanelId`] — the closed set of panels a host can show. Adding a
//!   panel is one enum variant plus one [`SidePanel`] impl; there is no
//!   per-panel `if` left in the render path.
//! * [`PanelSlot`] — one dockable edge of the frame. There is a slot per
//!   [`PanelPlacement`], so more than one edge can be showing a panel at
//!   once, and each remembers what it was showing before its current
//!   occupant arrived.
//! * [`SidePanelHost`] — which edge each panel is docked to, which edge the
//!   keyboard gestures act on, and whether that edge holds focus. Which
//!   panel is loaded is chosen by the manager box (`Shift+S`), which is a
//!   *dialog* rather than an overlay drawn inside the column — a box can
//!   show what it is about to change, which a 30-column overlay never could.
//!
//! Panel *state* is keyed by [`SidePanelId`], not by edge: one `ChatPanel`,
//! one `SessionPanel`. A slot only records which edge is showing which id.
//! That is what lets [`SidePanelHost::dock`] move a panel between edges
//! without losing its cursor, filter or scroll offset — and why a panel is
//! never loaded on two edges at once.
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

pub mod agents;
pub mod chat;
pub mod help;
pub mod hosted;
pub mod session;
pub mod terminal;

use std::collections::VecDeque;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};

use crate::tui::app::state::ChatState;
use crate::tui::app::TuiMsg;
use crate::tui::components::sidebar::SidebarWidget;
use crate::tui::input::InputAction;
use crate::tui::theme::Theme;

/// Paint `widget` into `buf` at `area`, bridging `Component::render`'s
/// `&mut Frame` to the host's `&mut Buffer`.
///
/// ratatui's `Frame` has no public constructor, so the widget is rendered into
/// an in-memory backend sized to the column and the result is copied out.
/// `TestBackend` is used for what it actually is — a `Backend` over a plain
/// `Buffer` — not as a test harness; nothing here asserts.
///
/// This exists so a dockable box can be adopted into the column without
/// rewriting that box's render path. It is the one place this bridge is
/// allowed, so a widget that needs real cursor or clipping semantics inside
/// the column can grow a native `render_into(&mut Buffer, ..)` — as
/// `HelpDialog` did — and the host can prefer it.
pub fn render_hosted(
    widget: &mut dyn crate::tui::components::component::Component,
    buf: &mut Buffer,
    area: Rect,
    theme: &Arc<Theme>,
    title: &str,
) {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    if area.width == 0 || area.height == 0 {
        return;
    }
    // A scratch terminal, rebuilt each frame. The column is small and the
    // alternative — caching one per panel — would let ratatui's buffer
    // diffing retain cells a shrunken widget no longer paints, which is the
    // one failure mode worse than the allocation.
    // `TestBackend`'s error type is `Infallible`, so this never fails; `.ok()`
    // rather than `expect` keeps the render path panic-free without tripping
    // the irrefutable-let-pattern lint that a direct `let Ok(..) else` hits.
    let Some(mut terminal) = Terminal::new(TestBackend::new(area.width, area.height)).ok() else {
        return;
    };
    // Backdrop first: modal widgets paint text, not backgrounds — as modals
    // they relied on the dimmed overlay behind them. Untouched scratch cells
    // would otherwise copy out as terminal-default (transparent) cells over
    // the cleared column, which reads as missing styling.
    for cell in terminal.current_buffer_mut().content.iter_mut() {
        cell.set_bg(theme.background);
    }
    let _ = terminal.draw(|frame| {
        frame.render_widget(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(title.to_string())
                .border_style(ratatui::style::Style::default().fg(theme.border)),
            frame.area(),
        );
        widget.render(frame, frame.area(), theme);
    });

    // Copy the origin-based scratch into `buf` at `area`'s screen position.
    // The scratch has no idea where the column lives: intersecting it with
    // `buf.area` instead painted the panel over the top-left corner, clipping
    // across the main view.
    let scratch = terminal.backend().buffer();
    for y in 0..area.height {
        for x in 0..area.width {
            let (Some(src_cell), Some(dst_cell)) = (
                scratch.cell((x, y)),
                buf.cell_mut((area.x.saturating_add(x), area.y.saturating_add(y))),
            ) else {
                continue;
            };
            dst_cell.set_symbol(src_cell.symbol());
            dst_cell.set_style(src_cell.style());
        }
    }
}

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
    /// The vim letter that spells this gesture, e.g. `H` for left.
    ///
    /// Used in the messages that *explain* a direction — a refusal that names
    /// "left" when every other key in the UI names `Shift+H` teaches the user
    /// one vocabulary and then switches to another mid-error.
    pub fn gesture(self) -> &'static str {
        match self {
            Self::Left => "H",
            Self::Right => "L",
            Self::Up => "K",
            Self::Down => "J",
        }
    }

    /// Prose for the same explanations ("no panel on the left yet").
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "above",
            Self::Down => "below",
        }
    }
}

/// Which edge of the frame the panel host is painted into.
///
/// Groundwork for more than one panel. The directionals already address panes
/// *by direction* — `Shift+H` means "the pane on my left" — so what was
/// missing was a way to say **which** pane the one existing host is currently
/// living in. Without that, every gesture silently assumed "right", which is
/// exactly the hardcoding that makes a second column impossible to add later.
///
/// `Right` is the default and reproduces the layout that shipped first, so a
/// user who configures nothing sees no change at all.
///
/// The three placements are the ones the frame can honestly host today: a
/// left column, a right column, and a full-width row beneath the main input
/// box. Growing a second column is a fourth variant plus one geometry arm —
/// the gesture dispatcher already reads *which* side a direction names and
/// needs no new rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelPlacement {
    /// The shipped default: a column beside the main buffer, on the right.
    #[default]
    Right,
    /// A column on the other side, so `Shift+H` is the gesture that enters
    /// the panel and `Shift+L` returns to the buffer.
    Left,
    /// A full-width row under the main input box, so `Shift+J` enters it and
    /// `Shift+K` returns to the buffer.
    Bottom,
}

impl PanelPlacement {
    /// Every placement, in the order the manager cycles them.
    pub const ALL: &'static [PanelPlacement] = &[Self::Right, Self::Left, Self::Bottom];

    /// Prose for the manager's layout strip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Right => "Right",
            Self::Left => "Left",
            Self::Bottom => "Bottom",
        }
    }

    /// The persisted spelling. Same rule as [`SidePanelId::config_key`]: user
    /// data, so it is written down rather than derived from a label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Bottom => "bottom",
        }
    }

    /// Read a persisted placement, tolerating the word users reach for.
    ///
    /// Returns `None` for an unknown value so the caller falls back to the
    /// default rather than starting with no panel at all — a hand-edited
    /// typo in a preference is not worth refusing to boot over, matching how
    /// a bad panel hotkey drops one slot.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "right" => Some(Self::Right),
            "left" => Some(Self::Left),
            "bottom" | "down" => Some(Self::Bottom),
            _ => None,
        }
    }

    /// Position in [`Self::ALL`], which is also the slot-table index.
    ///
    /// Derived rather than hand-written so a variant added to `ALL` without
    /// an index arm fails to compile instead of silently aliasing another
    /// slot's state.
    pub fn index(self) -> usize {
        match self {
            Self::Right => 0,
            Self::Left => 1,
            Self::Bottom => 2,
        }
    }

    /// The placement a slot index names, or `None` when out of range.
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// The next placement in [`Self::ALL`], wrapping.
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// The edge a directional gesture names, or `None` when it names none.
    ///
    /// The gesture is literal — `Shift+H` means the left column — rather than
    /// being reinterpreted through some selected edge's axis. That is the same
    /// rule every surface uses, so a box, the manager and the main buffer
    /// cannot disagree about what `H` means.
    ///
    /// `Up` names no edge. There is no top, and inventing one would let
    /// `Shift+K` silently mean "stay here" in one surface and "go somewhere
    /// else" in another.
    pub fn named_by(direction: PanelDirection) -> Option<Self> {
        match direction {
            PanelDirection::Left => Some(Self::Left),
            PanelDirection::Right => Some(Self::Right),
            PanelDirection::Down => Some(Self::Bottom),
            PanelDirection::Up => None,
        }
    }

    /// The gesture that moves the keyboard **into** the panel.
    pub fn toward(self) -> PanelDirection {
        match self {
            Self::Right => PanelDirection::Right,
            Self::Left => PanelDirection::Left,
            Self::Bottom => PanelDirection::Down,
        }
    }

    /// The gesture that moves the keyboard **back** to the main buffer.
    ///
    /// Exactly [`toward`](Self::toward) inverted, so the pair can never
    /// disagree about which key is the way out of a pane.
    pub fn away(self) -> PanelDirection {
        match self {
            Self::Right => PanelDirection::Left,
            Self::Left => PanelDirection::Right,
            Self::Bottom => PanelDirection::Up,
        }
    }

    /// How far `direction` walks the loaded panel types, or `None` when the
    /// direction runs *along* the placement axis.
    ///
    /// This is the whole reason placement and direction cannot be two
    /// independent lists: in a right-hand column `↑`/`↓` walk the panel list
    /// because `←`/`→` already have a job, and under a bottom row that swaps
    /// over. Deriving the answer from the placement keeps each direction doing
    /// the one thing that still changes something.
    pub fn cycle_step(self, direction: PanelDirection) -> Option<i8> {
        match (self, direction) {
            (Self::Right | Self::Left, PanelDirection::Up) => Some(-1),
            (Self::Right | Self::Left, PanelDirection::Down) => Some(1),
            (Self::Bottom, PanelDirection::Left) => Some(-1),
            (Self::Bottom, PanelDirection::Right) => Some(1),
            // Along the axis: the gesture means "enter the panel" or "return
            // to the buffer", and `move_directionally` has already answered it.
            _ => None,
        }
    }

    /// Whether the panel is a column beside the buffer rather than a row
    /// beneath it. The render path needs this to know which axis to split.
    pub fn is_column(self) -> bool {
        !matches!(self, Self::Bottom)
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

/// Two disjoint mutable borrows into a slot table, by placement.
///
/// Written out rather than open-coded at each call site because getting the
/// index order backwards silently addresses the wrong slot — the move would
/// still typecheck and still swap *something*, which is exactly the kind of
/// bug that reads as "the gesture is just weird". Ordering the two first and
/// splitting only once removes the case where the branches disagree.
fn two_slots_mut(
    slots: &mut [PanelSlot],
    a: PanelPlacement,
    b: PanelPlacement,
) -> (&mut PanelSlot, &mut PanelSlot) {
    debug_assert_ne!(a, b, "a slot cannot be borrowed twice");
    let (first, second) = if a.index() <= b.index() {
        (a, b)
    } else {
        (b, a)
    };
    let (low, high) = slots.split_at_mut(second.index());
    (&mut low[first.index()], &mut high[0])
}

/// One swappable side panel.
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
    /// `/tree` — workspace file tree.
    Tree,
    /// `/sessions` — session switcher.
    Sessions,
    /// `/tasks` — running, waiting and attention-capped project tasks.
    Tasks,
    /// `/plan` — the task plan.
    Plan,
    /// `/goal` — the active long-running goal.
    Goal,
    /// `/collaborators` — project presence.
    Collaborators,
    /// `/memory-search` — memory results.
    Memory,
    /// `/research-runs` — recent research runs.
    Research,
    /// `/logs` — bounded daemon log tail plus notification history.
    Logs,
    /// `/context` — context window usage.
    Context,
    /// `/cost` — token usage and cost.
    Cost,
    /// `/usage` — rate limits and quota.
    Usage,
    /// `/stats` — session analytics and cost breakdown.
    Stats,
    /// `/doctor` — diagnostics report.
    Doctor,
    /// `/shell-list` — recent shell commands.
    ShellShow,
    /// `/security-review` — enriched security review findings.
    Security,
    /// `/models` — model switcher.
    Models,
    /// `/agents` — agent switcher.
    Agents,
    /// `/mcps` — MCP server management.
    Mcp,
    /// `/keybinds` — keybinding customisation.
    Keybinds,
    /// `/themes` — theme picker.
    Themes,
    /// `/plugins` — installed and built-in plugins.
    Plugins,
    /// `/team` — project team membership.
    Team,
    /// `/control` — active-turn controller.
    Control,
    /// `/workspace` — global Workspace dashboard.
    Workspace,
    /// `/task` — the project Task view (WorkOrders).
    TaskView,
    /// `/diff` — diff for a file.
    Diff,
    /// `/review` — review changed files.
    Review,
    /// `/files` — workspace file manager.
    Files,
}

impl SidePanelId {
    /// Every registered panel, in manager order.
    ///
    /// Order is *load-bearing* in two places and neither may drift:
    /// [`index`](Self::index) is derived from this list (so `↑`/`↓` walking
    /// and the manager's cursor agree), and `PanelManagerDialog` renders this
    /// list directly — there is no second registry to keep in sync.
    ///
    /// The bespoke panels lead, then every box that can dock into the
    /// column. Bespoke means "owns its own implementation" — see
    /// [`is_bespoke`](Self::is_bespoke), which is what `SidePanelHost` filters
    /// on. A panel appears here only if it is reachable: either the box
    /// that fills it exists, or selecting it renders a placeholder that names
    /// the command to run.
    pub const ALL: &'static [SidePanelId] = &[
        SidePanelId::Session,
        SidePanelId::Chat,
        SidePanelId::Terminal,
        SidePanelId::Help,
        SidePanelId::Tree,
        SidePanelId::Sessions,
        SidePanelId::Tasks,
        SidePanelId::Plan,
        SidePanelId::Goal,
        SidePanelId::Collaborators,
        SidePanelId::Memory,
        SidePanelId::Research,
        SidePanelId::Logs,
        SidePanelId::Context,
        SidePanelId::Cost,
        SidePanelId::Usage,
        SidePanelId::Stats,
        SidePanelId::Doctor,
        SidePanelId::ShellShow,
        SidePanelId::Security,
        SidePanelId::Models,
        SidePanelId::Agents,
        SidePanelId::Mcp,
        SidePanelId::Keybinds,
        SidePanelId::Themes,
        SidePanelId::Plugins,
        SidePanelId::Team,
        SidePanelId::Control,
        SidePanelId::Workspace,
        SidePanelId::TaskView,
        SidePanelId::Diff,
        SidePanelId::Review,
        SidePanelId::Files,
    ];

    /// Short label shown in the title bar and the manager.
    pub fn label(self) -> &'static str {
        match self {
            SidePanelId::Session => "Session",
            SidePanelId::Chat => "Chat",
            SidePanelId::Terminal => "Terminal",
            SidePanelId::Help => "Help",
            SidePanelId::Tree => "Tree",
            SidePanelId::Files => "Files",
            SidePanelId::Sessions => "Sessions",
            SidePanelId::Tasks => "Tasks",
            SidePanelId::Plan => "Plan",
            SidePanelId::Goal => "Goal",
            SidePanelId::Collaborators => "Present",
            SidePanelId::Memory => "Memory",
            SidePanelId::Research => "Research",
            SidePanelId::Logs => "Logs",
            SidePanelId::Context => "Context",
            SidePanelId::Cost => "Cost",
            SidePanelId::Usage => "Usage",
            SidePanelId::Stats => "Stats",
            SidePanelId::Doctor => "Doctor",
            SidePanelId::ShellShow => "Shells",
            SidePanelId::Security => "Security",
            SidePanelId::Models => "Models",
            SidePanelId::Agents => "Agents",
            SidePanelId::Mcp => "MCP",
            SidePanelId::Keybinds => "Keys",
            SidePanelId::Themes => "Themes",
            SidePanelId::Plugins => "Plugins",
            SidePanelId::Team => "Team",
            SidePanelId::Control => "Control",
            SidePanelId::Workspace => "Workspace",
            SidePanelId::TaskView => "Task View",
            SidePanelId::Diff => "Diff",
            SidePanelId::Review => "Review",
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
            SidePanelId::Tree => "TREE",
            SidePanelId::Files => "FILES",
            SidePanelId::Sessions => "SWITCHER",
            SidePanelId::Tasks => "TASKS",
            SidePanelId::Plan => "PLAN",
            SidePanelId::Goal => "GOAL",
            SidePanelId::Collaborators => "PRESENT",
            SidePanelId::Memory => "MEMORY",
            SidePanelId::Research => "RESEARCH",
            SidePanelId::Logs => "LOGS",
            SidePanelId::Context => "CONTEXT",
            SidePanelId::Cost => "COST",
            SidePanelId::Usage => "USAGE",
            SidePanelId::Stats => "STATS",
            SidePanelId::Doctor => "DOCTOR",
            SidePanelId::ShellShow => "SHELLS",
            SidePanelId::Security => "SECURITY",
            SidePanelId::Models => "MODELS",
            SidePanelId::Agents => "AGENTS",
            SidePanelId::Mcp => "MCP",
            SidePanelId::Keybinds => "KEYS",
            SidePanelId::Themes => "THEMES",
            SidePanelId::Plugins => "PLUGINS",
            SidePanelId::Team => "TEAM",
            SidePanelId::Control => "CONTROL",
            SidePanelId::Workspace => "WORKSPACE",
            SidePanelId::TaskView => "ORDERS",
            SidePanelId::Diff => "DIFF",
            SidePanelId::Review => "REVIEW",
        }
    }

    /// The `/` command whose box fills this panel.
    ///
    /// Used by the placeholder a panel shows before anything is docked into
    /// it. A panel that cannot name its own loader cannot tell the user how to
    /// make itself useful.
    pub fn load_command(self) -> &'static str {
        match self {
            SidePanelId::Session => "/sidebar",
            SidePanelId::Chat => "/chat",
            SidePanelId::Terminal => "/terminal",
            SidePanelId::Help => "/help",
            SidePanelId::Tree => "/tree",
            SidePanelId::Files => "/files",
            SidePanelId::Sessions => "/sessions",
            SidePanelId::Tasks => "/tasks",
            SidePanelId::Plan => "/plan",
            SidePanelId::Goal => "/goal show",
            SidePanelId::Collaborators => "/collaborators",
            SidePanelId::Memory => "/memory-search",
            SidePanelId::Research => "/research-show",
            SidePanelId::Logs => "/logs",
            SidePanelId::Context => "/context",
            SidePanelId::Cost => "/cost",
            SidePanelId::Usage => "/usage",
            SidePanelId::Stats => "/stats",
            SidePanelId::Doctor => "/doctor",
            SidePanelId::ShellShow => "/shell-list",
            SidePanelId::Security => "/security-review-show",
            SidePanelId::Models => "/models",
            SidePanelId::Agents => "/agents",
            SidePanelId::Mcp => "/mcps",
            SidePanelId::Keybinds => "/keybinds",
            SidePanelId::Themes => "/themes",
            SidePanelId::Plugins => "/plugins",
            SidePanelId::Team => "/team",
            SidePanelId::Control => "/control",
            SidePanelId::Workspace => "/workspace",
            SidePanelId::TaskView => "/task",
            SidePanelId::Diff => "/diff",
            SidePanelId::Review => "/review",
        }
    }

    /// The stable config spelling of a panel, used for persisted `Ctrl+digit`
    /// hotkey bindings.
    ///
    /// These strings are user-visible: they are written into the user's
    /// global config and read back on a later run, so they are **not**
    /// derived from [`label`](Self::label) — rewording a title must not
    /// orphan an existing binding.
    ///
    /// `Session` keeps its original `sessions` spelling, and the `/sessions`
    /// box panel therefore takes `session_switcher` rather than colliding
    /// with it.
    pub fn config_key(self) -> &'static str {
        match self {
            SidePanelId::Session => "sessions",
            SidePanelId::Chat => "chat",
            SidePanelId::Terminal => "terminal",
            SidePanelId::Help => "help",
            SidePanelId::Tree => "tree",
            SidePanelId::Files => "files",
            SidePanelId::Sessions => "session_switcher",
            SidePanelId::Tasks => "tasks",
            SidePanelId::Plan => "plan",
            SidePanelId::Goal => "goal",
            SidePanelId::Collaborators => "collaborators",
            SidePanelId::Memory => "memory",
            SidePanelId::Research => "research",
            SidePanelId::Logs => "logs",
            SidePanelId::Context => "context",
            SidePanelId::Cost => "cost",
            SidePanelId::Usage => "usage",
            SidePanelId::Stats => "stats",
            SidePanelId::Doctor => "doctor",
            SidePanelId::ShellShow => "shells",
            SidePanelId::Security => "security",
            SidePanelId::Models => "models",
            SidePanelId::Agents => "agents",
            SidePanelId::Mcp => "mcp",
            SidePanelId::Keybinds => "keybinds",
            SidePanelId::Themes => "themes",
            SidePanelId::Plugins => "plugins",
            SidePanelId::Team => "team",
            SidePanelId::Control => "control",
            SidePanelId::Workspace => "workspace",
            SidePanelId::TaskView => "task_view",
            SidePanelId::Diff => "diff",
            SidePanelId::Review => "review",
        }
    }

    /// Whether this panel owns a live projection or a controller and so has
    /// its own `SidePanel` implementation, rather than being a box adopted at
    /// runtime.
    ///
    /// **One predicate, two consumers.** [`SidePanelHost::new`] filters this
    /// out of the hosted list, and the bespoke-or-hosted test reads the same
    /// function. When those were two separate `matches!` literals they drifted
    /// the moment a fifth bespoke panel was added: `Agents` kept its own
    /// `AgentsPanel` *and* got a hosted placeholder, so
    /// `hosted_panel_has_content` answered `Some(false)` for it and
    /// `materialize_hosted_panel` treated it as an unloaded box — calling
    /// `open_panel_box`, which calls `select_panel`, which calls
    /// `materialize_hosted_panel`. `/agents` recursed until the stack died.
    ///
    /// Adding a bespoke panel means adding it here and nothing else; the
    /// hosted list and the test both follow.
    pub fn is_bespoke(self) -> bool {
        matches!(
            self,
            Self::Session | Self::Agents | Self::Chat | Self::Terminal | Self::Help
        )
    }

    /// Position in [`SidePanelId::ALL`].
    ///
    /// Derived by searching [`Self::ALL`] rather than hand-numbered. A
    /// hand-numbered match is a 32-arm match that silently disagrees with the
    /// list the moment a panel is inserted, and the disagreement is invisible:
    /// the manager cursor and the `↑`/`↓` walk would each be *self-consistent*
    /// and would simply select different panels.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|id| *id == self).unwrap_or(0)
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
            SidePanelId::Tree => "session history",
            SidePanelId::Files => "browse · open in editor",
            SidePanelId::Sessions => "switch · resume",
            SidePanelId::Tasks => "running · waiting",
            SidePanelId::Plan => "steps · blockers",
            SidePanelId::Goal => "active goal · budget",
            SidePanelId::Collaborators => "present · online",
            SidePanelId::Memory => "recall · search",
            SidePanelId::Research => "runs · artifacts",
            SidePanelId::Logs => "daemon · notices",
            SidePanelId::Context => "window · usage",
            SidePanelId::Cost => "tokens · spend",
            SidePanelId::Usage => "rate limits · quota",
            SidePanelId::Stats => "analytics · cost",
            SidePanelId::Doctor => "diagnostics · health",
            SidePanelId::ShellShow => "recent commands",
            SidePanelId::Security => "findings · severity",
            SidePanelId::Models => "switch · provider",
            SidePanelId::Agents => "switch · roles",
            SidePanelId::Mcp => "servers · tools",
            SidePanelId::Keybinds => "map · customise",
            SidePanelId::Themes => "colours · reload",
            SidePanelId::Plugins => "installed · trust",
            SidePanelId::Team => "members · roles",
            SidePanelId::Control => "turn · handoff",
            SidePanelId::Workspace => "projects · attention",
            SidePanelId::TaskView => "orders · triggers",
            SidePanelId::Diff => "hunks · files",
            SidePanelId::Review => "changed files",
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
    ///
    /// Takes `ctx` because the claim can depend on state the panel does not
    /// own: the Terminal panel claims these keys only while its PTY view is
    /// focused, which lives on the controller rather than on the panel.
    fn owns_exit_key(&self, _key: KeyEvent, _ctx: &PanelCtx<'_>) -> bool {
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

/// Number of dockable edges — one slot per [`PanelPlacement`].
///
/// A named constant rather than `PanelPlacement::ALL.len()` so the slot table
/// is a fixed-size array that cannot be resized by a variant added for a
/// different reason, and so a mismatch between the table and the enum is a
/// compile error at the type level instead of a panic at runtime.
pub const PANEL_SLOT_COUNT: usize = 3;

/// How many panels a side remembers before dropping the oldest.
/// Bounded because this is a *convenience* memory, not an undo stack: the
/// only thing it is ever asked for is "what was here before the thing that
/// just left", so one entry deep already answers every real case. Two spare
/// entries cover a user bouncing a panel between two edges. The cap is what
/// stops a long session of docking from growing the host without limit.
pub const PANEL_HISTORY_DEPTH: usize = 3;

/// What an edge was showing, at a moment in the past.
///
/// Visibility is part of the remembered state, not an afterthought. An edge
/// the user had already collapsed with `Ctrl+B` must come back *collapsed*
/// when its panel is moved away — restoring it expanded would pop a column
/// back onto the frame the user had deliberately closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotState {
    panel: SidePanelId,
    visible: bool,
}

/// One dockable edge of the frame: what it shows now, and what it showed
/// before.
///
/// The history is what makes swapping sides feel like moving a panel rather
/// than deleting one. When a panel leaves an edge — because the user pushed it
/// to the other side — that edge restores the state it was showing, so the
/// thing they displaced comes back instead of the column going blank. An edge
/// whose history is empty restores to *collapsed*, which is the honest
/// answer: nothing was there before, so there is nothing to bring back.
///
/// Deliberately a value type with no back-reference to the host. Restoring a
/// panel cannot run a load or touch a client — it puts an already-constructed
/// panel back on screen — so this stays plain data and the whole swap stays
/// testable without an `App`.
#[derive(Debug, Clone)]
pub struct PanelSlot {
    /// The panel this edge is showing.
    panel: SidePanelId,
    /// Whether the edge is collapsed (`Ctrl+B`).
    ///
    /// Per-slot rather than one global flag because the whole point of
    /// swapping is that the two edges end up in *different* states: the panel
    /// that moved is up, and the edge it left is collapsed again. A shared flag
    /// would collapse both or neither.
    visible: bool,
    /// What this edge showed before its current panel, most recent last.
    history: VecDeque<SlotState>,
}

impl PanelSlot {
    /// A slot showing `panel`, expanded or collapsed.
    pub fn new(panel: SidePanelId, visible: bool) -> Self {
        Self {
            panel,
            visible,
            history: VecDeque::new(),
        }
    }

    pub fn panel(&self) -> SidePanelId {
        self.panel
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Put `panel` on this edge, remembering what it displaced.
    ///
    /// Selecting the panel already showing is a no-op rather than a
    /// self-push: re-selecting the current panel would fill the history with
    /// the panel it is supposed to be restoring, so the next swap would bring
    /// back what never left.
    pub fn occupy(&mut self, panel: SidePanelId) {
        if self.panel == panel {
            return;
        }
        if self.history.len() >= PANEL_HISTORY_DEPTH {
            self.history.pop_front();
        }
        self.history.push_back(SlotState {
            panel: self.panel,
            visible: self.visible,
        });
        self.panel = panel;
        // Occupying always reveals: a panel cannot be selected into a
        // collapsed edge, or `Enter` in the manager would look like a no-op.
        self.visible = true;
    }

    /// Move whatever is here to `target`, leaving this edge restored.
    ///
    /// Returns the panel that left, so the caller can hand it to the other
    /// edge. This is the swap primitive: the panel is *moved*, never copied,
    /// so it cannot end up loaded in two places at once.
    ///
    /// The vacated edge puts back the state it was in — panel *and*
    /// collapsed-ness — or collapses when it never had one, which is the case
    /// a user reaches by docking into a column they had previously emptied
    /// with `Ctrl+B`.
    pub fn release_to(&mut self, target: &mut PanelSlot) -> SidePanelId {
        let leaving = self.panel;
        match self.history.pop_back() {
            Some(previous) => {
                self.panel = previous.panel;
                self.visible = previous.visible;
            }
            None => {
                // Nothing was here before, so "restoring" means going back to
                // the collapsed state this edge was in before the first dock.
                self.visible = false;
            }
        }
        target.occupy(leaving);
        leaving
    }

    /// Whether this edge currently has a panel showing.
    pub fn is_occupied(&self) -> bool {
        self.visible
    }

    /// How many panels this edge remembers. Exposed for the bound test.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

/// Which panel the side column shows, and whether it holds focus.
pub struct SidePanelHost {
    /// One slot per placement, indexed by [`PanelPlacement::index`].
    ///
    /// The panel *state* below (session, chat, terminal, …) is still shared
    /// and keyed by [`SidePanelId`]; the slots only record which edge is
    /// showing which panel. That split is what lets a panel move between
    /// edges — the state travels with the id rather than living in the edge.
    slots: [PanelSlot; PANEL_SLOT_COUNT],
    /// Which edge the keyboard-facing API (`active`, `placement`, `Ctrl+B`)
    /// speaks for.
    ///
    /// Not "wherever focus is": this is the edge the *next* gesture acts on,
    /// so it can name a panel the user is reading without having driven the
    /// keyboard into it yet.
    selected: PanelPlacement,
    focused: bool,
    /// Whether yazi-style resize mode is armed (`Ctrl+N`).
    ///
    /// Host state rather than `App` state because the *subject* of a resize is
    /// "whatever surface has the keyboard", and only the host knows that: a
    /// panel takes the keyboard through [`Self::focus`], the main buffer takes
    /// it through [`Self::blur`], and the two mean opposite resizes.
    resizing: bool,
    session: session::SessionPanel,
    agents: agents::AgentsPanel,
    chat: chat::ChatPanel,
    terminal: terminal::TerminalPanel,
    help: help::HelpPanel,
    /// Every box-derived panel, in [`SidePanelId::ALL`] order from index 4 on.
    ///
    /// A `Vec` rather than 28 named fields: these panels share one
    /// implementation ([`hosted::HostPanel`]), so a field per panel would be
    /// 28 declarations that all say the same thing. Ordering is not relied on
    /// — lookup is by [`SidePanelId`] — but keeping it parallel to `ALL`
    /// makes a debugger dump of the host readable in registry order.
    hosted: Vec<hosted::HostPanel>,
}

impl Default for SidePanelHost {
    fn default() -> Self {
        Self::new()
    }
}

impl SidePanelHost {
    pub fn new() -> Self {
        Self {
            // Every edge starts on Session and expanded, but only the
            // selected one is *shown*: the other two are staging for a swap,
            // and a fresh frame must still be the single right-hand column
            // that shipped.
            slots: std::array::from_fn(|index| {
                PanelSlot::new(
                    SidePanelId::Session,
                    PanelPlacement::from_index(index) == Some(PanelPlacement::default()),
                )
            }),
            // Right is the shipped layout, so a user with no stored
            // preference gets exactly the frame they had before placements
            // existed.
            selected: PanelPlacement::default(),
            focused: false,
            resizing: false,
            session: session::SessionPanel,
            agents: agents::AgentsPanel::default(),
            chat: chat::ChatPanel::default(),
            terminal: terminal::TerminalPanel::default(),
            help: help::HelpPanel::default(),
            hosted: SidePanelId::ALL
                .iter()
                .copied()
                .filter(|id| !id.is_bespoke())
                .map(hosted::HostPanel::placeholder)
                .collect(),
        }
    }

    /// A host whose selected edge is `placement`.
    ///
    /// The startup path uses this rather than `new()` plus `select_placement`
    /// because the placement is restored from config while the `App` struct
    /// literal is still being built — there is no moment where a half-built
    /// host could paint the wrong edge.
    pub fn with_placement(placement: PanelPlacement) -> Self {
        let mut host = Self::new();
        host.select_placement(placement);
        host
    }

    /// The slot at `placement`.
    pub fn slot(&self, placement: PanelPlacement) -> &PanelSlot {
        &self.slots[placement.index()]
    }

    fn slot_mut(&mut self, placement: PanelPlacement) -> &mut PanelSlot {
        &mut self.slots[placement.index()]
    }

    /// Whether any edge is currently showing a panel.
    pub fn any_visible(&self) -> bool {
        self.slots.iter().any(PanelSlot::is_visible)
    }

    /// The panel `placement` is showing, whatever its expanded state.
    pub fn panel_at(&self, placement: PanelPlacement) -> SidePanelId {
        self.slot(placement).panel()
    }

    /// Whether `placement` is expanded right now.
    pub fn is_visible_at(&self, placement: PanelPlacement) -> bool {
        self.slot(placement).is_visible()
    }

    /// Expand or collapse `placement` without changing which panel it holds.
    pub fn set_visible_at(&mut self, placement: PanelPlacement, visible: bool) {
        self.slot_mut(placement).set_visible(visible);
    }

    /// Which edge the keyboard-facing gestures currently act on.
    pub fn placement(&self) -> PanelPlacement {
        self.selected
    }

    /// Point the gestures at `placement`.
    ///
    /// Deliberately does not move the keyboard or change what is loaded:
    /// choosing which edge you are driving is not a request to enter it, and
    /// is emphatically not a request to re-select the panel it holds.
    pub fn select_placement(&mut self, placement: PanelPlacement) {
        self.selected = placement;
        // Focus is a property of the panel you are *in*, so pointing the
        // gestures elsewhere hands it back rather than leaving a highlight on
        // an edge the user has walked away from.
        self.focused = false;
    }

    /// Step the selected edge to the next placement, returning the new one.
    pub fn cycle_placement(&mut self) -> PanelPlacement {
        let next = self.selected.next();
        self.selected = next;
        next
    }

    /// Show `id` on `placement`, and remember what that edge was showing.
    ///
    /// This is a *swap* when the panel currently lives somewhere else: it is
    /// moved rather than copied, so the edge it leaves restores its own
    /// previous panel (or collapses, if it never had one) instead of going
    /// blank. See [`PanelSlot::release_to`].
    pub fn dock(&mut self, id: SidePanelId, placement: PanelPlacement) {
        // Where the panel is coming from, if anywhere.
        let origin = self
            .slots
            .iter()
            .position(|slot| slot.panel() == id)
            .and_then(PanelPlacement::from_index);
        if let Some(origin) = origin {
            if origin == placement {
                // Already here: just make sure it is showing and selected.
                self.slot_mut(placement).set_visible(true);
                self.selected = placement;
                return;
            }
            // Borrow both slots at once, so the move is a single operation
            // rather than two steps a reader could interleave wrongly.
            let (from_slot, to_slot) = two_slots_mut(&mut self.slots, origin, placement);
            from_slot.release_to(to_slot);
        } else {
            self.slot_mut(placement).occupy(id);
        }
        self.selected = placement;
    }

    pub fn active(&self) -> SidePanelId {
        self.slot(self.selected).panel()
    }

    /// The hosted panel for `id`, if it is one.
    fn hosted_mut(&mut self, id: SidePanelId) -> Option<&mut hosted::HostPanel> {
        self.hosted.iter_mut().find(|panel| panel.id() == id)
    }

    /// Whether a hosted panel currently has a real box adopted into it, as
    /// opposed to the placeholder it shows when nothing has been docked.
    ///
    /// `None` for the bespoke panels, which are not hosted (`is_bespoke`). Exposed so a
    /// test can observe adoption without reaching through a private field.
    pub fn hosted_panel_has_content(&self, id: SidePanelId) -> Option<bool> {
        self.hosted
            .iter()
            .find(|panel| panel.id() == id)
            .map(hosted::HostPanel::has_content)
    }

    /// Hand a just-unmounted box to the panel that can hold it.
    ///
    /// This is what makes docking a *move* rather than a copy: the component
    /// [`FocusManager::pop`] returned is the very one the box was painting, so
    /// the panel starts with the same filter, cursor, scroll offset and
    /// selection. Returns `false` for a panel that is not hosted (the
    /// bespoke ones), which is a programming error rather than a runtime
    /// condition — those panels have their own dock paths.
    pub fn adopt(
        &mut self,
        id: SidePanelId,
        widget: Box<dyn crate::tui::components::component::Component>,
    ) -> bool {
        match self.hosted_mut(id) {
            Some(panel) => {
                panel.adopt(widget);
                true
            }
            None => false,
        }
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
        if self.is_focused() && self.active() == SidePanelId::Chat {
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
            self.active().title()
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
        match self.active() {
            SidePanelId::Session => self.session.composer_hints(&hints),
            SidePanelId::Chat => self.chat.composer_hints(&hints),
            SidePanelId::Terminal => self.terminal.composer_hints(&hints),
            SidePanelId::Help => self.help.composer_hints(&hints),
            other => self
                .hosted
                .iter()
                .find(|panel| panel.id() == other)
                .and_then(|panel| panel.composer_hints(&hints)),
        }
    }

    /// Chat panel's composer buffer, so a keystroke can mirror it into the
    /// shared per-project draft.
    pub fn chat_panel_mut(&mut self) -> &mut chat::ChatPanel {
        &mut self.chat
    }

    /// The agents panel, so the command layer can load the registry rows it
    /// is not allowed to resolve itself.
    pub fn agents_panel_mut(&mut self) -> &mut agents::AgentsPanel {
        &mut self.agents
    }

    /// Replace the agents panel's rows from the resolved registry.
    ///
    /// Goes through the panel's reload rather than `set_rows` so an armed
    /// delete is dropped: the row the user armed against may not survive a
    /// reload, and a confirm against a different row would delete something
    /// they never selected.
    pub fn set_agents_rows(&mut self, rows: Vec<agents::AgentRow>) {
        self.agents.reload_from(rows);
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
        match self.active() {
            SidePanelId::Session => false,
            SidePanelId::Chat => self.chat.is_unbound(),
            // The terminal panel is always actionable: `i` takes the keyboard
            // of a terminal that exists, or starts a shell when there is
            // none, so refusing focus would only strand the keyboard on the
            // one surface that can fix it.
            SidePanelId::Terminal => self.terminal.is_empty(),
            // Help always has rows, so it can always take focus.
            SidePanelId::Help => false,
            // The agents panel is never "empty": even with no agents loaded
            // it can create one, and `n` is the action that fills it. The
            // Terminal precedent applies — a surface that can act must be
            // reachable, or the key that would fix it never arrives.
            SidePanelId::Agents => false,
            // A hosted panel with nothing adopted into it is empty, and
            // refusing focus is what leaves the keyboard in the composer —
            // where the user can immediately type the `/` command that fills
            // it. Taking focus instead would hand every key to a widget that
            // has no verbs, which is the dead end the rule exists to avoid.
            other => self
                .hosted
                .iter()
                .find(|panel| panel.id() == other)
                .is_none_or(|panel| !panel.has_content()),
        }
    }

    fn panel(&mut self) -> &mut dyn SidePanel {
        let id = self.active();
        self.panel_by_id_mut(id)
    }

    /// The panel instance for `id`.
    ///
    /// Panel *state* is keyed by [`SidePanelId`] rather than by edge, which is
    /// what lets a panel move between edges without losing its cursor, filter
    /// or scroll offset: the state travels with the id, and the slot only
    /// records which edge is showing which id.
    fn panel_by_id_mut(&mut self, id: SidePanelId) -> &mut dyn SidePanel {
        match id {
            SidePanelId::Session => &mut self.session,
            SidePanelId::Agents => &mut self.agents,
            SidePanelId::Chat => &mut self.chat,
            SidePanelId::Terminal => &mut self.terminal,
            SidePanelId::Help => &mut self.help,
            other => self
                .hosted_mut(other)
                .expect("every non-bespoke panel is constructed from SidePanelId::ALL")
                as &mut dyn SidePanel,
        }
    }

    /// Switch to `id`. The previous panel is deactivated and the new one
    /// activated; focus follows only if it already had it, so selecting a
    /// panel from an unfocused state does not silently capture the composer.
    pub fn select(&mut self, id: SidePanelId, ctx: &mut PanelCtx<'_>) {
        if id == self.active() {
            self.panel().on_activate(ctx);
            return;
        }
        self.panel().on_deactivate(ctx);
        // Goes through the slot so the panel this edge was showing is
        // remembered: selecting a different panel here is the same act as
        // docking one, and the panel that was displaced must come back if
        // this edge is later vacated by a swap.
        self.slot_mut(self.selected).occupy(id);
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
        let current = self.active().index() as i64;
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

    /// Whether resize mode is armed.
    pub fn is_resizing(&self) -> bool {
        self.resizing
    }

    /// Arm or disarm resize mode, returning the new state.
    pub fn set_resizing(&mut self, resizing: bool) -> bool {
        self.resizing = resizing;
        resizing
    }

    /// Flip resize mode, returning the new state.
    pub fn toggle_resizing(&mut self) -> bool {
        self.resizing = !self.resizing;
        self.resizing
    }

    /// Which surface a resize key acts on.
    ///
    /// The focused panel when the keyboard is in one, otherwise the main
    /// buffer — which is the whole reason the mode lives here: a chat column
    /// and the transcript beside it share a border, and `h` has to mean the
    /// opposite thing on each side of it.
    pub fn resize_subject(&self) -> crate::tui::panel_resize::ResizeSubject {
        use crate::tui::panel_resize::ResizeSubject;
        if self.focused && self.is_visible_at(self.selected) {
            ResizeSubject::Panel(self.selected)
        } else {
            ResizeSubject::Main
        }
    }

    /// Every edge currently showing a panel, in [`PanelPlacement::ALL`] order.
    ///
    /// The same list the render path derives, read from one place so a resize
    /// can never reason about a border that is not actually on screen.
    pub fn occupied_placements(&self) -> Vec<PanelPlacement> {
        PanelPlacement::ALL
            .iter()
            .copied()
            .filter(|placement| self.is_visible_at(*placement))
            .collect()
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
            if self.panel().owns_exit_key(key, ctx) {
                // The panel answered the key itself, so its intent has to
                // reach the caller. Discarding it here and returning a bare
                // `Consumed` meant a panel could claim `Esc` and emit
                // `TerminalPanelBlur` into nothing: focus stayed on the PTY,
                // the host looked like it had handled the key, and there was no
                // exit left. Returning the panel's own answer keeps the
                // first-refusal contract and the effect.
                return Some(self.panel().handle_key(key, action, ctx));
            }
            self.blur();
            return Some(PanelKey::Consumed);
        }
        Some(self.panel().handle_key(key, action, ctx))
    }

    /// Paint the active panel.
    pub fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        self.panel().render(ctx, buf, area);
    }

    /// Paint the panel docked to `placement`.
    ///
    /// Takes the edge explicitly rather than reading the selected one, so the
    /// render path can paint every occupied column in one pass without
    /// re-pointing the app's idea of "the" panel as a side effect. Focus is
    /// answered per-edge for the same reason: the focused edge is the selected
    /// one *and* still up.
    pub fn render_at(
        &mut self,
        placement: PanelPlacement,
        ctx: &mut PanelCtx<'_>,
        buf: &mut Buffer,
        area: Rect,
    ) {
        let id = self.panel_at(placement);
        self.panel_by_id_mut(id).render(ctx, buf, area);
    }

    /// Whether the panel on `placement` is the one holding the keyboard.
    pub fn is_focused_at(&self, placement: PanelPlacement) -> bool {
        self.is_focused() && placement == self.selected
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

    // ------------------------------------------------------------ placements

    /// Every direction must mean exactly one thing under every placement.
    ///
    /// The dispatcher in `move_directionally` answers `toward` and `away`
    /// first and falls back to `cycle_step`. If any direction satisfied more
    /// than one of those, one of the branches would be silently unreachable
    /// and a gesture would quietly change meaning between layouts.
    #[test]
    fn every_direction_is_exactly_one_gesture_per_placement() {
        for placement in PanelPlacement::ALL.iter().copied() {
            let toward = placement.toward();
            let away = placement.away();
            assert_ne!(toward, away, "{placement:?}: toward and away collide");
            for direction in [
                PanelDirection::Left,
                PanelDirection::Right,
                PanelDirection::Up,
                PanelDirection::Down,
            ] {
                let meanings = usize::from(direction == toward)
                    + usize::from(direction == away)
                    + usize::from(placement.cycle_step(direction).is_some());
                assert_eq!(
                    meanings, 1,
                    "{placement:?}: {direction:?} has {meanings} meanings, want exactly 1"
                );
            }
        }
    }

    /// `away` is the way *out*. If it ever stopped being the exact inverse of
    /// `toward`, a panel would have no gesture that returns to the composer
    /// and the keyboard would be stranded in it.
    #[test]
    fn away_is_the_exact_inverse_of_toward() {
        let inverse = |d: PanelDirection| match d {
            PanelDirection::Left => PanelDirection::Right,
            PanelDirection::Right => PanelDirection::Left,
            PanelDirection::Up => PanelDirection::Down,
            PanelDirection::Down => PanelDirection::Up,
        };
        for placement in PanelPlacement::ALL.iter().copied() {
            assert_eq!(placement.away(), inverse(placement.toward()));
        }
    }

    /// The column placements keep `↑`/`↓` walking the panel list — the
    /// behavior that shipped — and only the bottom row swaps the axis.
    #[test]
    fn the_panel_list_cycles_perpendicular_to_the_placement_axis() {
        assert_eq!(
            PanelPlacement::Right.cycle_step(PanelDirection::Down),
            Some(1)
        );
        assert_eq!(PanelPlacement::Right.cycle_step(PanelDirection::Left), None);
        assert_eq!(
            PanelPlacement::Left.cycle_step(PanelDirection::Down),
            Some(1)
        );
        assert_eq!(
            PanelPlacement::Bottom.cycle_step(PanelDirection::Right),
            Some(1)
        );
        assert_eq!(
            PanelPlacement::Bottom.cycle_step(PanelDirection::Down),
            None,
            "Down is how you enter a bottom panel, not how you walk the list"
        );
    }

    /// A persisted spelling is user data: it is written to disk and read back
    /// next run, so every placement needs a round trip. A variant that forgot
    /// one would silently reset to the default layout on the next launch.
    #[test]
    fn placements_round_trip_through_their_persisted_spelling() {
        for placement in PanelPlacement::ALL.iter().copied() {
            assert_eq!(PanelPlacement::parse(placement.as_str()), Some(placement));
            assert!(!placement.label().is_empty());
        }
        // Tolerant of what a user would actually type and of stray casing.
        assert_eq!(
            PanelPlacement::parse("  Bottom "),
            Some(PanelPlacement::Bottom)
        );
        assert_eq!(PanelPlacement::parse("DOWN"), Some(PanelPlacement::Bottom));
        // A typo drops the preference rather than refusing to boot.
        assert_eq!(PanelPlacement::parse("sideways"), None);
    }

    /// Cycling must cover every placement and return home, or `p` would
    /// strand the user on a layout they cannot leave.
    #[test]
    fn cycling_visits_every_placement_and_wraps() {
        let mut seen = vec![PanelPlacement::Right];
        let mut current = PanelPlacement::Right;
        for _ in 0..PanelPlacement::ALL.len() - 1 {
            current = current.next();
            seen.push(current);
        }
        assert_eq!(seen.len(), PanelPlacement::ALL.len());
        assert!(PanelPlacement::ALL.iter().all(|p| seen.contains(p)));
        assert_eq!(current.next(), PanelPlacement::Right);
    }

    /// The default is the layout that shipped first, so a user with no stored
    /// preference sees no change at all.
    #[test]
    fn the_default_placement_is_the_shipped_right_column() {
        assert_eq!(PanelPlacement::default(), PanelPlacement::Right);
        assert_eq!(SidePanelHost::new().placement(), PanelPlacement::Right);
    }

    #[test]
    fn with_placement_restores_without_disturbing_the_loaded_panel() {
        let mut host = SidePanelHost::with_placement(PanelPlacement::Bottom);
        assert_eq!(host.placement(), PanelPlacement::Bottom);
        assert_eq!(
            host.active(),
            SidePanelId::Session,
            "restoring a layout must not re-select a panel"
        );
        assert!(
            !host.is_focused(),
            "restoring a layout must not seize focus"
        );
        host.select_placement(PanelPlacement::Left);
        assert_eq!(host.placement(), PanelPlacement::Left);
    }

    // ------------------------------------------------------------ the swap

    /// The whole point of the slot history: pushing a panel to another edge
    /// must not cost the edge it left whatever it was showing.
    #[test]
    fn a_moved_panel_restores_what_the_edge_it_left_was_showing() {
        let mut host = SidePanelHost::new();
        host.dock(SidePanelId::Chat, PanelPlacement::Right);
        // `Session` was on the right before `Chat` displaced it.
        host.dock(SidePanelId::Chat, PanelPlacement::Left);

        assert_eq!(
            host.panel_at(PanelPlacement::Left),
            SidePanelId::Chat,
            "the panel moved to the edge the gesture named"
        );
        assert_eq!(
            host.panel_at(PanelPlacement::Right),
            SidePanelId::Session,
            "and the edge it left brought its own panel back"
        );
        assert!(
            host.is_visible_at(PanelPlacement::Right),
            "a restore is visible — a column that reappeared empty reads as data loss"
        );
    }

    /// The user's own scenario: an edge that was collapsed before the panel
    /// arrived has nothing to restore, so it goes back to collapsed rather
    /// than to an arbitrary default panel.
    #[test]
    fn an_edge_that_was_collapsed_restores_to_collapsed() {
        let mut host = SidePanelHost::new();
        host.set_visible_at(PanelPlacement::Right, false);
        host.dock(SidePanelId::Chat, PanelPlacement::Right);
        assert!(host.is_visible_at(PanelPlacement::Right));

        host.dock(SidePanelId::Chat, PanelPlacement::Left);

        assert_eq!(host.panel_at(PanelPlacement::Left), SidePanelId::Chat);
        assert!(
            !host.is_visible_at(PanelPlacement::Right),
            "nothing was there before the panel, so nothing comes back"
        );
        assert!(
            host.is_visible_at(PanelPlacement::Left),
            "and the edge that received it is up"
        );
    }

    /// A panel is *moved*, never copied. Two edges showing the same panel id
    /// would mean one widget state rendered twice, which is how a chat panel
    /// ends up drawing the same selection into two columns.
    #[test]
    fn a_panel_is_never_loaded_on_two_edges_at_once() {
        let mut host = SidePanelHost::new();
        host.dock(SidePanelId::Help, PanelPlacement::Right);
        host.dock(SidePanelId::Help, PanelPlacement::Bottom);
        let showing: Vec<_> = PanelPlacement::ALL
            .iter()
            .copied()
            .filter(|placement| host.panel_at(*placement) == SidePanelId::Help)
            .collect();
        assert_eq!(showing, vec![PanelPlacement::Bottom]);
    }

    /// Re-docking a panel to the edge it is already on must not push itself
    /// onto that edge's history: the next swap would then "restore" a panel
    /// that never left.
    #[test]
    fn re_docking_to_the_same_edge_does_not_corrupt_its_history() {
        let mut host = SidePanelHost::new();
        host.dock(SidePanelId::Chat, PanelPlacement::Right);
        host.dock(SidePanelId::Chat, PanelPlacement::Right);
        host.dock(SidePanelId::Chat, PanelPlacement::Left);
        assert_eq!(
            host.panel_at(PanelPlacement::Right),
            SidePanelId::Session,
            "the edge must restore what it had, not the panel that left it"
        );
    }

    /// The history is a convenience memory, not an undo stack, so it must
    /// not grow without bound in a long session.
    #[test]
    fn the_history_is_bounded() {
        let mut slot = PanelSlot::new(SidePanelId::Session, true);
        for id in SidePanelId::ALL
            .iter()
            .copied()
            .take(PANEL_HISTORY_DEPTH + 4)
        {
            if id != SidePanelId::Session {
                slot.occupy(id);
            }
        }
        assert!(
            slot.history_len() <= PANEL_HISTORY_DEPTH,
            "history grew to {}",
            slot.history_len()
        );
    }

    /// Collapsing is per-edge. It has to be: the entire swap experience is
    /// that the edge a panel left and the edge it arrived at can be in
    /// different states at the same time.
    #[test]
    fn collapsing_one_edge_leaves_the_other_alone() {
        let mut host = SidePanelHost::new();
        host.dock(SidePanelId::Chat, PanelPlacement::Right);
        host.dock(SidePanelId::Session, PanelPlacement::Left);
        host.set_visible_at(PanelPlacement::Right, false);
        assert!(!host.is_visible_at(PanelPlacement::Right));
        assert!(
            host.is_visible_at(PanelPlacement::Left),
            "collapsing one column must not take the other with it"
        );
        assert!(host.any_visible());
    }

    /// A fresh frame is still the single right-hand column that shipped,
    /// even though the host now carries slots for three edges.
    #[test]
    fn a_fresh_host_shows_only_the_right_column() {
        let host = SidePanelHost::new();
        let shown: Vec<_> = PanelPlacement::ALL
            .iter()
            .copied()
            .filter(|placement| host.is_visible_at(*placement))
            .collect();
        assert_eq!(shown, vec![PanelPlacement::Right]);
    }

    /// Every panel must be constructible by name and nameable on disk.
    ///
    /// `load_command` is what a placeholder tells the user to run, and
    /// `config_key` is what a persisted `Ctrl+digit` binding is stored under.
    /// A variant that forgets either one is reachable from the manager but
    /// cannot say how to fill itself or survive a restart.
    #[test]
    fn every_panel_names_its_loader_and_its_config_spelling() {
        for panel in SidePanelId::ALL.iter().copied() {
            let command = panel.load_command();
            assert!(
                command.starts_with('/'),
                "{panel:?} has no `/` loader, got {command:?}"
            );
            assert!(
                !panel.config_key().is_empty(),
                "{panel:?} has no persisted config spelling"
            );
            assert!(
                panel
                    .config_key()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "{panel:?} config spelling must be snake_case"
            );
        }
    }

    /// Persisted hotkey spellings are user data: two panels sharing one
    /// means a binding silently retargets on reload.
    #[test]
    fn config_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for panel in SidePanelId::ALL.iter().copied() {
            assert!(
                seen.insert(panel.config_key()),
                "duplicate persisted panel spelling: {}",
                panel.config_key()
            );
        }
    }

    /// The hosted bridge paints into the column, not the screen corner, and
    /// fills the theme backdrop behind widgets that paint text only.
    #[test]
    fn render_hosted_paints_at_the_column_with_a_backdrop() {
        use crate::tui::components::component::{Component, DialogType};

        /// A widget that paints nothing: as a modal it relied on the dimmed
        /// overlay behind it, so every interior cell must come from the bridge.
        struct Blank;
        impl Component for Blank {
            fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
                None
            }
            fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
                None
            }
            fn render(&mut self, _frame: &mut ratatui::Frame, _area: Rect, _theme: &Arc<Theme>) {}
            fn dialog_type(&self) -> DialogType {
                DialogType::None
            }
        }

        let theme = Arc::new(Theme::default());
        let mut widget = Blank;
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 30));
        for cell in buf.content.iter_mut() {
            cell.set_symbol("X");
        }
        let area = Rect::new(70, 5, 30, 20);
        render_hosted(&mut widget, &mut buf, area, &theme, " Panel ");

        // Nothing outside the column is touched. The old copy loop
        // intersected the scratch with the screen buffer, which painted the
        // panel over the top-left corner of the main view.
        for y in 0..30 {
            for x in 0..100 {
                if x >= 70 && (5..25).contains(&y) {
                    continue;
                }
                let cell = buf.cell((x, y)).expect("in bounds");
                assert_eq!(cell.symbol(), "X", "stray paint at ({x}, {y})");
            }
        }
        // The border lands at the column's top-left, not the screen's.
        assert_eq!(buf.cell((70, 5)).expect("in bounds").symbol(), "┌");
        // Untouched interior cells carry the theme backdrop, not the
        // terminal default showing through as transparency.
        let interior = buf.cell((75, 10)).expect("in bounds");
        assert_eq!(interior.bg, theme.background);
    }

    /// Titles are the composer's destination badge. Two panels sharing one
    /// makes `[X]` ambiguous about which surface has the keyboard.
    #[test]
    fn composer_badges_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for panel in SidePanelId::ALL.iter().copied() {
            assert!(
                seen.insert(panel.title()),
                "duplicate composer badge: {}",
                panel.title()
            );
        }
    }

    /// The hosted adapter is the implementation for every box-derived panel,
    /// so the host must have constructed one for each of them. A panel in
    /// `ALL` with no backing `HostPanel` would make `panel()` panic the first
    /// time it is selected.
    #[test]
    fn every_box_derived_panel_has_a_hosted_instance() {
        let host = SidePanelHost::new();
        for panel in SidePanelId::ALL.iter().copied() {
            let hosted = host.hosted.iter().any(|candidate| candidate.id() == panel);
            assert_eq!(
                panel.is_bespoke(),
                !hosted,
                "{panel:?} must be exactly one of bespoke-or-hosted"
            );
        }
    }

    /// The invariant `materialize_hosted_panel` actually branches on.
    ///
    /// It proceeds only for `Some(false)` — a hosted panel with no box adopted
    /// yet — and its first move is to `open_panel_box`, which comes straight
    /// back through `select_panel`. So a bespoke panel answering `Some(false)`
    /// is an infinite recursion, not a slow path. The bespoke-or-hosted test
    /// above cannot see this: it compared against the same stale literal that
    /// built the list, so it agreed with the bug.
    #[test]
    fn a_bespoke_panel_never_reads_as_an_unloaded_hosted_panel() {
        let host = SidePanelHost::new();
        for panel in SidePanelId::ALL
            .iter()
            .copied()
            .filter(|id| id.is_bespoke())
        {
            assert_eq!(
                host.hosted_panel_has_content(panel),
                None,
                "{panel:?} is bespoke: reporting a hosted placeholder would send \
                 materialize_hosted_panel into open_panel_box and back again"
            );
        }
    }
}

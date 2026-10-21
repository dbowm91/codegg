//! Generic side panel for any box that can dock into the column.
//!
//! ## Why this exists
//!
//! The panel host originally shipped four panels: `Session` and `Chat` are
//! bespoke (they own live projections), while `Terminal` and `Help` host an
//! existing widget. Extending that to the rest of the dockable boxes one
//! bespoke `SidePanel` impl at a time would mean 28 copies of the same four
//! methods — and 28 chances to drift from the box they claim to represent.
//!
//! So this module adds one adapter instead. [`HostPanel`] holds a
//! `Box<dyn Component>` — the *same* component the modal stack mounted — and
//! paints it into the side column. Docking therefore does not "make a similar
//! panel", it **moves the box**: [`SidePanelHost::adopt`](super::SidePanelHost::adopt)
//! is handed the component that [`FocusManager::pop`] returned, so the panel
//! shows the identical filter, cursor, scroll offset and selection the box was
//! showing a frame earlier.
//!
//! ## The render bridge
//!
//! `SidePanel::render` receives a `&mut Buffer`; every `Component::render`
//! takes a `&mut Frame`, and ratatui's `Frame` has no public constructor. The
//! one widget that predates the host — `HelpDialog` — solves this with a
//! bespoke `render_into(&mut Buffer, ..)` added alongside `Component::render`.
//! Repeating that for 28 dialogs would be a large mechanical diff across
//! unrelated files, so the bridge here does it generically: render into an
//! in-memory backend the size of the column, then copy the result into the
//! host's buffer.
//!
//! This is a *bridge*, not the destination. It costs one small allocation per
//! panel per frame, and it is the only reason a dockable box can be adopted
//! without touching that box. A widget that wants to render into a buffer
//! directly should grow its own `render_into` — as `HelpDialog` did — and the
//! host should prefer it; see [`HostPanel::render`], which checks for exactly
//! that before falling back.
//!
//! ## Why the content is not fetched here
//!
//! A panel renders from already-resolved frontend state; it never opens a
//! client or resolves a project. That is why an adopted panel is empty until a
//! box is docked into it: the box was the thing that fetched the content. A
//! panel selected straight from the manager with nothing adopted renders an
//! explicit placeholder describing the panel's **state** — nothing loaded, a
//! load in flight, or the reason it cannot act — because an empty bordered box
//! is indistinguishable from a rendering bug, and a panel that told the user
//! to go run a `/` command elsewhere was describing a workflow this surface
//! does not have.

use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{buffer::Buffer, Frame};

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

use super::{PanelCtx, PanelHints, PanelKey, SidePanel, SidePanelId};

/// A side panel that hosts one existing modal component.
///
/// Constructed either by adopting the component a box was running
/// ([`adopt`](super::SidePanelHost::adopt)) — the docked case, which has live
/// content — or with a [`Placeholder`] (the manager case, which does not).
pub struct HostPanel {
    id: SidePanelId,
    widget: Box<dyn Component>,
}

impl HostPanel {
    /// Host `widget` as the panel for `id`.
    pub fn new(id: SidePanelId, widget: Box<dyn Component>) -> Self {
        Self { id, widget }
    }

    /// Host an explicit placeholder describing this panel's current state.
    pub fn placeholder(id: SidePanelId) -> Self {
        Self::new(id, Box::new(Placeholder::new(id)))
    }

    /// Record what happened when the manager last tried to fill this panel.
    ///
    /// A no-op once a real box has been adopted: a placeholder that is not on
    /// screen has no business remembering anything, and rewriting state here
    /// would let a stale reason outlive the condition that produced it.
    pub fn set_placeholder_state(&mut self, state: PlaceholderState) {
        if self.has_content() {
            return;
        }
        if let Some(placeholder) = self.widget.as_any_mut().downcast_mut::<Placeholder>() {
            placeholder.state = state;
        }
    }

    /// Whether a real box has been adopted into this panel yet.
    pub fn has_content(&self) -> bool {
        !self.widget.is_placeholder()
    }

    /// Hand the mounted component of an open box to this panel.
    pub fn adopt(&mut self, widget: Box<dyn Component>) {
        self.widget = widget;
    }

    fn title(&self, focused: bool) -> String {
        let dot = if focused { "●" } else { " " };
        format!(" {} {} ", self.id.label(), dot)
    }
}

/// The verbs a hosted panel falls back to describing when its widget has no
/// keymap of its own.
///
/// Still a host-side guess, so `i act` stays here — but only as the fallback.
/// Every widget whose verbs can be wrong should answer
/// [`Component::keymap_hint`] instead, because this string cannot know what
/// `handle_key` dispatches, and it had `i` wrong for every box it described.
const GENERIC_KEYS: &str = "j/k scroll · i act";

impl SidePanel for HostPanel {
    fn id(&self) -> SidePanelId {
        self.id
    }

    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        let title = self.title(ctx.focused);
        crate::tui::panels::render_hosted(&mut *self.widget, buf, area, &ctx.theme, &title);
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        _action: Option<crate::tui::input::InputAction>,
        _ctx: &mut PanelCtx<'_>,
    ) -> PanelKey {
        // The hosted widget owns its own keymap, and the host has already
        // answered the directional before a panel sees a key (see
        // `SidePanelHost::handle_key`). Everything that reaches here is the
        // widget's business, and it is consumed either way: a stray `j` must
        // never fall through to the composer and start an agent turn.
        match self.widget.handle_key(key) {
            Some(msg) => PanelKey::Intent(msg),
            None => PanelKey::Consumed,
        }
    }

    /// The verbs this panel actually dispatches, named once by the widget.
    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        // Nothing adopted, so there are no verbs to teach and no keys the user
        // could usefully press — and the panel cannot take the keyboard either
        // (`focus` refuses an empty panel, so `hints.focused` is never true
        // here), which is why this says nothing rather than naming the command
        // that would fill it. That line used to live in this branch and could
        // never render: advice about a surface you cannot enter is advice for
        // someone who is not there.
        if !self.has_content() {
            return None;
        }
        // The widget's own keymap, not a paraphrase of it. `/files` peeks
        // with `l`, expands with `enter` and pages with `^d`/`^u`, and none of
        // those are `i act`; a docked panel that advertised the generic line was
        // teaching the wrong keys for the one box whose keys cannot be guessed.
        let keys = self
            .widget
            .keymap_hint()
            .unwrap_or_else(|| GENERIC_KEYS.to_string());
        if hints.focused {
            Some(format!("{} · {keys} · Shift+← back", self.id.title()))
        } else {
            // The verbs are not live yet, so the line leads with the gesture
            // that makes them live. Placement-correct, because it is a different
            // key on every edge: `Shift+→` for the right column, `Shift+←` for
            // the left, `Shift+↓` for the bottom row.
            Some(format!(
                "{} · {} in · {keys}",
                self.id.title(),
                hints.enter_gesture()
            ))
        }
    }
}

/// What a hosted panel is showing while nothing has been adopted into it.
///
/// The manager sets this when it tries to fill a panel, so the column can say
/// what actually happened instead of asserting that nothing is loaded. It is
/// deliberately a **state**, never an instruction: the panel is already where
/// the user asked for it, so there is nothing to move into it, and telling them
/// to leave and run a `/` command described a workflow this surface does not
/// have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderState {
    /// Nothing has been committed to this panel yet, so there is nothing to
    /// report beyond the fact that it is empty.
    Idle,
    /// A load started; the box opens through its own completion path.
    Loading,
    /// The panel cannot act without something the manager cannot supply.
    ///
    /// The reason is a *why*, never a command to go and run — that instruction
    /// is the toast's job, which is transient and already gone by the time the
    /// user looks at the column again.
    Cannot(&'static str),
}

/// What is mounted when a panel has no adopted box.
///
/// Named rather than inferred: a panel that is empty because nothing has been
/// docked into it must say so, because the alternative — an empty bordered box
/// — is indistinguishable from a rendering bug.
pub struct Placeholder {
    id: SidePanelId,
    state: PlaceholderState,
}

impl Placeholder {
    fn new(id: SidePanelId) -> Self {
        Self {
            id,
            state: PlaceholderState::Idle,
        }
    }

    /// Headline plus explanation, in that order.
    ///
    /// Both lines describe the panel's state; neither one tells the user to go
    /// somewhere and come back.
    fn lines(&self) -> Vec<ratatui::text::Line<'static>> {
        let bold = Style::default().add_modifier(ratatui::style::Modifier::BOLD);
        match self.state {
            PlaceholderState::Idle => vec![
                ratatui::text::Line::from(""),
                ratatui::text::Line::from("Nothing loaded.").style(bold),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from("Open it from the panel manager (⇧S)."),
            ],
            PlaceholderState::Loading => vec![
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(format!("Loading {}…", self.id.label())).style(bold),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from("It opens here when it arrives."),
            ],
            PlaceholderState::Cannot(reason) => vec![
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(format!("{} can't open here.", self.id.label()))
                    .style(bold),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(reason),
            ],
        }
    }

    /// The body both render paths share. The difference is only who owns the
    /// border: the host owns it in a column, the widget owns it standalone.
    fn body(&self) -> Paragraph<'static> {
        Paragraph::new(self.lines())
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true })
    }
}

impl Component for Placeholder {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        // Nothing to act on. The host consumes the key either way.
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        // Standalone path only: a placeholder is never mounted as a modal
        // (`dialog_type` is `None`), so this exists to keep the component total
        // rather than to be a supported surface.
        let body = self.body().block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(body, area);
    }

    /// The host has already painted a border, a title, and the `●` focus
    /// marker into `area` by the time this runs, so painting another border
    /// over the same rect does not layer — it overwrites all three. That is
    /// why the host has its own seam for this and why the body is inset
    /// instead of framed.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        let inner = area.inner(ratatui::layout::Margin::new(1, 1));
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        frame.render_widget(self.body(), inner);
    }

    fn dialog_type(&self) -> DialogType {
        // A placeholder is never mounted as a modal, so this value is only
        // ever read by the host's render path. `None` keeps it out of the
        // modal slot semantics entirely.
        DialogType::None
    }

    /// A placeholder is not a viewer box: it has nothing to dock *into*, and
    /// `docks_as_viewer` is also the host's "this key is mine" signal.
    fn docks_as_viewer(&self) -> bool {
        false
    }
}

/// Marker so [`HostPanel::has_content`] can tell a real box from a
/// placeholder without matching on the concrete type.
trait PlaceholderMarker {
    fn is_placeholder(&self) -> bool;
}

impl PlaceholderMarker for Box<dyn Component> {
    fn is_placeholder(&self) -> bool {
        self.as_any().downcast_ref::<Placeholder>().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A column big enough that the copy under test is not wrapped away.
    const AREA: Rect = Rect::new(0, 0, 44, 12);

    fn rendered(placeholder: &mut Placeholder, title: &str) -> String {
        let theme = Arc::new(Theme::default());
        let mut buf = Buffer::empty(AREA);
        crate::tui::panels::render_hosted(placeholder, &mut buf, AREA, &theme, title);
        (0..AREA.height)
            .map(|y| {
                (0..AREA.width)
                    .map(|x| {
                        buf.cell((x, y))
                            .map(|cell| cell.symbol().to_string())
                            .unwrap_or_default()
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The host's border and title are the panel's identity, and the `●` in
    /// that title is the only thing telling a focused panel from an unfocused
    /// one. A placeholder that painted its own all-borders `Block` over the
    /// whole column destroyed all three at once.
    #[test]
    fn a_hosted_placeholder_keeps_the_panel_title_and_focus_dot() {
        for state in [
            PlaceholderState::Idle,
            PlaceholderState::Loading,
            PlaceholderState::Cannot("nothing to show yet"),
        ] {
            let mut placeholder = Placeholder::new(SidePanelId::Files);
            placeholder.state = state;
            let text = rendered(&mut placeholder, " ● Files ");
            assert!(
                text.contains("● Files"),
                "{state:?}: the panel title and its focus marker are the host's, not the widget's, got {text:?}"
            );
        }
    }

    /// The panel is already where it belongs: there is no box to move into
    /// it, and `Shift+←` means something else on every edge. Telling the user
    /// to leave the panel they are looking at, to run a `/` command, to press
    /// a key, and to come back was three instructions where the state was one
    /// sentence.
    #[test]
    fn a_hosted_placeholder_never_asks_the_user_to_go_and_come_back() {
        // The panels that cannot fill themselves from the manager are the ones
        // that used to be told to name a command. Their loaders are spelled
        // out here on purpose: the assertion is that the copy no longer
        // reaches for them, not that the registry still tracks them.
        let cases = [
            (SidePanelId::Memory, "/memory-search"),
            (SidePanelId::Plan, "/plan"),
            (SidePanelId::Security, "/security-review-show"),
            (SidePanelId::ShellShow, "/shell-list"),
        ];
        for (id, loader) in cases {
            for state in [
                PlaceholderState::Idle,
                PlaceholderState::Loading,
                PlaceholderState::Cannot("nothing to show yet"),
            ] {
                let mut placeholder = Placeholder::new(id);
                placeholder.state = state;
                let text = rendered(&mut placeholder, " x ");
                assert!(
                    !text.contains("Shift+←"),
                    "{id:?}/{state:?} told the user to move a box that is already in place, got {text:?}"
                );
                assert!(
                    !text.contains("to open it"),
                    "{id:?}/{state:?} told the user to go run a command from the input box, got {text:?}"
                );
                assert!(
                    !text.contains(loader),
                    "{id:?}/{state:?} named its own loader instead of describing its state, got {text:?}"
                );
            }
        }
    }

    /// A panel whose load is running is not an empty panel, and one that
    /// cannot act says why. Both were "No X loaded" while the toast said
    /// "Loading…" — the two halves of the same selection disagreeing on screen.
    #[test]
    fn a_declared_state_is_described_rather_than_denied() {
        let mut loading = Placeholder::new(SidePanelId::Goal);
        loading.state = PlaceholderState::Loading;
        let text = rendered(&mut loading, " Goal ");
        assert!(
            text.contains("Loading Goal"),
            "a load in flight must read as a load in flight, got {text:?}"
        );

        let mut blocked = Placeholder::new(SidePanelId::Memory);
        blocked.state = PlaceholderState::Cannot("memory search needs a query");
        let text = rendered(&mut blocked, " Memory ");
        assert!(
            text.contains("memory search needs a query"),
            "a panel that cannot act must say why, got {text:?}"
        );
    }
}

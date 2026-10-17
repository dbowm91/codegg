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
//! explicit placeholder naming the command that loads it, rather than an empty
//! box that looks broken.

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

/// The `/` command that fills a given panel, used by the placeholder.
fn load_command(id: SidePanelId) -> &'static str {
    id.load_command()
}

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

    /// Host an explicit placeholder naming the command that loads this panel.
    pub fn placeholder(id: SidePanelId) -> Self {
        Self::new(id, Box::new(Placeholder { id }))
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
        // The widget's own keymap, not a paraphrase of it. `/files` re-roots
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

/// What is mounted when a panel has no adopted box.
///
/// Named rather than inferred: a panel that is empty because nothing has been
/// docked into it must say so and name the fix, because the alternative — an
/// empty bordered box — is indistinguishable from a rendering bug.
pub struct Placeholder {
    id: SidePanelId,
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
        let body = Paragraph::new(vec![
            ratatui::text::Line::from(""),
            ratatui::text::Line::from(format!("No {} loaded.", self.id.label()))
                .style(Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from(format!("Run  {}  to open it,", load_command(self.id))),
            ratatui::text::Line::from("then Shift+← to move it here."),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(body, area);
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

//! The panel manager: pick what the side column shows, and bind a hotkey to it.
//!
//! One box replaces the old two-button grid. The grid was two unlabelled
//! buttons in a 30-column overlay; this is a filtered list that names each
//! panel, says what it is for, and shows which digit is already bound to it.
//!
//! ## Interaction
//!
//! Two modes, because this box has to answer two different questions and the
//! keys for them collide.
//!
//! **Normal** (how it opens — the manager is reached with `Shift+S`, a
//! Normal-mode chord, and is read before it is typed into):
//!
//! * **`j`/`k`, `↑`/`↓`** — move the highlight.
//! * **`Shift+H/J/K/L`, `Shift+←/→/↑/↓`** — put the highlighted panel on that
//!   edge. This is the point of the mode: "where does it live" becomes the same
//!   gesture it is in the main buffer, so arriving on an edge is one key.
//! * **`Enter`** — load the highlighted panel onto the bracketed edge.
//! * **`1`-`9`, `0`** — bind the highlighted panel to that digit.
//! * **`p`** — step the bracketed edge to the next one.
//! * **`i`/`a`** — start typing a filter.
//!
//! **Insert** (for the filter):
//!
//! * **type** — filter, same as `/help`.
//! * **`Backspace`** — edit the filter.
//! * **`Esc`** — back to Normal with the filter intact.
//!
//! `Esc` from Normal clears the filter first and closes only when there is
//! nothing to clear, so one `Esc` never discards a query the user is midway
//! through typing.
//!
//! Why the directional needed a mode of its own: `Shift+H` is the letter `H`.
//! Resolving it as "dock" unconditionally is safe in a viewer box with no
//! filter and wrong in one with a query buffer, where the user may be typing
//! `http` into the filter. Gating it on Normal is the same rule `HelpDialog`,
//! `/models` and `/sessions` already follow — and it buys something the other
//! boxes do not have, because in Normal the gesture is free to mean *move the
//! highlighted panel* rather than *dock this box*.
//!
//! The cursor stays on a real panel rather than a filtered position, so
//! typing narrows what is *visible* without renumbering what `Enter` and the
//! digits act on. A user who filtered to `chat` and pressed `Enter` expects
//! chat, not "the third of whatever matched".
//!
//! ## Why the layout is a strip and not rows
//!
//! Which edge the panel lives on and which panel is loaded are two different
//! questions, and the panel list answers only the second. Putting the
//! placements into the same navigable list would renumber every panel and
//! make `Enter` mean something new depending on where the cursor had drifted
//! — so the placements get a display-only strip with one key, leaving the
//! panel cursor and the digits exactly as they were.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::state::panel_hotkeys::PanelHotkeys;
use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::dialogs::panel_list::PanelListState;
use crate::tui::input::InputMode;
use crate::tui::panels::{PanelPlacement, SidePanelId};
use crate::tui::theme::Theme;

pub struct PanelManagerDialog {
    theme: Arc<Theme>,
    /// Query, cursor, scroll and mode. Shared with the other panel picker so
    /// `j`/`k`, the filter and the NOR/INS badge cannot mean two different
    /// things in two boxes.
    list: PanelListState,
    /// The user's saved digit bindings, read-only here: saving happens in
    /// the command layer so persistence lives in one place.
    hotkeys: PanelHotkeys,
    /// Which edge the panel is on.
    ///
    /// A copy, not a live borrow: the manager keeps rendering while `p`
    /// rewrites the host underneath it, and a box that had to go back and ask
    /// the app every frame would have to decide what to show in the frame
    /// between the press and the applied change. The command layer is the
    /// only writer, and it owns the truth — this is the box's echo of it.
    placement: PanelPlacement,
    /// Whether `placement` is expanded right now.
    ///
    /// A copy for the same reason `placement` is one. The manager used to
    /// answer "the edge I am describing is collapsed" by *force-expanding it*,
    /// which meant a column the user had deliberately put away came back the
    /// moment they reached for `Shift+S`. Naming the state is the honest fix;
    /// changing the frame to match a strip is not.
    placement_visible: bool,
}

impl Clone for PanelManagerDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            list: self.list.clone(),
            hotkeys: self.hotkeys.clone(),
            placement: self.placement,
            placement_visible: self.placement_visible,
        }
    }
}

impl std::fmt::Debug for PanelManagerDialog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PanelManagerDialog(<filtered panel list>)")
    }
}

impl PanelManagerDialog {
    pub fn new(
        theme: Arc<Theme>,
        hotkeys: PanelHotkeys,
        active: SidePanelId,
        placement: PanelPlacement,
        placement_visible: bool,
    ) -> Self {
        Self {
            theme,
            list: PanelListState::new(active),
            hotkeys,
            placement,
            placement_visible,
        }
    }

    pub fn cursor(&self) -> usize {
        self.list.cursor()
    }

    /// Which mode the filter is in. Exposed for the parity tests, which cannot
    /// see the badge the render draws.
    pub fn input_mode(&self) -> InputMode {
        self.list.mode()
    }

    /// The placement the layout strip is showing.
    pub fn placement(&self) -> PanelPlacement {
        self.placement
    }

    /// Adopt a placement the command layer applied.
    ///
    /// The box has no way to learn this on its own — `TuiMsg` handling runs
    /// outside the mounted component — so the layer pushes it back. Without
    /// this the strip would keep advertising the edge the user just left.
    /// Visibility travels with it for the same reason: a swap can collapse the
    /// edge it leaves, and the strip has to say so.
    pub fn set_placement(&mut self, placement: PanelPlacement, visible: bool) {
        self.placement = placement;
        self.placement_visible = visible;
    }

    /// The highlighted panel.
    pub fn selected(&self) -> Option<SidePanelId> {
        self.list.selected()
    }

    /// True when `panel` passes the current filter.
    ///
    /// Delegated so the manager and the picker can never disagree about what
    /// the filter means.
    pub fn matches(&self, panel: SidePanelId) -> bool {
        self.list.matches(panel)
    }

    /// The live filter text.
    pub fn query(&self) -> &str {
        self.list.query()
    }

    /// The layout strip: every edge, with the live one marked.
    ///
    /// All three are drawn rather than only the current one. `p` cycles, and
    /// a strip showing a single word makes a cycling key read as a glitch —
    /// the user has to see that there are three places to be before "press
    /// `p` again" is a predictable act.
    ///
    /// Kept short because it shares the title row: the box is only about 44
    /// columns wide, and anything longer wraps into the border and loses the
    /// entry at the end — which is the one the user has not tried yet.
    fn layout_spans<'s>(&self, theme: &Arc<Theme>) -> Vec<Span<'s>> {
        let mut spans = vec![Span::styled("  ", Style::default())];
        for placement in PanelPlacement::ALL.iter().copied() {
            let live = placement == self.placement;
            // The bracketed edge says whether it is up. Only the live one does:
            // three states in a 44-column box would wrap, and a wrapped strip
            // loses the entry the user has not tried yet.
            let text = if live {
                if self.placement_visible {
                    format!("[{}]", placement.label())
                } else {
                    format!("[{} collapsed]", placement.label())
                }
            } else {
                placement.label().to_string()
            };
            spans.push(Span::styled(
                format!("{text} "),
                if live {
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.muted)
                },
            ));
        }
        spans.push(Span::styled("· p", Style::default().fg(theme.muted)));
        spans
    }
}

impl Component for PanelManagerDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::{KeyCode, KeyModifiers};

        // The directional is a Normal-mode verb only. In Insert `Shift+H` is
        // the letter `H` and has to reach the filter — checked ahead of the
        // match so the printable arm below cannot shadow it with a bare `H`.
        if self.list.mode() == InputMode::Normal {
            if let Some(dir) = crate::tui::panels::direction_of(key) {
                let panel = self.selected()?;
                return Some(TuiMsg::MovePanelTo {
                    panel,
                    placement: crate::tui::panels::PanelPlacement::named_by(dir)?,
                });
            }
        }

        // The manager's own verbs are answered first; everything shared with the
        // other panel picker (Esc, arrows, `j`/`k`, `i`/`a`, Insert text) is
        // delegated so the two boxes cannot drift.
        match key.code {
            // Loading is the primary act in both modes: a user who filtered to
            // `chat` and pressed Enter wants chat, not to be handed back to a
            // box they have to leave themselves.
            KeyCode::Enter => {
                let panel = self.selected()?;
                return Some(TuiMsg::SelectPanel { panel });
            }
            // The layout strip. Not a filter character and not a panel verb:
            // `p` answers "where does the panel live", which the panel list
            // cannot, and folding it into the list would renumber every
            // panel. Guarded on no modifier for the same reason the digits
            // are — a `Ctrl+P` must keep its own meaning.
            KeyCode::Char('p')
                if self.list.mode() == InputMode::Normal && key.modifiers == KeyModifiers::NONE =>
            {
                return Some(TuiMsg::CyclePanelPlacement);
            }
            // A digit binds the *highlighted* panel. Guarded on no modifier
            // so a Ctrl+digit (which recalls a saved panel) is not swallowed
            // here and recorded as a fresh binding by accident.
            KeyCode::Char(d)
                if self.list.mode() == InputMode::Normal
                    && key.modifiers == KeyModifiers::NONE
                    && d.is_ascii_digit() =>
            {
                let panel = self.selected()?;
                return Some(TuiMsg::SavePanelHotkey { digit: d, panel });
            }
            _ => {}
        }
        self.list.handle_shared_key(key)
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        // Purely local state: every change is the user's own key, so there is
        // nothing for a pushed message to carry.
        None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let buf = frame.buffer_mut();
        let chunks = Layout::vertical([
            Constraint::Length(3), // title (+ the layout strip, on its inner row)
            Constraint::Length(3), // filter
            Constraint::Min(1),    // list
            Constraint::Length(3), // footer
        ])
        .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Panels ")
            .title_style(
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));
        Widget::render(block, chunks[0], buf);

        // The strip shares the title's inner row rather than taking a chunk
        // of its own. A dedicated 3-row block looks right at 40 rows tall and
        // then, at 24, the fixed chunks exceed the box and the *footer* — the
        // one place the keys are written down — is pushed off the bottom
        // entirely. One row inside a block that already exists cannot cost
        // the footer anything.
        let strip = Rect {
            y: chunks[0].y + 1,
            height: 1,
            ..chunks[0]
        };
        Widget::render(
            Paragraph::new(Line::from(self.layout_spans(theme))),
            strip,
            buf,
        );

        let filtering = self.list.filtering();
        Widget::render(
            Paragraph::new(Line::from(self.list.filter_row(theme, "panels"))).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if filtering {
                        theme.primary
                    } else {
                        theme.border
                    })),
            ),
            chunks[1],
            buf,
        );

        // The list. A filtered-out row stays visible but dimmed rather than
        // disappearing: the user needs to see *why* the entry they expected
        // is not there.
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (index, panel) in SidePanelId::ALL.iter().copied().enumerate() {
            let selected = index == self.list.cursor();
            let visible = self.list.matches(panel);
            let saved = self.hotkeys.digit_for(panel);
            let badge = match saved {
                Some(digit) => format!("  ^-{digit}"),
                None => String::new(),
            };
            let marker = if selected { "▸" } else { " " };
            let style = self.list.row_style(theme, index, visible);
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(format!("{:<10}", panel.label()), style),
                Span::styled(
                    format!("{}  ", panel.hint()),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(badge, Style::default().fg(theme.warning)),
            ]));
        }
        Widget::render(
            Paragraph::new(lines)
                .scroll((self.list.scroll(chunks[2].height), 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                ),
            chunks[2],
            buf,
        );

        // Mode-specific, because the keys that differ are exactly the ones a user
        // reaches for and then finds do nothing. In Normal the directional is
        // the point of the box, so it is named first.
        //
        // Budgeted to the box's inner width (~41 columns at the test backend):
        // this is a `Length(3)` chunk, so anything past that is silently cut
        // and the row just ends mid-key. `p` and `0`-`9` are not repeated here
        // because the strip and the list rows already say where they live.
        let footer = match (self.list.mode(), filtering) {
            (InputMode::Insert, true) => "Enter load  Esc normal  Bksp edit",
            (InputMode::Insert, false) => "type to filter  Enter load  Esc normal",
            (InputMode::Normal, true) => "⇧H/J/K/L move  Enter load  Esc clear",
            (InputMode::Normal, false) => "⇧H/J/K/L move  Enter load  i filter",
        };
        Widget::render(
            Paragraph::new(Line::from(Span::styled(
                footer,
                Style::default().fg(theme.secondary),
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            ),
            chunks[3],
            buf,
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::PanelManager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn dialog() -> PanelManagerDialog {
        PanelManagerDialog::new(
            Arc::new(Theme::default()),
            PanelHotkeys::default(),
            SidePanelId::Session,
            PanelPlacement::Right,
            true,
        )
    }

    #[test]
    fn opens_on_the_live_panel_so_enter_is_a_no_op() {
        let d = PanelManagerDialog::new(
            Arc::new(Theme::default()),
            PanelHotkeys::default(),
            SidePanelId::Terminal,
            PanelPlacement::Right,
            true,
        );
        assert_eq!(d.selected(), Some(SidePanelId::Terminal));
    }

    #[test]
    fn enter_selects_the_highlighted_panel() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            d.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiMsg::SelectPanel {
                panel: SidePanelId::Chat
            })
        );
    }

    #[test]
    fn the_highlight_is_clamped_rather_than_wrapping() {
        let mut d = dialog(); // opens at index 0
        for _ in 0..10 {
            d.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
        }
        assert_eq!(d.cursor(), 0, "must not wrap past the first panel");
    }

    /// Filtering is an Insert-mode act. Opening in Normal is what makes the
    /// box readable on arrival: `j`/`k` walk the highlight instead of silently
    /// typing letters nobody can see.
    #[test]
    fn the_box_opens_in_normal_so_the_list_is_readable_first() {
        let d = dialog();
        assert_eq!(d.input_mode(), InputMode::Normal);
    }

    #[test]
    fn typing_narrows_what_is_visible_without_moving_the_highlight() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE)); // highlight Chat
        d.handle_key(key(KeyCode::Char('i'), KeyModifiers::NONE)); // to the filter
        for ch in "term".chars() {
            d.handle_key(key(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        // Terminal does not match "term"? It does. Chat is filtered out but
        // the highlight is still where it was.
        assert!(!d.matches(SidePanelId::Chat));
        assert!(d.matches(SidePanelId::Terminal));
        assert_eq!(d.cursor(), 1, "filtering must not renumber the cursor");
    }

    /// In Normal a printable is a verb, not text. Without this the box would
    /// swallow every letter the user typed at it.
    #[test]
    fn a_printable_in_normal_is_not_a_filter_character() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
        d.handle_key(key(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(d.query(), "", "Normal-mode keys must not narrow the list");
    }

    #[test]
    fn a_digit_binds_the_highlighted_panel() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            d.handle_key(key(KeyCode::Char('1'), KeyModifiers::NONE)),
            Some(TuiMsg::SavePanelHotkey {
                digit: '1',
                panel: SidePanelId::Chat
            })
        );
    }

    /// `Esc` walks out one step at a time — Insert → Normal → clear → close —
    /// so one press never discards a query the user is midway through typing.
    #[test]
    fn escape_leaves_insert_before_it_clears_or_closes() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
        d.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));

        assert_eq!(
            d.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            None,
            "one Esc must not discard a query mid-typing"
        );
        assert_eq!(d.input_mode(), InputMode::Normal);
        assert_eq!(d.query(), "c", "the query survives Esc");

        assert_eq!(
            d.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            None,
            "a second Esc clears rather than closing"
        );
        assert_eq!(d.query(), "", "the query is cleared");

        assert_eq!(
            d.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(TuiMsg::CloseDialog),
            "a third Esc, with nothing left to clear, closes"
        );
    }

    /// The reason the box needs a mode of its own: in Normal the gesture is
    /// free, so it means "put this panel over there" — the same gesture the
    /// main buffer uses, not a manager-only verb with its own vocabulary.
    #[test]
    fn the_normal_directional_moves_the_highlighted_panel() {
        let mut d = dialog();
        assert_eq!(
            d.handle_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT)),
            Some(TuiMsg::MovePanelTo {
                panel: SidePanelId::Session,
                placement: PanelPlacement::Left,
            })
        );
        assert_eq!(
            d.handle_key(key(KeyCode::Char('L'), KeyModifiers::SHIFT)),
            Some(TuiMsg::MovePanelTo {
                panel: SidePanelId::Session,
                placement: PanelPlacement::Right,
            })
        );
        // The arrow twin resolves identically, for terminals that disagree
        // about how a shifted letter arrives.
        assert_eq!(
            d.handle_key(key(KeyCode::Left, KeyModifiers::SHIFT)),
            Some(TuiMsg::MovePanelTo {
                panel: SidePanelId::Session,
                placement: PanelPlacement::Left,
            })
        );
    }

    /// `Shift+K` names no edge — there is no top — so it must do nothing at
    /// all rather than inventing a destination.
    #[test]
    fn an_upward_directional_moves_nothing() {
        let mut d = dialog();
        assert_eq!(
            d.handle_key(key(KeyCode::Char('K'), KeyModifiers::SHIFT)),
            None
        );
    }

    /// The guard that makes the split necessary: in Insert, `Shift+H` is the
    /// letter `H` and has to reach the filter.
    #[test]
    fn the_directional_is_a_filter_character_in_insert() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
        d.handle_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT));
        assert_eq!(d.input_mode(), InputMode::Insert);
        assert_eq!(d.query(), "H", "the H reached the filter");
    }

    /// A bare arrow is not the gesture. In the manager it walks the highlight
    /// in either mode, because it is never a letter.
    #[test]
    fn a_bare_arrow_walks_the_list_in_insert() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(d.cursor(), 1);
        assert_eq!(d.query(), "", "and did not filter anything");
    }

    // --------------------------------------------------------------- layout

    /// `p` asks where the panel lives, which the panel list cannot answer.
    /// It must not also be a filter character, or the layout would be
    /// unreachable while the user is searching for a panel.
    #[test]
    fn p_asks_for_the_layout_rather_than_filtering() {
        let mut d = dialog();
        assert_eq!(
            d.handle_key(key(KeyCode::Char('p'), KeyModifiers::NONE)),
            Some(TuiMsg::CyclePanelPlacement)
        );
    }

    /// A `Ctrl+P` is a different gesture that something else may own.
    #[test]
    fn a_modified_p_is_not_the_layout_key() {
        let mut d = dialog();
        // Unmodified handling falls through to the filter branch below.
        assert_ne!(
            d.handle_key(key(KeyCode::Char('P'), KeyModifiers::CONTROL)),
            Some(TuiMsg::CyclePanelPlacement)
        );
    }

    /// Cycling the layout must not move the panel highlight: the two answer
    /// different questions, and folding them together would renumber every
    /// panel row for a change that touches none of them.
    #[test]
    fn the_layout_is_independent_of_the_panel_cursor() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        let cursor = d.cursor();
        d.handle_key(key(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(d.cursor(), cursor);
    }

    /// The strip has to be able to describe an applied change it did not
    /// make itself — `TuiMsg` handling runs outside the mounted box.
    #[test]
    fn the_box_adopts_the_placement_the_command_layer_applied() {
        let mut d = dialog();
        assert_eq!(d.placement(), PanelPlacement::Right);
        d.set_placement(PanelPlacement::Bottom, true);
        assert_eq!(d.placement(), PanelPlacement::Bottom);
    }

    /// All three edges are drawn, with the live one marked. A strip showing
    /// one word makes a cycling key read as a glitch.
    #[test]
    fn the_layout_strip_names_every_placement() {
        let theme = Arc::new(Theme::default());
        let d = dialog();
        let text: String = d
            .layout_spans(&theme)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        for placement in PanelPlacement::ALL {
            assert!(
                text.contains(placement.label()),
                "{placement:?} is missing from the strip: {text}"
            );
        }
        assert!(
            text.contains("[Right]"),
            "the live placement must be marked: {text}"
        );
    }

    #[test]
    fn the_strip_moves_its_mark_with_the_placement() {
        let theme = Arc::new(Theme::default());
        let mut d = dialog();
        d.set_placement(PanelPlacement::Left, true);
        let text: String = d
            .layout_spans(&theme)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("[Left]"), "{text}");
        assert!(!text.contains("[Right]"), "{text}");
    }
}

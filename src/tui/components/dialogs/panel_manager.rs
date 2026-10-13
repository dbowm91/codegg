//! The panel manager: pick what the side column shows, and bind a hotkey to it.
//!
//! One box replaces the old two-button grid. The grid was two unlabelled
//! buttons in a 30-column overlay; this is a filtered list that names each
//! panel, says what it is for, and shows which digit is already bound to it.
//!
//! ## Interaction
//!
//! * **type** — filter, same as `/help`.
//! * **`j`/`k`, `↑`/`↓`** — move the highlight.
//! * **`Enter`** — load the highlighted panel and take focus.
//! * **`1`-`9`, `0`** — bind the highlighted panel to that digit, persisting
//!   it to `codegg.json`.
//! * **`p`** — move the panel to the next edge (right → left → bottom).
//! * **`Esc`** — clear the filter, then close.
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
use crate::tui::panels::{PanelPlacement, SidePanelId};
use crate::tui::theme::Theme;

/// Upper bound on the filter text, so a held key cannot grow this without
/// limit.
const MAX_QUERY_LEN: usize = 48;

pub struct PanelManagerDialog {
    theme: Arc<Theme>,
    query: String,
    /// Index into [`SidePanelId::ALL`], not into the filtered view.
    cursor: usize,
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
}

impl Clone for PanelManagerDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            query: self.query.clone(),
            cursor: self.cursor,
            hotkeys: self.hotkeys.clone(),
            placement: self.placement,
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
    ) -> Self {
        Self {
            theme,
            query: String::new(),
            // Opens on the live panel, so `Enter` with no movement is a no-op
            // rather than a silent jump to whatever happens to be first.
            cursor: active.index(),
            hotkeys,
            placement,
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
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
    pub fn set_placement(&mut self, placement: PanelPlacement) {
        self.placement = placement;
    }

    /// The highlighted panel.
    pub fn selected(&self) -> Option<SidePanelId> {
        SidePanelId::ALL.get(self.cursor).copied()
    }

    /// First visible row, scrolled so `cursor` is always on screen.
    ///
    /// There are more panels than rows — 33 entries into roughly ten lines —
    /// so without this `j` walks the highlight straight off the bottom of
    /// the box and `Enter` then commits a panel nobody can see was selected.
    /// The highlight is the only thing saying which panel `Enter` will load,
    /// so a highlight that has scrolled away is a silent mis-selection.
    fn scroll(&self, list_height: u16) -> u16 {
        // Two rows go to the list's own border.
        let page = list_height.saturating_sub(2);
        if page == 0 {
            return 0;
        }
        let total = SidePanelId::ALL.len() as u16;
        // Never past the end: with fewer rows than panels, sitting at the
        // bottom is what puts the *last* entries on screen.
        let max_offset = total.saturating_sub(page);
        let cursor = self.cursor as u16;
        if cursor < page {
            return 0;
        }
        cursor
            .saturating_add(1)
            .saturating_sub(page)
            .min(max_offset)
    }

    /// True when `panel` passes the current filter.
    fn matches(&self, panel: SidePanelId) -> bool {
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        query.split_whitespace().all(|term| {
            panel.label().to_lowercase().contains(term)
                || panel.title().to_lowercase().contains(term)
                || panel.hint().to_lowercase().contains(term)
        })
    }

    /// Move the highlight by `delta`, clamped to the registry.
    fn move_cursor(&mut self, delta: isize) {
        let len = SidePanelId::ALL.len() as isize;
        if len == 0 {
            return;
        }
        let next = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = next as usize;
    }

    fn push_query(&mut self, ch: char) {
        if self.query.chars().count() < MAX_QUERY_LEN {
            self.query.push(ch);
        }
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
            spans.push(Span::styled(
                if live {
                    format!("[{}] ", placement.label())
                } else {
                    format!("{} ", placement.label())
                },
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
        match key.code {
            KeyCode::Esc => {
                if self.query.is_empty() {
                    return Some(TuiMsg::CloseDialog);
                }
                self.query.clear();
            }
            KeyCode::Enter => {
                let panel = self.selected()?;
                return Some(TuiMsg::SelectPanel { panel });
            }
            KeyCode::Up => self.move_cursor(-1),
            KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Backspace => {
                self.query.pop();
            }
            // The layout strip. Not a filter character and not a panel verb:
            // `p` answers "where does the panel live", which the panel list
            // cannot, and folding it into the list would renumber every
            // panel. Guarded on no modifier for the same reason the digits
            // are — a `Ctrl+P` must keep its own meaning.
            KeyCode::Char('p') if key.modifiers == KeyModifiers::NONE => {
                return Some(TuiMsg::CyclePanelPlacement);
            }
            // A digit binds the *highlighted* panel. Guarded on no modifier
            // so a Ctrl+digit (which recalls a saved panel) is not swallowed
            // here and recorded as a fresh binding by accident.
            KeyCode::Char(d) if key.modifiers == KeyModifiers::NONE && d.is_ascii_digit() => {
                let panel = self.selected()?;
                return Some(TuiMsg::SavePanelHotkey { digit: d, panel });
            }
            KeyCode::Char(ch)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.push_query(ch);
            }
            _ => {}
        }
        None
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

        let filtering = !self.query.trim().is_empty();
        let filter_spans = if filtering {
            vec![
                Span::styled("search ", Style::default().fg(theme.muted)),
                Span::styled(
                    format!("{}_", self.query),
                    Style::default().fg(theme.primary),
                ),
            ]
        } else {
            vec![Span::styled(
                "type to filter panels…",
                Style::default().fg(theme.muted),
            )]
        };
        Widget::render(
            Paragraph::new(Line::from(filter_spans)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            ),
            chunks[1],
            buf,
        );

        // The list. A filtered-out row stays visible but dimmed rather than
        // disappearing: the user needs to see *why* the entry they expected
        // is not there.
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (index, panel) in SidePanelId::ALL.iter().copied().enumerate() {
            let selected = index == self.cursor;
            let visible = self.matches(panel);
            let saved = self.hotkeys.digit_for(panel);
            let badge = match saved {
                Some(digit) => format!("  ^-{digit}"),
                None => String::new(),
            };
            let marker = if selected { "▸" } else { " " };
            let style = if !visible {
                Style::default().fg(theme.border)
            } else if selected {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };
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
                .scroll((self.scroll(chunks[2].height), 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                ),
            chunks[2],
            buf,
        );

        let footer = if filtering {
            "Enter load  |  p move panel  |  0-9 bind hotkey  |  Esc clear filter  |  Esc close"
        } else {
            "Enter load  |  p move panel  |  0-9 bind hotkey  |  type to filter  |  Esc close"
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
        )
    }

    #[test]
    fn opens_on_the_live_panel_so_enter_is_a_no_op() {
        let d = PanelManagerDialog::new(
            Arc::new(Theme::default()),
            PanelHotkeys::default(),
            SidePanelId::Terminal,
            PanelPlacement::Right,
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

    #[test]
    fn typing_narrows_what_is_visible_without_moving_the_highlight() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE)); // highlight Chat
        for ch in "term".chars() {
            d.handle_key(key(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        // Terminal does not match "term"? It does. Chat is filtered out but
        // the highlight is still where it was.
        assert!(!d.matches(SidePanelId::Chat));
        assert!(d.matches(SidePanelId::Terminal));
        assert_eq!(d.cursor(), 1, "filtering must not renumber the cursor");
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

    #[test]
    fn escape_clears_the_filter_before_it_closes() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(
            d.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            None,
            "one Esc must not discard a query mid-typing"
        );
        assert_eq!(
            d.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(TuiMsg::CloseDialog)
        );
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
        d.set_placement(PanelPlacement::Bottom);
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
        d.set_placement(PanelPlacement::Left);
        let text: String = d
            .layout_spans(&theme)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("[Left]"), "{text}");
        assert!(!text.contains("[Right]"), "{text}");
    }
}

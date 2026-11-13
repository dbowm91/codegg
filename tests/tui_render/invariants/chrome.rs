//! M002 — one panel chrome: I3b, and the control case that proves adoption
//! did not move the two panels whose chrome already matched the policy.
//!
//! ## What I3b asserts
//!
//! I3a (in [`super::focus`]) asks only that a focused surface *differ* from an
//! unfocused one, because a `●` was enough to satisfy it. **I3b** asks that the
//! difference be a **border-style or title-style** change, uniformly, wherever
//! chrome is drawn — the invariant a glyph-only marker cannot satisfy. The two
//! halves are separate because they become true at different times: I3a held at
//! the M001 baseline, I3b could not hold until every panel painted its edge
//! through `PanelChrome`.
//!
//! The sweep reads the production panel path (`SidePanelHost::render_at`) and
//! compares two renders of the same surface cell by cell, symbol and style. A
//! cell on the panel's edge that keeps its symbol and changes its style is the
//! evidence; a surface that changed only glyphs, or only its body, fails.
//!
//! ## Why below the chrome threshold is not asserted
//!
//! `PanelChrome::has_chrome` declines below 12×4, where a border and a title
//! would consume the content they frame. A borderless column has no edge to
//! style, so the sweep measures the four sizes §10 names, all above it, and
//! [`every_panel_below_the_chrome_threshold_paints_no_chrome_border`] checks
//! the decline separately.
//!
//! ## The control case
//!
//! Chat and Terminal already implemented this policy by hand. Their frames at
//! 80×24 and 120×40, focused and unfocused, are therefore recorded as digests
//! measured at the pre-adoption baseline (`7642b9e38`) and compared on every
//! run: if adoption moved either panel, the primitive is wrong, not the panel.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use codegg::tui::panels::{chrome::has_chrome, PanelPlacement, SidePanelId};
use codegg::tui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Widget};

use super::capture::{Cell, Frame};

/// The sizes §10's integration list names. Every one is above the chrome
/// threshold, so every panel is expected to carry an edge.
const CHROME_GRID: &[(u16, u16)] = &[(20, 6), (40, 12), (80, 24), (120, 40)];

fn theme() -> Arc<Theme> {
    Arc::new(Theme::dark())
}

/// Whether a cell sits on the panel's edge: the top or bottom rule, or either
/// vertical rule. The title is on the top rule, so a title-style change lands
/// here too.
fn on_edge(cell: &Cell, frame: &Frame) -> bool {
    cell.y == 0 || cell.y + 1 == frame.height || cell.x == 0 || cell.x + 1 == frame.width
}

/// I3b: at least one edge cell must keep its glyph and change its style.
///
/// A style-only difference is required rather than any difference: a moved
/// title, a new glyph or a body recolour are all visible changes that do not
/// answer "is this panel focused?" in one channel.
pub fn focus_style_changes_the_edge(focused: &Frame, unfocused: &Frame) -> Result<(), String> {
    let differing = focused.differing(unfocused);
    if differing.is_empty() {
        return Err("focused and unfocused render identically".to_string());
    }
    let styled_edge = differing.iter().any(|(left, right)| {
        left.symbol == right.symbol && left.style() != right.style() && on_edge(left, focused)
    });
    if styled_edge {
        Ok(())
    } else {
        Err(format!(
            "the {} differing cells do not include an edge cell whose style changed; \
             focus is marked by glyphs or by the body, not by the border or title style",
            differing.len()
        ))
    }
}

/// A stable digest of every cell's glyph and style.
///
/// The encoding is fixed: one string of rows joined by a newline, each row's
/// cells joined by a tab. The recorded digests were measured through it, so
/// changing it invalidates them rather than the panels.
fn encode(frame: &Frame) -> String {
    (0..frame.height)
        .map(|y| {
            frame
                .row_cells(y)
                .iter()
                .map(|cell| {
                    format!(
                        "{}#{:?}#{:?}#{:?}",
                        cell.symbol, cell.fg, cell.bg, cell.modifiers
                    )
                })
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn digest(frame: &Frame) -> u64 {
    let mut hasher = DefaultHasher::new();
    encode(frame).hash(&mut hasher);
    hasher.finish()
}

#[test]
fn every_panel_marks_focus_with_an_edge_style_change() {
    let theme = theme();
    let mut panels = super::focus::ready_panels(&theme);
    for panel in SidePanelId::ALL {
        for placement in PanelPlacement::ALL {
            panels.host.dock(*panel, *placement);
            panels.host.set_visible_at(*placement, true);
            assert_eq!(
                panels.host.panel_at(*placement),
                *panel,
                "the sweep must measure the panel it names"
            );
            for (width, height) in CHROME_GRID {
                let area = Rect::new(0, 0, *width, *height);
                let focused = panels.capture(*placement, area, true);
                let unfocused = panels.capture(*placement, area, false);
                assert_eq!(
                    focus_style_changes_the_edge(&focused, &unfocused),
                    Ok(()),
                    "{panel:?} on {placement:?} at {width}x{height} does not mark focus \
                     with a border-style or title-style change"
                );
            }
        }
    }
}

/// The pre-adoption Agents panel: a fixed border, and a title that was always
/// `primary` + `BOLD`. It is the defect I3b exists to catch, restated so the
/// assertion is proved to bite rather than assumed to.
fn pre_adoption_agents(focused: bool) -> Frame {
    let theme = theme();
    let area = Rect::new(0, 0, 40, 16);
    let title = if focused {
        " AGENTS ● ".to_string()
    } else {
        " AGENTS ".to_string()
    };
    let block = Block::default()
        .title(ratatui::text::Span::styled(
            title,
            Style::default()
                .fg(theme.primary)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.background));
    let mut buf = Buffer::empty(area);
    block.render(area, &mut buf);
    Frame::from_buffer(&buf)
}

#[test]
fn the_i3b_check_rejects_the_pre_adoption_agents_panel() {
    let verdict =
        focus_style_changes_the_edge(&pre_adoption_agents(true), &pre_adoption_agents(false));
    let message = verdict.expect_err(
        "the pre-adoption Agents panel changed only the `●` glyph, so I3b must reject it",
    );
    assert!(
        message.contains("edge cell whose style changed"),
        "the rejection must name the property that failed, got: {message}"
    );
}

#[test]
fn chat_and_terminal_corpora_stay_byte_identical_to_the_pre_adoption_baseline() {
    // Measured at `7642b9e38`, before `PanelChrome` was adopted. These are the
    // control case: both panels already implemented the policy, so a changed
    // digest means the shared primitive reproduced something else.
    let expected: &[(SidePanelId, u16, u16, bool, u64)] = &[
        (SidePanelId::Chat, 80, 24, true, 3167304810658960557),
        (SidePanelId::Chat, 80, 24, false, 783957252602921977),
        (SidePanelId::Chat, 120, 40, true, 3754238857442777879),
        (SidePanelId::Chat, 120, 40, false, 6330799720704254993),
        (SidePanelId::Terminal, 80, 24, true, 15220833477778996678),
        (SidePanelId::Terminal, 80, 24, false, 10344610211801457318),
        (SidePanelId::Terminal, 120, 40, true, 4788885108008190873),
        (SidePanelId::Terminal, 120, 40, false, 15950482605221617052),
    ];
    let theme = theme();
    let mut panels = super::focus::ready_panels(&theme);
    for (panel, width, height, focused, hash) in expected {
        panels.host.dock(*panel, PanelPlacement::Right);
        panels.host.set_visible_at(PanelPlacement::Right, true);
        let frame = panels.capture(
            PanelPlacement::Right,
            Rect::new(0, 0, *width, *height),
            *focused,
        );
        assert_eq!(
            digest(&frame),
            *hash,
            "{panel:?} at {width}x{height} (focused={focused}) moved; the control case says \
             the primitive is wrong, not the panel"
        );
    }
}

#[test]
fn the_chrome_primitive_reproduces_the_block_chat_and_terminal_hand_rolled() {
    // The same claim as the digests above, localised: if this fails, the
    // primitive changed; if only the digests fail, a panel's content did.
    let theme = theme();
    for (width, height) in [(80u16, 24u16), (120u16, 40u16)] {
        for focused in [true, false] {
            let area = Rect::new(0, 0, width, height);
            let hand_rolled = legacy_chat_block(area, &theme, focused);
            let shared = shared_chrome(area, &theme, focused);
            assert_eq!(
                shared.differing(&hand_rolled),
                Vec::new(),
                "the shared chrome differs from Chat's pre-adoption block at \
                 {width}x{height} (focused={focused})"
            );
        }
    }
}

fn legacy_chat_block(area: Rect, theme: &Theme, focused: bool) -> Frame {
    let label = SidePanelId::Chat.label();
    let title = if focused {
        format!(" {label} ● ")
    } else {
        format!(" {label} ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { theme.primary } else { theme.border }))
        .title(title)
        .title_style(Style::default().fg(if focused { theme.primary } else { theme.muted }))
        .style(Style::default().bg(theme.background));
    let mut buf = Buffer::empty(area);
    block.render(area, &mut buf);
    Frame::from_buffer(&buf)
}

fn shared_chrome(area: Rect, theme: &Theme, focused: bool) -> Frame {
    let mut buf = Buffer::empty(area);
    codegg::tui::panels::chrome::PanelChrome::new(SidePanelId::Chat.label(), focused, theme)
        .draw(area, &mut buf);
    Frame::from_buffer(&buf)
}

/// Panels whose **body** draws a border of its own, so a corner glyph below the
/// chrome threshold is content rather than chrome.
///
/// Help is the only one: its body is the four-section dialog, whose first
/// section is a bordered title box. That box is content — the panel's own edge
/// still declines — and recording it here keeps the check two-directional: a
/// new body that starts painting a border below the threshold is a failure,
/// and so is a recorded one that stops.
const BODY_DRAWS_A_BORDER: &[SidePanelId] = &[SidePanelId::Help];

fn shows_a_corner(frame: &Frame) -> bool {
    let (right, bottom) = (frame.width - 1, frame.height - 1);
    [(0, 0), (right, 0), (0, bottom), (right, bottom)]
        .iter()
        .any(|(x, y)| matches!(frame.at(*x, *y).symbol.as_str(), "┌" | "┐" | "└" | "┘"))
}

#[test]
fn every_panel_below_the_chrome_threshold_paints_no_chrome_border() {
    let theme = theme();
    let mut panels = super::focus::ready_panels(&theme);
    let area = Rect::new(0, 0, 11, 3);
    assert!(
        !has_chrome(area),
        "the fixture is meant to be below the threshold"
    );
    let mut bordered_bodies = Vec::new();
    for panel in SidePanelId::ALL {
        panels.host.dock(*panel, PanelPlacement::Right);
        panels.host.set_visible_at(PanelPlacement::Right, true);
        assert_eq!(
            panels.host.panel_at(PanelPlacement::Right),
            *panel,
            "the sweep must measure the panel it names"
        );
        let frame = panels.capture(PanelPlacement::Right, area, false);
        assert_eq!(
            frame.len(),
            usize::from(area.width) * usize::from(area.height),
            "{panel:?} returned a short 11x3 frame"
        );
        if shows_a_corner(&frame) {
            bordered_bodies.push(*panel);
        }
        // A degenerate area must return a full frame rather than panic; the
        // length is the property a panic cannot satisfy.
        let degenerate = panels.capture(PanelPlacement::Right, Rect::new(0, 0, 1, 1), false);
        assert_eq!(degenerate.len(), 1, "{panel:?} returned a short 1x1 frame");
    }

    bordered_bodies.sort_by_key(|panel| panel.label());
    let mut recorded = BODY_DRAWS_A_BORDER.to_vec();
    recorded.sort_by_key(|panel| panel.label());
    assert_eq!(
        bordered_bodies, recorded,
        "the set of panels whose body shows a border corner below the chrome \
         threshold changed; a corner here is content, and an unrecorded one \
         means a body started drawing chrome of its own"
    );
}

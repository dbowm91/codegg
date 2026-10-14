//! Yazi-style panel resizing: the rules, with no dependency on `App`.
//!
//! The gesture is deliberately **outward-only**. A surface — a focused panel,
//! or the main buffer — may only drag its *own* borders further out; it can
//! never pull one in. The border it drags is shared with whatever sits on the
//! other side, so the neighbour shrinks:
//!
//! | subject | `h` | `j` | `k` | `l` |
//! |---|---|---|---|---|
//! | chat on the **right** | its left border out → chat grows | frame edge | frame edge | frame edge |
//! | panel on the **left** | frame edge | frame edge | frame edge | its right border out → grows |
//! | panel on the **bottom** | frame edge | frame edge | its top border up → grows | frame edge |
//! | main **buffer** | left panel's right border → left panel shrinks | bottom panel's top border → bottom panel shrinks | nothing is ever above | right panel's left border → right panel shrinks |
//!
//! Two properties make this the rule rather than a set of special cases:
//!
//! 1. **Only shared borders move.** A column already owns its left edge to the
//!    frame, so a left-placed panel grows with `l` and *not* with `h` — the
//!    mirror of a right-placed panel growing with `h`. Pressing the other key
//!    is a no-op, never a shrink.
//! 2. **One key, one meaning.** [`resolve_resize`] is asked "which panel's
//!    size changes, and in which direction", so the frame cannot answer the
//!    same key two ways depending on which surface the reader was on.
//!
//! Sizes are per edge rather than one shared `sidebar_width`, because "make
//! the right column wider" and "make the left column wider" are different
//! requests and a single value can only satisfy one of them at a time.

use crate::tui::layout::TuiLayout;
use crate::tui::panels::PanelPlacement;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Columns a single resize keypress moves a column edge.
///
/// One cell per press, matching the gesture a yazi user already has in their
/// fingers. A larger step would make it impossible to nudge a border that is
/// one column too wide.
pub const COLUMN_RESIZE_STEP: u16 = 1;

/// Rows a single resize keypress moves the bottom edge.
pub const ROW_RESIZE_STEP: u16 = 1;

/// Narrowest a column may be dragged, in cells.
///
/// Not a "you may not resize below this" policy — it is the floor past which a
/// column stops being readable at all, and below which the frame would rather
/// collapse the edge than show a three-character panel.
pub const MIN_COLUMN_WIDTH: u16 = 16;

/// Widest a column may be dragged, in cells.
///
/// A safety rail rather than a frame-aware limit: `split` already refuses to
/// show a column that would starve the transcript, and this keeps a single
/// drag from parking the value somewhere unrecoverable without a reset.
pub const MAX_COLUMN_WIDTH: u16 = 120;

/// Fewest rows the bottom panel may be dragged to.
///
/// Matches the layout's own `MIN_BOTTOM_PANEL_HEIGHT`: a bottom panel is a row
/// of the frame, and the floor for "enough rows to be a panel" is not the same
/// number as the floor for "enough columns to be a column". Sharing one bound
/// between the two axes silently refused the first few legitimate drags of the
/// bottom edge — 14 rows is a perfectly good panel and is below any column
/// floor.
pub const MIN_PANEL_ROWS: u16 = 8;

/// Most rows the bottom panel may be dragged to.
pub const MAX_PANEL_ROWS: u16 = 200;

/// Which surface a resize key acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeSubject {
    /// The main transcript buffer — the surface the badge calls `LLM`.
    Main,
    /// The focused panel, by the edge it is docked to.
    Panel(PanelPlacement),
}

/// Which border of the subject a key drags outward.
///
/// The letters are the vim/Helix ones and mean what they mean everywhere else
/// in this TUI: `h` left, `j` down, `k` up, `l` right. What is new is that
/// here they name the subject's *own* border in that direction rather than a
/// neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    /// The subject's left border moves left.
    Left,
    /// The subject's right border moves right.
    Right,
    /// The subject's top border moves up.
    Top,
    /// The subject's bottom border moves down.
    Bottom,
}

/// A panel whose size changes, and which way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeDelta {
    /// The edge whose panel changes size.
    pub placement: PanelPlacement,
    /// `true` when the named panel gets bigger, `false` when the subject
    /// growing means the named panel shrinks.
    pub grow: bool,
}

/// Resolve one resize key against the frame's current layout.
///
/// Returns `None` when the key names a border that is a frame edge, or an edge
/// with nothing on it. `None` is a *silent* no-op by design: the frame has no
/// way to say "that border does not exist" from here, and refusing loudly for
/// a key that is correct on the other subject would make the same key feel
/// broken.
pub fn resolve_resize(
    subject: ResizeSubject,
    edge: ResizeEdge,
    occupied: &[PanelPlacement],
) -> Option<ResizeDelta> {
    let showing = |placement: PanelPlacement| occupied.contains(&placement);
    match subject {
        // A panel can only drag a border it shares with something. A column
        // spans the full frame height and touches the frame on three of its
        // four sides; the bottom row spans the full width and touches it on
        // three more. The one shared border is the one that moves.
        ResizeSubject::Panel(placement) => {
            let shared = match placement {
                PanelPlacement::Right => ResizeEdge::Left,
                PanelPlacement::Left => ResizeEdge::Right,
                PanelPlacement::Bottom => ResizeEdge::Top,
            };
            (edge == shared).then_some(ResizeDelta {
                placement,
                grow: true,
            })
        }
        // The main buffer touches the frame on its top and, usually, its left;
        // every other side is shared with a panel. Growing the buffer means
        // shrinking whichever panel is across that border.
        ResizeSubject::Main => {
            let (placement, showing) = match edge {
                ResizeEdge::Left => (PanelPlacement::Left, showing(PanelPlacement::Left)),
                ResizeEdge::Right => (PanelPlacement::Right, showing(PanelPlacement::Right)),
                ResizeEdge::Bottom => (PanelPlacement::Bottom, showing(PanelPlacement::Bottom)),
                // Nothing can ever dock above the buffer.
                ResizeEdge::Top => (PanelPlacement::Left, false),
            };
            showing.then_some(ResizeDelta {
                placement,
                grow: false,
            })
        }
    }
}

/// Resolve a key to the border it drags, or `None` when it is not a resize key.
///
/// Bare `h/j/k/l` only. The arrows are deliberately absent: they are the
/// panel-docking gesture family and are already claimed by the host before
/// resize mode is consulted, so claiming them here too would be a second rule
/// for one key.
pub fn resize_edge_of(key: KeyEvent) -> Option<ResizeEdge> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Char('h') => Some(ResizeEdge::Left),
        KeyCode::Char('j') => Some(ResizeEdge::Bottom),
        KeyCode::Char('k') => Some(ResizeEdge::Top),
        KeyCode::Char('l') => Some(ResizeEdge::Right),
        _ => None,
    }
}

/// Apply one [`ResizeDelta`] to the layout, returning whether anything moved.
///
/// Refuses rather than clamping to a degenerate size, because the alternative
/// is a key that appears dead at the limit for no stated reason. The limits
/// live here so every surface agrees on them.
///
/// Takes the whole [`TuiLayout`] rather than its `config` for exactly one
/// reason: the bottom panel is sized as a *percentage* until someone drags it,
/// and converting that to an exact row count needs the frame height the layout
/// recorded on the last render.
pub fn apply_resize(layout: &mut TuiLayout, delta: ResizeDelta) -> bool {
    match delta.placement {
        PanelPlacement::Left => step_within(
            layout.config.column_width(PanelPlacement::Left),
            delta.grow,
            COLUMN_RESIZE_STEP,
            MIN_COLUMN_WIDTH,
            MAX_COLUMN_WIDTH,
        )
        .is_some_and(|width| {
            layout.config.set_left_panel_width(width);
            true
        }),
        PanelPlacement::Right => step_within(
            layout.config.column_width(PanelPlacement::Right),
            delta.grow,
            COLUMN_RESIZE_STEP,
            MIN_COLUMN_WIDTH,
            MAX_COLUMN_WIDTH,
        )
        .is_some_and(|width| {
            layout.config.set_right_panel_width(width);
            true
        }),
        PanelPlacement::Bottom => {
            // Seed the exact row count from whatever the ratio produced for the
            // frame we last rendered. Without this the first drag would have no
            // starting value to move away from.
            let current = layout
                .config
                .bottom_panel_rows
                .or_else(|| layout.bottom_panel_height(layout.frame_height()));
            let Some(current) = current else {
                return false;
            };
            let Some(next) = step_within(
                current,
                delta.grow,
                ROW_RESIZE_STEP,
                MIN_PANEL_ROWS,
                MAX_PANEL_ROWS,
            ) else {
                return false;
            };
            layout.config.bottom_panel_rows = Some(next);
            true
        }
    }
}

/// One step in one direction, refused when it would leave the legal range.
///
/// `checked_add`/`checked_sub` rather than `saturating_*` on purpose: at
/// `u16::MAX` a saturating add reports success while reporting the same value,
/// which turns "the edge is already as far as it goes" into a silent no-op
/// indistinguishable from a dropped key. `None` is the honest answer, and the
/// caller leaves the value exactly where it was.
fn step_within(current: u16, grow: bool, step: u16, min: u16, max: u16) -> Option<u16> {
    let next = if grow {
        current.checked_add(step)?
    } else {
        current.checked_sub(step)?
    };
    (min..=max).contains(&next).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_resize, resize_edge_of, resolve_resize, PanelPlacement, ResizeEdge, ResizeSubject,
        COLUMN_RESIZE_STEP, MAX_COLUMN_WIDTH, MIN_COLUMN_WIDTH,
    };
    use crate::tui::layout::{LayoutConfig, TuiLayout};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    const BOTH_COLUMNS: &[PanelPlacement] = &[PanelPlacement::Left, PanelPlacement::Right];

    #[test]
    fn a_right_panel_grows_leftward_only() {
        let subject = ResizeSubject::Panel(PanelPlacement::Right);
        assert_eq!(
            resolve_resize(subject, ResizeEdge::Left, BOTH_COLUMNS),
            Some(super::ResizeDelta {
                placement: PanelPlacement::Right,
                grow: true
            })
        );
        for edge in [ResizeEdge::Right, ResizeEdge::Top, ResizeEdge::Bottom] {
            assert_eq!(
                resolve_resize(subject, edge, BOTH_COLUMNS),
                None,
                "{edge:?} is a frame edge for a right-docked panel"
            );
        }
    }

    #[test]
    fn a_left_panel_grows_rightward_only() {
        let subject = ResizeSubject::Panel(PanelPlacement::Left);
        assert_eq!(
            resolve_resize(subject, ResizeEdge::Right, BOTH_COLUMNS)
                .map(|delta| (delta.placement, delta.grow)),
            Some((PanelPlacement::Left, true))
        );
        assert_eq!(
            resolve_resize(subject, ResizeEdge::Left, BOTH_COLUMNS),
            None
        );
    }

    #[test]
    fn a_bottom_panel_grows_upward_only() {
        let subject = ResizeSubject::Panel(PanelPlacement::Bottom);
        assert_eq!(
            resolve_resize(subject, ResizeEdge::Top, &[PanelPlacement::Bottom])
                .map(|delta| (delta.placement, delta.grow)),
            Some((PanelPlacement::Bottom, true))
        );
        assert_eq!(
            resolve_resize(subject, ResizeEdge::Bottom, &[PanelPlacement::Bottom]),
            None
        );
    }

    #[test]
    fn growing_the_main_buffer_shrinks_the_panel_across_the_border() {
        let occupied: &[PanelPlacement] = &[
            PanelPlacement::Left,
            PanelPlacement::Right,
            PanelPlacement::Bottom,
        ];
        let main = ResizeSubject::Main;
        assert_eq!(
            resolve_resize(main, ResizeEdge::Right, occupied)
                .map(|delta| (delta.placement, delta.grow)),
            Some((PanelPlacement::Right, false)),
            "`l` pushes the right panel's left border out from under it"
        );
        assert_eq!(
            resolve_resize(main, ResizeEdge::Left, occupied)
                .map(|delta| (delta.placement, delta.grow)),
            Some((PanelPlacement::Left, false))
        );
        assert_eq!(
            resolve_resize(main, ResizeEdge::Bottom, occupied)
                .map(|delta| (delta.placement, delta.grow)),
            Some((PanelPlacement::Bottom, false))
        );
    }

    #[test]
    fn the_main_buffer_has_nothing_above_it() {
        assert_eq!(
            resolve_resize(
                ResizeSubject::Main,
                ResizeEdge::Top,
                &[PanelPlacement::Left, PanelPlacement::Right]
            ),
            None
        );
    }

    #[test]
    fn an_empty_edge_is_not_resizable_from_the_main_buffer() {
        // Chat is docked right, nothing on the left: `h` has no panel to push
        // and must be a no-op rather than stealing width from the frame edge.
        assert_eq!(
            resolve_resize(
                ResizeSubject::Main,
                ResizeEdge::Left,
                &[PanelPlacement::Right]
            ),
            None
        );
        assert_eq!(
            resolve_resize(ResizeSubject::Main, ResizeEdge::Right, &[]),
            None
        );
    }

    #[test]
    fn only_bare_hjkl_are_resize_keys() {
        assert_eq!(
            resize_edge_of(key(KeyCode::Char('h'))),
            Some(ResizeEdge::Left)
        );
        assert_eq!(
            resize_edge_of(key(KeyCode::Char('j'))),
            Some(ResizeEdge::Bottom)
        );
        assert_eq!(
            resize_edge_of(key(KeyCode::Char('k'))),
            Some(ResizeEdge::Top)
        );
        assert_eq!(
            resize_edge_of(key(KeyCode::Char('l'))),
            Some(ResizeEdge::Right)
        );
        // Arrows belong to the docking gesture family and are claimed earlier.
        assert_eq!(resize_edge_of(key(KeyCode::Left)), None);
        assert_eq!(resize_edge_of(key(KeyCode::Right)), None);
        assert_eq!(
            resize_edge_of(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(resize_edge_of(key(KeyCode::Char('x'))), None);
    }

    /// A layout that has already seen a frame, so a bottom-edge drag has a
    /// starting value to move away from.
    fn layout() -> TuiLayout {
        let mut layout = TuiLayout::with_config(LayoutConfig::default());
        layout.observe_frame(ratatui::layout::Rect::new(0, 0, 120, 40));
        layout
    }

    fn grow(placement: PanelPlacement) -> super::ResizeDelta {
        super::ResizeDelta {
            placement,
            grow: true,
        }
    }

    fn shrink(placement: PanelPlacement) -> super::ResizeDelta {
        super::ResizeDelta {
            placement,
            grow: false,
        }
    }

    #[test]
    fn a_panel_press_widens_only_that_edge() {
        let mut layout = layout();
        let before_left = layout.config.column_width(PanelPlacement::Left);
        let before_right = layout.config.column_width(PanelPlacement::Right);

        let delta = resolve_resize(
            ResizeSubject::Panel(PanelPlacement::Right),
            ResizeEdge::Left,
            BOTH_COLUMNS,
        )
        .expect("a right panel's left border is shared, so `h` grows it");
        assert!(apply_resize(&mut layout, delta));

        assert_eq!(
            layout.config.column_width(PanelPlacement::Right),
            before_right + COLUMN_RESIZE_STEP
        );
        assert_eq!(
            layout.config.column_width(PanelPlacement::Left),
            before_left,
            "one edge's width must not become another's"
        );
    }

    #[test]
    fn a_main_press_narrows_the_panel_across_the_border() {
        let mut layout = layout();
        let before = layout.config.column_width(PanelPlacement::Right);
        let delta = resolve_resize(
            ResizeSubject::Main,
            ResizeEdge::Right,
            &[PanelPlacement::Right],
        )
        .expect("the main buffer's right border is the panel's left border");
        assert!(apply_resize(&mut layout, delta));
        assert_eq!(
            layout.config.column_width(PanelPlacement::Right),
            before - COLUMN_RESIZE_STEP
        );
    }

    #[test]
    fn the_same_border_grows_or_shrinks_depending_on_which_side_asked() {
        // The chat column's left border, seen from each side of it. This is the
        // whole outward-only rule: neither surface can shrink itself, they can
        // only be pushed into or out of.
        let mut layout = layout();
        let before = layout.config.column_width(PanelPlacement::Right);

        let from_panel = resolve_resize(
            ResizeSubject::Panel(PanelPlacement::Right),
            ResizeEdge::Left,
            &[PanelPlacement::Right],
        )
        .expect("h from the panel");
        assert_eq!(
            (from_panel.placement, from_panel.grow),
            (PanelPlacement::Right, true)
        );
        assert!(apply_resize(&mut layout, from_panel));
        assert_eq!(
            layout.config.column_width(PanelPlacement::Right),
            before + 1
        );

        let from_main = resolve_resize(
            ResizeSubject::Main,
            ResizeEdge::Right,
            &[PanelPlacement::Right],
        )
        .expect("l from the buffer");
        assert_eq!(
            (from_main.placement, from_main.grow),
            (PanelPlacement::Right, false)
        );
        assert!(apply_resize(&mut layout, from_main));
        assert_eq!(layout.config.column_width(PanelPlacement::Right), before);
    }

    #[test]
    fn columns_are_refused_past_their_limits_rather_than_clamped() {
        let mut layout = layout();
        layout.config.set_right_panel_width(MAX_COLUMN_WIDTH);
        assert!(!apply_resize(&mut layout, grow(PanelPlacement::Right)));
        assert_eq!(
            layout.config.column_width(PanelPlacement::Right),
            MAX_COLUMN_WIDTH,
            "a refused step must leave the value exactly where it was"
        );

        layout.config.set_right_panel_width(MIN_COLUMN_WIDTH);
        assert!(!apply_resize(&mut layout, shrink(PanelPlacement::Right)));
        assert_eq!(
            layout.config.column_width(PanelPlacement::Right),
            MIN_COLUMN_WIDTH
        );
    }

    #[test]
    fn a_column_width_never_saturates_into_a_wraparound() {
        // `u16::MAX - 1 + 1` would otherwise wrap to zero and collapse the
        // column rather than refusing.
        let mut layout = layout();
        layout.config.set_right_panel_width(u16::MAX - 1);
        assert!(!apply_resize(&mut layout, grow(PanelPlacement::Right)));
        assert_eq!(
            layout.config.column_width(PanelPlacement::Right),
            u16::MAX - 1
        );
    }

    #[test]
    fn the_bottom_panel_seeds_a_row_count_from_the_ratio_then_steps_in_rows() {
        let mut layout = layout();
        assert!(
            layout.config.bottom_panel_rows.is_none(),
            "a fresh layout is sized from the ratio, not from a row count"
        );
        let seeded = layout
            .bottom_panel_height(layout.frame_height())
            .expect("a 40-row frame can host a bottom panel");

        assert!(apply_resize(&mut layout, shrink(PanelPlacement::Bottom)));
        assert_eq!(layout.config.bottom_panel_rows, Some(seeded - 1));
        assert!(apply_resize(&mut layout, shrink(PanelPlacement::Bottom)));
        assert_eq!(layout.config.bottom_panel_rows, Some(seeded - 2));

        // And a wider bottom panel now wins over the ratio, which is the point
        // of recording an exact count.
        assert_eq!(
            layout
                .bottom_panel_height(layout.frame_height())
                .expect("still hostable"),
            seeded - 2
        );
    }

    #[test]
    fn a_bottom_panel_resize_is_refused_on_a_frame_too_short_to_host_one() {
        let mut layout = TuiLayout::with_config(LayoutConfig::default());
        layout.observe_frame(ratatui::layout::Rect::new(0, 0, 120, 4));
        assert!(
            !apply_resize(&mut layout, grow(PanelPlacement::Bottom)),
            "there is no bottom panel to grow on a frame this short"
        );
    }
}

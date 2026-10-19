//! The Helix selection **state machine**, with no opinion about coordinates.
//!
//! One rule set, shared by every surface that speaks Helix selection. The
//! rules are deliberately separate from what a *point* is, because the two
//! surfaces that need them do not agree on what a point is: the terminal
//! addresses viewport cells (`(row, col)`) while the editor addresses byte
//! offsets into a `DocumentSnapshot`. Re-deriving "what does `x` do when the
//! cursor is three lines below the anchor" twice is how two surfaces end up
//! disagreeing, and the terminal/panel split already taught that lesson once.
//!
//! The rules, in one place:
//!
//! - `v` arms **character** selection at the cursor; pressing `v` again
//!   deactivates. `v` is a *mode*, so movement extends the selection.
//! - `x` arms **line** selection over the cursor's whole line, and pressing
//!   `x` again extends by exactly one more line. `x` is an *action*, not a
//!   mode: movement must not silently extend a line selection, because then
//!   `x` could never re-anchor on a line the reader scrolled to.
//! - A bare movement extends the head **only** while in character mode. In
//!   line mode it relocates the cursor and disarms further `x` growth.
//! - `x` after `v` and `v` after `x` both re-anchor at the cursor, which is
//!   what makes `x`-then-`v` mean "take this line, then refine it by moving".
//!
//! What a selection *does* — copying, deleting, drawing — is not here. Those
//! are effects, and each surface supplies its own.

/// A point a selection can address.
///
/// `Ord` must be the surface's own reading order (top-to-bottom, then
/// left-to-right), because a selection's start is `min(anchor, head)`.
pub trait SelectionPoint: Copy + Ord {
    /// Identity of the line/row this point belongs to.
    ///
    /// Separated from the point itself because whole-line selection needs to
    /// answer "which lines does this span" without knowing how wide a line is.
    /// A surface with variable line length resolves the bounds when it acts on
    /// the range, not here.
    type Line: Copy + Ord;

    /// The line this point sits on.
    fn line(&self) -> Self::Line;
}

/// Granularity a selection is anchored at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Helix `v`: character granularity, inclusive of both endpoint points.
    ///
    /// This is the only mode a movement extends, which is what makes `v` a
    /// mode and `x` an action.
    Char,
    /// Helix `x`: whole lines, inclusive of both endpoint rows.
    Line,
}

/// A held selection: an anchor, a head, a granularity, and whether `x` growth
/// is still armed.
///
/// The head is stored rather than read from the cursor because line mode's
/// head **must not** follow the cursor — that is the whole difference between
/// `x` and `v`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection<P: SelectionPoint> {
    /// Where the selection started.
    pub anchor: P,
    /// Where it currently ends. Moved by growth and by character-mode motion.
    pub head: P,
    /// Character or line granularity.
    pub mode: SelectionMode,
    /// Whether a further `x` would extend this selection.
    ///
    /// Armed from the moment line selection starts and cleared by any
    /// deliberate cursor movement. Comparing the cursor against the anchor
    /// cannot make the same distinction: after one growth the cursor is
    /// legitimately no longer on the anchor.
    pub growing: bool,
}

impl<P: SelectionPoint> Selection<P> {
    /// Build a selection anchored at `point`.
    pub fn new(point: P, mode: SelectionMode) -> Self {
        Self {
            anchor: point,
            head: point,
            mode,
            growing: false,
        }
    }

    /// Whether a bare cursor movement should extend this selection.
    ///
    /// True for character selection only. See the module docs for why line
    /// selection is excluded rather than treated as "the same rule, coarser".
    pub fn extends_on_move(&self) -> bool {
        matches!(self.mode, SelectionMode::Char)
    }

    /// Lines spanned by the selection, inclusive of both endpoints.
    pub fn line_range(&self) -> (P::Line, P::Line) {
        let first = self.anchor.min(self.head).line();
        let last = self.anchor.max(self.head).line();
        (first, last)
    }

    /// First and last point covered, inclusive.
    pub fn bounds(&self) -> (P, P) {
        let first = self.anchor.min(self.head);
        let last = self.anchor.max(self.head);
        (first, last)
    }

    /// Whether `point` falls inside the selection.
    ///
    /// A line selection covers every point on every line it spans. A character
    /// selection is bounded only at its two ends: the interior rows run from
    /// their first column to their last, which is what makes a selection
    /// dragged down two lines read back as whole middle lines rather than as
    /// a diagonal.
    pub fn contains(&self, point: P) -> bool {
        let (first, last) = self.bounds();
        let (first_line, last_line) = self.line_range();
        let line = point.line();
        match self.mode {
            SelectionMode::Line => (first_line..=last_line).contains(&line),
            SelectionMode::Char => {
                if first_line == last_line {
                    point >= first && point <= last
                } else if line == first_line {
                    point >= first
                } else if line == last_line {
                    point <= last
                } else {
                    (first_line..=last_line).contains(&line)
                }
            }
        }
    }
}

/// A cursor plus whatever selection it is holding, and the verbs that move
/// both.
///
/// The cursor is here rather than on the surface because "moving the cursor
/// extends the selection" is a rule, not a drawing detail: a surface that kept
/// its own cursor would have to re-implement the growth interaction and would
/// eventually get `x` wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionState<P: SelectionPoint> {
    cursor: P,
    selection: Option<Selection<P>>,
}

impl<P: SelectionPoint> SelectionState<P> {
    /// Start at `cursor` with nothing selected.
    pub fn new(cursor: P) -> Self {
        Self {
            cursor,
            selection: None,
        }
    }

    /// Rebuild from a surface that stores the cursor and the selection
    /// separately.
    ///
    /// The terminal keeps its navigation on the controller and never needs
    /// this; the editor keeps a byte-offset cursor because its viewport, LSP
    /// reads, and undo entries all address that offset directly, so it cannot
    /// move wholesale into this type. Driving the rules through a restored
    /// value rather than re-deriving them is the point: `v` must stay a mode
    /// and `x` an action on both surfaces, and a second implementation is how
    /// those two drifted the first time.
    ///
    /// Cost is one move of two copies; call it per verb, not per frame.
    pub fn restore(cursor: P, selection: Option<Selection<P>>) -> Self {
        Self { cursor, selection }
    }

    /// Current cursor position.
    pub fn cursor(&self) -> P {
        self.cursor
    }

    /// Relocate the cursor without touching the selection.
    ///
    /// For bookkeeping — clamping back inside the viewport, or re-pinning a
    /// cursor to an edge after a scroll. A deliberate movement goes through
    /// [`Self::move_to`], which is the one that knows about selection modes.
    pub fn set_cursor(&mut self, cursor: P) {
        self.cursor = cursor;
    }

    /// The held selection, if any.
    pub fn selection(&self) -> Option<Selection<P>> {
        self.selection
    }

    /// Whether a selection is held.
    pub fn is_selected(&self) -> bool {
        self.selection.is_some()
    }

    /// Ordered first/last points of the held selection.
    pub fn span(&self) -> Option<(P, P)> {
        self.selection.map(|selection| selection.bounds())
    }

    /// Lines spanned by the held selection.
    pub fn line_range(&self) -> Option<(P::Line, P::Line)> {
        self.selection.map(|selection| selection.line_range())
    }

    /// Arm character selection at the cursor (`v`).
    pub fn select_char(&mut self) {
        self.selection = Some(Selection::new(self.cursor, SelectionMode::Char));
    }

    /// Arm whole-line selection at the cursor (`x`), growth enabled.
    pub fn select_line(&mut self) {
        let mut selection = Selection::new(self.cursor, SelectionMode::Line);
        selection.growing = true;
        self.selection = Some(selection);
    }

    /// `v`: arm character selection, or deactivate if already in it.
    ///
    /// Toggling rather than re-arming is what lets `v` be a mode the reader
    /// leaves — and it is why a second `v` with a selection held discards it
    /// instead of re-anchoring on the cursor.
    pub fn toggle_char(&mut self) {
        match self.selection {
            Some(selection) if selection.mode == SelectionMode::Char => self.selection = None,
            _ => self.select_char(),
        }
    }

    /// Whether a further `x` would extend the held selection instead of
    /// re-anchoring on the cursor.
    ///
    /// The caller resolves the next line's point itself and passes it to
    /// [`Self::grow_to`], because "the next line" is geometry the surface
    /// owns and this module deliberately does not.
    pub fn line_select_extends(&self) -> bool {
        self.selection
            .is_some_and(|selection| selection.mode == SelectionMode::Line && selection.growing)
    }

    /// Extend the held selection's head to `head`, keeping its anchor.
    ///
    /// Panics if nothing is held; call only when a selection exists.
    pub fn grow_to(&mut self, head: P) {
        if let Some(selection) = self.selection.as_mut() {
            selection.head = head;
        }
    }

    /// Deliberate cursor movement.
    ///
    /// In character mode the head follows, so the selection grows with the
    /// cursor. In line mode the selection is left exactly where it was and
    /// only its growth is disarmed — this is the rule that makes `x` mean
    /// "select this line" rather than "select everything between here and
    /// wherever I next wander".
    pub fn move_to(&mut self, cursor: P) {
        self.repin_cursor(cursor);
        if let Some(selection) = self.selection.as_mut() {
            selection.growing = false;
        }
    }

    /// Put the cursor back where it belongs after a scroll moved every stored
    /// point, honouring the selection's own extension rule.
    ///
    /// A scroll moves content past a fixed frame, so it translates the anchor
    /// and the head with it. The cursor is the exception: a reader walking off
    /// the top edge wants to stay on the top edge and see the line that was
    /// just revealed. The head follows only in character mode — in line mode
    /// re-pinning it would extend a selection the reader never asked to grow.
    pub fn repin_cursor(&mut self, cursor: P) {
        let extends = self
            .selection
            .is_some_and(|selection| selection.extends_on_move());
        self.cursor = cursor;
        if let Some(selection) = self.selection.as_mut() {
            if extends {
                selection.head = cursor;
            }
        }
    }

    /// Drop the held selection, leaving the cursor alone.
    pub fn deactivate(&mut self) {
        self.selection = None;
    }

    /// Re-map every stored point — cursor, anchor and head.
    ///
    /// A scrolling viewport moves its content past a fixed frame, so stored
    /// points have to ride along or a selection would silently point at
    /// different text. The surface supplies the mapping because only it knows
    /// what "moved by one row" means for its coordinates.
    pub fn translate(&mut self, map: impl Fn(P) -> P) {
        self.cursor = map(self.cursor);
        if let Some(selection) = self.selection.as_mut() {
            selection.anchor = map(selection.anchor);
            selection.head = map(selection.head);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SelectionMode, SelectionPoint, SelectionState};

    /// A terminal-style viewport cell.
    ///
    /// A newtype rather than `(u16, u16)` on purpose: the production terminal
    /// implements [`SelectionPoint`] for the bare tuple, and a test that reused it
    /// would be a duplicate impl rather than an independent check of the rules.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    struct Cell(u16, u16);

    impl SelectionPoint for Cell {
        type Line = u16;

        fn line(&self) -> u16 {
            self.0
        }
    }

    /// The composer's own point type.
    ///
    /// The rules below are the *shared* rules, so they have to hold for the
    /// composer's point as well as the terminal's. A second implementation of
    /// "what a selection covers" in the composer would be exactly the drift
    /// this module exists to prevent, so the composer is checked against the
    /// same rules rather than against a copy of them.
    use crate::tui::composer_text::ComposerPoint;

    /// The width of one line in the composer's test buffer.
    ///
    /// The rules ask about columns like 999 and 200, so the buffer has to be
    /// wide enough for the questions to mean something.
    const LINE_WIDTH: usize = 1000;

    /// The stride between two lines' starts: the line's bytes and its newline.
    const LINE_STRIDE: usize = LINE_WIDTH + 1;

    fn cell(row: u16, col: u16) -> Cell {
        Cell(row, col)
    }

    /// A viewport cell moved down `rows` rows.
    fn cell_shift(point: &Cell, rows: i32) -> Cell {
        Cell((i32::from(point.0) + rows).max(0) as u16, point.1)
    }

    /// A point in the composer's test buffer.
    fn composer_point(line: u16, column: u16) -> ComposerPoint {
        ComposerPoint::new(
            usize::from(line) * LINE_STRIDE + usize::from(column),
            usize::from(line),
        )
    }

    /// A composer point moved down `rows` rows, keeping its column.
    ///
    /// A scroll of a text buffer moves the content, so the byte offset moves
    /// with the line — which is why the composer's shift is not the same
    /// arithmetic as the viewport's.
    fn composer_shift(point: &ComposerPoint, rows: i32) -> ComposerPoint {
        let line = (point.line as i32 + rows).max(0) as usize;
        let column = point.offset - point.line * LINE_STRIDE;
        ComposerPoint::new(line * LINE_STRIDE + column, line)
    }

    /// The rules, written once and checked against every surface that uses
    /// them.
    ///
    /// Each rule takes the surface's two functions: how to build a point at a
    /// line and column, and how to move a point down some rows. Everything
    /// else — the points, the state, the modes — is the shared machine's.
    mod rules {
        use super::{SelectionMode, SelectionPoint, SelectionState};

        /// The bounds a rule needs from a surface.
        ///
        /// The surface's `Line` **is** the point's line: the rules compare a
        /// selection's line range against literals, so the two have to be the
        /// same type rather than merely similar ones.
        pub(super) trait Surface {
            type Point: SelectionPoint<Line = Self::Line> + std::fmt::Debug + PartialEq;
            type Line: Copy + Ord + std::fmt::Debug + PartialEq + From<u16>;

            /// A point at `line` and `column`.
            fn point(line: u16, column: u16) -> Self::Point;

            /// The point moved down `rows` rows.
            fn shift(point: &Self::Point, rows: i32) -> Self::Point;
        }

        pub(super) fn line_selection_covers_every_column_of_every_line_it_spans<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(3, 7));
            state.select_line();
            let selection = state.selection().expect("selected");
            assert_eq!(selection.line_range(), (S::Line::from(3), S::Line::from(3)));
            assert!(selection.contains(S::point(3, 0)));
            assert!(selection.contains(S::point(3, 7)));
            assert!(selection.contains(S::point(3, 200)));
            assert!(!selection.contains(S::point(2, 7)));
            assert!(!selection.contains(S::point(4, 7)));
        }

        pub(super) fn line_selection_ignores_the_cursor_column<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(3, 0));
            state.select_line();
            let selection = state.selection().expect("selected");
            // The anchor column is irrelevant in line mode; a reader landing on
            // column 0 still gets the whole line.
            assert!(selection.contains(S::point(3, 41)));
        }

        pub(super) fn repeated_line_select_grows_only_while_nothing_has_moved<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(3, 7));
            state.select_line();
            assert!(state.line_select_extends());

            state.grow_to(S::point(4, 7));
            assert_eq!(
                state.line_range(),
                Some((S::Line::from(3), S::Line::from(4)))
            );
            assert!(state.line_select_extends());

            // Any deliberate movement disarms growth, so the next `x` re-anchors
            // instead of silently swallowing everything walked past.
            state.move_to(S::point(4, 0));
            assert!(!state.line_select_extends());
            assert_eq!(
                state.line_range(),
                Some((S::Line::from(3), S::Line::from(4)))
            );
        }

        pub(super) fn movement_does_not_extend_a_line_selection<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(1, 0));
            state.select_line();
            state.move_to(S::point(9, 0));
            assert_eq!(
                state.line_range(),
                Some((S::Line::from(1), S::Line::from(1))),
                "a bare movement must not add lines to a line selection"
            );
        }

        pub(super) fn movement_extends_a_char_selection<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(1, 0));
            state.select_char();
            state.move_to(S::point(1, 4));
            assert_eq!(state.span(), Some((S::point(1, 0), S::point(1, 4))));

            state.move_to(S::point(3, 2));
            assert_eq!(state.span(), Some((S::point(1, 0), S::point(3, 2))));
        }

        pub(super) fn char_selection_is_inclusive_at_both_ends<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(2, 3));
            state.select_char();
            state.move_to(S::point(2, 6));
            let selection = state.selection().expect("selected");
            assert!(selection.contains(S::point(2, 3)));
            assert!(selection.contains(S::point(2, 6)));
            assert!(!selection.contains(S::point(2, 2)));
            assert!(!selection.contains(S::point(2, 7)));
        }

        pub(super) fn a_multi_line_char_selection_takes_whole_interior_lines<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(1, 4));
            state.select_char();
            state.move_to(S::point(3, 1));
            let selection = state.selection().expect("selected");
            // Start row keeps its tail, end row keeps its head, the row between is
            // whole. A diagonal reading here would make a copied command unusable.
            assert!(selection.contains(S::point(1, 4)));
            assert!(selection.contains(S::point(1, 999)));
            assert!(selection.contains(S::point(2, 0)));
            assert!(selection.contains(S::point(2, 999)));
            assert!(selection.contains(S::point(3, 0)));
            assert!(selection.contains(S::point(3, 1)));
            assert!(!selection.contains(S::point(3, 2)));
            assert!(!selection.contains(S::point(1, 3)));
        }

        pub(super) fn a_backwards_char_selection_reads_the_same_as_a_forwards_one<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(4, 9));
            state.select_char();
            state.move_to(S::point(2, 1));
            let selection = state.selection().expect("selected");
            assert_eq!(selection.bounds(), (S::point(2, 1), S::point(4, 9)));
            assert!(selection.contains(S::point(2, 1)));
            assert!(selection.contains(S::point(4, 9)));
        }

        pub(super) fn v_toggles_character_mode_off<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(2, 2));
            state.toggle_char();
            assert_eq!(state.selection().map(|s| s.mode), Some(SelectionMode::Char));
            state.toggle_char();
            assert!(!state.is_selected());
            // And back on, re-anchored at wherever the cursor is now.
            state.toggle_char();
            assert!(state
                .selection()
                .is_some_and(|s| s.anchor == S::point(2, 2)));
        }

        pub(super) fn v_after_x_re_anchors_at_the_cursor_in_character_mode<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(1, 0));
            state.select_line();
            state.grow_to(S::point(2, 0));
            assert_eq!(
                state.line_range(),
                Some((S::Line::from(1), S::Line::from(2)))
            );

            state.move_to(S::point(1, 5));
            state.toggle_char();
            let selection = state.selection().expect("selected");
            assert_eq!(selection.mode, SelectionMode::Char);
            assert_eq!(selection.anchor, S::point(1, 5));
            assert_eq!(selection.head, S::point(1, 5));
            // …and movement now extends, which is the whole point of `x` then `v`.
            state.move_to(S::point(2, 2));
            assert_eq!(state.span(), Some((S::point(1, 5), S::point(2, 2))));
        }

        pub(super) fn x_after_v_re_anchors_at_the_cursor_as_a_whole_line<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(1, 2));
            state.select_char();
            state.move_to(S::point(2, 7));
            state.select_line();
            let selection = state.selection().expect("selected");
            assert_eq!(selection.mode, SelectionMode::Line);
            assert_eq!(selection.anchor, S::point(2, 7));
            assert!(selection.contains(S::point(2, 0)));
            assert!(selection.contains(S::point(2, 99)));
            assert!(!selection.contains(S::point(1, 2)));
        }

        pub(super) fn deactivate_leaves_the_cursor_where_it_was<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(5, 5));
            state.select_char();
            state.move_to(S::point(6, 6));
            state.deactivate();
            assert!(!state.is_selected());
            assert_eq!(state.cursor(), S::point(6, 6));
        }

        pub(super) fn translate_moves_the_cursor_anchor_and_head_together<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(3, 4));
            state.select_char();
            state.move_to(S::point(5, 6));
            // Scrolling toward older output moves content down the viewport.
            state.translate(|point| S::shift(&point, 2));
            assert_eq!(state.cursor(), S::point(7, 6));
            let selection = state.selection().expect("selected");
            assert_eq!(selection.anchor, S::point(5, 4));
            assert_eq!(selection.head, S::point(7, 6));
        }

        pub(super) fn translate_with_nothing_selected_still_moves_the_cursor<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(1, 1));
            state.translate(|point| S::shift(&point, 3));
            assert_eq!(state.cursor(), S::point(4, 1));
            assert!(!state.is_selected());
        }

        pub(super) fn set_cursor_never_touches_the_selection<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(2, 2));
            state.select_char();
            state.move_to(S::point(4, 4));
            state.set_cursor(S::point(0, 0));
            assert_eq!(state.cursor(), S::point(0, 0));
            assert_eq!(state.span(), Some((S::point(2, 2), S::point(4, 4))));
        }

        pub(super) fn a_line_selection_grows_downward_from_a_mid_screen_cursor<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(6, 3));
            state.select_line();
            assert!(state.line_select_extends());
            state.grow_to(S::point(7, 3));
            state.grow_to(S::point(8, 3));
            assert_eq!(
                state.line_range(),
                Some((S::Line::from(6), S::Line::from(8)))
            );
            assert!(state
                .selection()
                .expect("selected")
                .contains(S::point(8, 0)));
        }

        pub(super) fn repinning_after_a_scroll_extends_a_char_selection_to_the_new_row<
            S: Surface,
        >(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(2, 4));
            state.select_char();
            state.move_to(S::point(2, 9));
            // Scrolling toward newer output moves the content up by one, taking the
            // anchor and head with it.
            state.translate(|point| S::shift(&point, -1));
            assert_eq!(state.cursor(), S::point(1, 9));
            // Re-pinning to the edge row the reader walked off puts the head there
            // too, so the line just revealed is inside the selection — which now
            // runs *upward*, with the head above the anchor.
            state.repin_cursor(S::point(0, 9));
            let selection = state.selection().expect("selected");
            assert_eq!(selection.head, S::point(0, 9));
            assert_eq!(selection.anchor, S::point(1, 4));
            assert_eq!(selection.bounds(), (S::point(0, 9), S::point(1, 4)));
            assert_eq!(selection.line_range(), (S::Line::from(0), S::Line::from(1)));
            assert!(selection.contains(S::point(0, 9)));
            assert!(selection.contains(S::point(1, 4)));
        }

        pub(super) fn repinning_after_a_scroll_does_not_extend_a_line_selection<S: Surface>(
            _surface: &S,
        ) {
            let mut state = SelectionState::new(S::point(1, 0));
            state.select_line();
            // `select_line` anchors at the *cursor*; growth only moves the head. So
            // the anchor is on row 1 and the head has been pushed to row 5.
            state.grow_to(S::point(5, 0));
            state.translate(|point| S::shift(&point, -1));
            assert_eq!(
                state.line_range(),
                Some((S::Line::from(0), S::Line::from(4)))
            );
            state.repin_cursor(S::point(0, 0));
            assert_eq!(
                state.line_range(),
                Some((S::Line::from(0), S::Line::from(4))),
                "a line selection's head must not follow a scroll re-pin, or the \
                 reader would select rows they never walked past"
            );
            assert_eq!(state.cursor(), S::point(0, 0));
        }

        pub(super) fn a_movement_disarms_growth_in_either_mode<S: Surface>(_surface: &S) {
            let mut state = SelectionState::new(S::point(3, 0));
            state.select_line();
            assert!(state.line_select_extends());
            state.move_to(S::point(3, 2));
            assert!(!state.line_select_extends());

            state.select_char();
            state.move_to(S::point(3, 4));
            let selection = state.selection().expect("selected");
            assert!(!selection.growing, "growing is a line-mode flag only");
        }
    }

    /// The viewport cell, as a surface.
    struct CellSurface;

    impl rules::Surface for CellSurface {
        type Point = Cell;
        type Line = u16;

        fn point(line: u16, column: u16) -> Cell {
            cell(line, column)
        }

        fn shift(point: &Cell, rows: i32) -> Cell {
            cell_shift(point, rows)
        }
    }

    /// The composer's point, as a surface.
    struct ComposerSurface;

    impl rules::Surface for ComposerSurface {
        type Point = ComposerPoint;
        type Line = usize;

        fn point(line: u16, column: u16) -> ComposerPoint {
            composer_point(line, column)
        }

        fn shift(point: &ComposerPoint, rows: i32) -> ComposerPoint {
            composer_shift(point, rows)
        }
    }

    /// Check every rule against every surface.
    ///
    /// The rules are the shared machine's contract. A surface that claims to
    /// use the machine has to satisfy them, and the cheapest way to prove that
    /// is to run the same rules against it — not to write a second set of
    /// tests that says the same thing in different words.
    macro_rules! shared_rule_tests {
        ($module:ident, $surface:expr, $($name:ident),* $(,)?) => {
            mod $module {
                use super::rules;

                $(
                    #[test]
                    fn $name() {
                        rules::$name(&$surface);
                    }
                )*
            }
        };
    }

    shared_rule_tests!(
        cell_surface,
        super::CellSurface,
        line_selection_covers_every_column_of_every_line_it_spans,
        line_selection_ignores_the_cursor_column,
        repeated_line_select_grows_only_while_nothing_has_moved,
        movement_does_not_extend_a_line_selection,
        movement_extends_a_char_selection,
        char_selection_is_inclusive_at_both_ends,
        a_multi_line_char_selection_takes_whole_interior_lines,
        a_backwards_char_selection_reads_the_same_as_a_forwards_one,
        v_toggles_character_mode_off,
        v_after_x_re_anchors_at_the_cursor_in_character_mode,
        x_after_v_re_anchors_at_the_cursor_as_a_whole_line,
        deactivate_leaves_the_cursor_where_it_was,
        translate_moves_the_cursor_anchor_and_head_together,
        translate_with_nothing_selected_still_moves_the_cursor,
        set_cursor_never_touches_the_selection,
        a_line_selection_grows_downward_from_a_mid_screen_cursor,
        repinning_after_a_scroll_extends_a_char_selection_to_the_new_row,
        repinning_after_a_scroll_does_not_extend_a_line_selection,
        a_movement_disarms_growth_in_either_mode,
    );

    shared_rule_tests!(
        composer_surface,
        super::ComposerSurface,
        line_selection_covers_every_column_of_every_line_it_spans,
        line_selection_ignores_the_cursor_column,
        repeated_line_select_grows_only_while_nothing_has_moved,
        movement_does_not_extend_a_line_selection,
        movement_extends_a_char_selection,
        char_selection_is_inclusive_at_both_ends,
        a_multi_line_char_selection_takes_whole_interior_lines,
        a_backwards_char_selection_reads_the_same_as_a_forwards_one,
        v_toggles_character_mode_off,
        v_after_x_re_anchors_at_the_cursor_in_character_mode,
        x_after_v_re_anchors_at_the_cursor_as_a_whole_line,
        deactivate_leaves_the_cursor_where_it_was,
        translate_moves_the_cursor_anchor_and_head_together,
        translate_with_nothing_selected_still_moves_the_cursor,
        set_cursor_never_touches_the_selection,
        a_line_selection_grows_downward_from_a_mid_screen_cursor,
        repinning_after_a_scroll_extends_a_char_selection_to_the_new_row,
        repinning_after_a_scroll_does_not_extend_a_line_selection,
        a_movement_disarms_growth_in_either_mode,
    );
}

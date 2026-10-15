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

    fn state_at(row: u16, col: u16) -> SelectionState<Cell> {
        SelectionState::new(Cell(row, col))
    }

    fn cell(row: u16, col: u16) -> Cell {
        Cell(row, col)
    }

    #[test]
    fn line_selection_covers_every_column_of_every_line_it_spans() {
        let mut state = state_at(3, 7);
        state.select_line();
        let selection = state.selection().expect("selected");
        assert_eq!(selection.line_range(), (3, 3));
        assert!(selection.contains(cell(3, 0)));
        assert!(selection.contains(cell(3, 7)));
        assert!(selection.contains(cell(3, 200)));
        assert!(!selection.contains(cell(2, 7)));
        assert!(!selection.contains(cell(4, 7)));
    }

    #[test]
    fn line_selection_ignores_the_cursor_column() {
        let mut state = state_at(3, 0);
        state.select_line();
        let selection = state.selection().expect("selected");
        // The anchor column is irrelevant in line mode; a reader landing on
        // column 0 still gets the whole line.
        assert!(selection.contains(cell(3, 41)));
    }

    #[test]
    fn repeated_line_select_grows_only_while_nothing_has_moved() {
        let mut state = state_at(3, 7);
        state.select_line();
        assert!(state.line_select_extends());

        state.grow_to(cell(4, 7));
        assert_eq!(state.line_range(), Some((3, 4)));
        assert!(state.line_select_extends());

        // Any deliberate movement disarms growth, so the next `x` re-anchors
        // instead of silently swallowing everything walked past.
        state.move_to(cell(4, 0));
        assert!(!state.line_select_extends());
        assert_eq!(state.line_range(), Some((3, 4)));
    }

    #[test]
    fn movement_does_not_extend_a_line_selection() {
        let mut state = state_at(1, 0);
        state.select_line();
        state.move_to(cell(9, 0));
        assert_eq!(
            state.line_range(),
            Some((1, 1)),
            "a bare movement must not add lines to a line selection"
        );
    }

    #[test]
    fn movement_extends_a_char_selection() {
        let mut state = state_at(1, 0);
        state.select_char();
        state.move_to(cell(1, 4));
        assert_eq!(state.span(), Some((cell(1, 0), cell(1, 4))));

        state.move_to(cell(3, 2));
        assert_eq!(state.span(), Some((cell(1, 0), cell(3, 2))));
    }

    #[test]
    fn char_selection_is_inclusive_at_both_ends() {
        let mut state = state_at(2, 3);
        state.select_char();
        state.move_to(cell(2, 6));
        let selection = state.selection().expect("selected");
        assert!(selection.contains(cell(2, 3)));
        assert!(selection.contains(cell(2, 6)));
        assert!(!selection.contains(cell(2, 2)));
        assert!(!selection.contains(cell(2, 7)));
    }

    #[test]
    fn a_multi_line_char_selection_takes_whole_interior_lines() {
        let mut state = state_at(1, 4);
        state.select_char();
        state.move_to(cell(3, 1));
        let selection = state.selection().expect("selected");
        // Start row keeps its tail, end row keeps its head, the row between is
        // whole. A diagonal reading here would make a copied command unusable.
        assert!(selection.contains(cell(1, 4)));
        assert!(selection.contains(cell(1, 999)));
        assert!(selection.contains(cell(2, 0)));
        assert!(selection.contains(cell(2, 999)));
        assert!(selection.contains(cell(3, 0)));
        assert!(selection.contains(cell(3, 1)));
        assert!(!selection.contains(cell(3, 2)));
        assert!(!selection.contains(cell(1, 3)));
    }

    #[test]
    fn a_backwards_char_selection_reads_the_same_as_a_forwards_one() {
        let mut state = state_at(4, 9);
        state.select_char();
        state.move_to(cell(2, 1));
        let selection = state.selection().expect("selected");
        assert_eq!(selection.bounds(), (cell(2, 1), cell(4, 9)));
        assert!(selection.contains(cell(2, 1)));
        assert!(selection.contains(cell(4, 9)));
    }

    #[test]
    fn v_toggles_character_mode_off() {
        let mut state = state_at(2, 2);
        state.toggle_char();
        assert_eq!(state.selection().map(|s| s.mode), Some(SelectionMode::Char));
        state.toggle_char();
        assert!(!state.is_selected());
        // And back on, re-anchored at wherever the cursor is now.
        state.toggle_char();
        assert!(state.selection().is_some_and(|s| s.anchor == cell(2, 2)));
    }

    #[test]
    fn v_after_x_re_anchors_at_the_cursor_in_character_mode() {
        let mut state = state_at(1, 0);
        state.select_line();
        state.grow_to(cell(2, 0));
        assert_eq!(state.line_range(), Some((1, 2)));

        state.move_to(cell(1, 5));
        state.toggle_char();
        let selection = state.selection().expect("selected");
        assert_eq!(selection.mode, SelectionMode::Char);
        assert_eq!(selection.anchor, cell(1, 5));
        assert_eq!(selection.head, cell(1, 5));
        // …and movement now extends, which is the whole point of `x` then `v`.
        state.move_to(cell(2, 2));
        assert_eq!(state.span(), Some((cell(1, 5), cell(2, 2))));
    }

    #[test]
    fn x_after_v_re_anchors_at_the_cursor_as_a_whole_line() {
        let mut state = state_at(1, 2);
        state.select_char();
        state.move_to(cell(2, 7));
        state.select_line();
        let selection = state.selection().expect("selected");
        assert_eq!(selection.mode, SelectionMode::Line);
        assert_eq!(selection.anchor, cell(2, 7));
        assert!(selection.contains(cell(2, 0)));
        assert!(selection.contains(cell(2, 99)));
        assert!(!selection.contains(cell(1, 2)));
    }

    #[test]
    fn deactivate_leaves_the_cursor_where_it_was() {
        let mut state = state_at(5, 5);
        state.select_char();
        state.move_to(cell(6, 6));
        state.deactivate();
        assert!(!state.is_selected());
        assert_eq!(state.cursor(), cell(6, 6));
    }

    #[test]
    fn translate_moves_the_cursor_anchor_and_head_together() {
        let mut state = state_at(3, 4);
        state.select_char();
        state.move_to(cell(5, 6));
        // Scrolling toward older output moves content down the viewport.
        state.translate(|Cell(row, col)| Cell(row + 2, col));
        assert_eq!(state.cursor(), cell(7, 6));
        let selection = state.selection().expect("selected");
        assert_eq!(selection.anchor, cell(5, 4));
        assert_eq!(selection.head, cell(7, 6));
    }

    #[test]
    fn translate_with_nothing_selected_still_moves_the_cursor() {
        let mut state = state_at(1, 1);
        state.translate(|Cell(row, col)| Cell(row + 3, col));
        assert_eq!(state.cursor(), cell(4, 1));
        assert!(!state.is_selected());
    }

    #[test]
    fn set_cursor_never_touches_the_selection() {
        let mut state = state_at(2, 2);
        state.select_char();
        state.move_to(cell(4, 4));
        state.set_cursor(cell(0, 0));
        assert_eq!(state.cursor(), cell(0, 0));
        assert_eq!(state.span(), Some((cell(2, 2), cell(4, 4))));
    }

    #[test]
    fn a_line_selection_grows_downward_from_a_mid_screen_cursor() {
        let mut state = state_at(6, 3);
        state.select_line();
        assert!(state.line_select_extends());
        state.grow_to(cell(7, 3));
        state.grow_to(cell(8, 3));
        assert_eq!(state.line_range(), Some((6, 8)));
        assert!(state.selection().expect("selected").contains(cell(8, 0)));
    }

    #[test]
    fn repinning_after_a_scroll_extends_a_char_selection_to_the_new_row() {
        let mut state = state_at(2, 4);
        state.select_char();
        state.move_to(cell(2, 9));
        // Scrolling toward newer output moves the content up by one, taking the
        // anchor and head with it.
        state.translate(|Cell(row, col)| Cell(row.saturating_sub(1), col));
        assert_eq!(state.cursor(), cell(1, 9));
        // Re-pinning to the edge row the reader walked off puts the head there
        // too, so the line just revealed is inside the selection — which now
        // runs *upward*, with the head above the anchor.
        state.repin_cursor(cell(0, 9));
        let selection = state.selection().expect("selected");
        assert_eq!(selection.head, cell(0, 9));
        assert_eq!(selection.anchor, cell(1, 4));
        assert_eq!(selection.bounds(), (cell(0, 9), cell(1, 4)));
        assert_eq!(selection.line_range(), (0, 1));
        assert!(selection.contains(cell(0, 9)));
        assert!(selection.contains(cell(1, 4)));
    }

    #[test]
    fn repinning_after_a_scroll_does_not_extend_a_line_selection() {
        let mut state = state_at(1, 0);
        state.select_line();
        // `select_line` anchors at the *cursor*; growth only moves the head. So
        // the anchor is on row 1 and the head has been pushed to row 5.
        state.grow_to(cell(5, 0));
        state.translate(|Cell(row, col)| Cell(row.saturating_sub(1), col));
        assert_eq!(state.line_range(), Some((0, 4)));
        state.repin_cursor(cell(0, 0));
        assert_eq!(
            state.line_range(),
            Some((0, 4)),
            "a line selection's head must not follow a scroll re-pin, or the \
             reader would select rows they never walked past"
        );
        assert_eq!(state.cursor(), cell(0, 0));
    }

    #[test]
    fn a_movement_disarms_growth_in_either_mode() {
        let mut state = state_at(3, 0);
        state.select_line();
        assert!(state.line_select_extends());
        state.move_to(cell(3, 2));
        assert!(!state.line_select_extends());

        state.select_char();
        state.move_to(cell(3, 4));
        let selection = state.selection().expect("selected");
        assert!(!selection.growing, "growing is a line-mode flag only");
    }
}

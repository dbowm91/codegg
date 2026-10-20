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
//! Every rule above is stated over a **set**. Helix resolves a verb against
//! every selection it is holding, so `v`, `x`, and motion all apply to all of
//! them, and the members are kept in document order around one distinguished
//! **primary**. The accessors that predate the set stayed primary-scoped on
//! purpose — see [`SelectionState`], which explains why that keeps the two
//! surfaces on one implementation instead of two.
//!
//! What a selection *does* — copying, deleting, drawing — is not here. Those
//! are effects, and each surface supplies its own.

use std::mem;

/// Order `selections` by [`Selection::bounds`] and clamp `primary` into range.
///
/// Shared by [`SelectionState`]'s own mutations and by a surface restoring a
/// set from storage or from an undo entry, so "members are in document order
/// and `primary` names one of them" is established in exactly one place. A
/// surface that sorted its own way would be free to disagree with the rules.
pub fn normalize_set<P: SelectionPoint>(
    mut selections: Vec<Selection<P>>,
    primary: usize,
) -> (Vec<Selection<P>>, usize) {
    let tracked = selections.get(primary).copied();
    if selections.len() > 1 {
        // `sort_by_key` is not stable and may call the key more than once per
        // element; `bounds()` is a field read, so that is free here.
        selections.sort_by_key(|selection| selection.bounds().0);
    }
    // Clamping happens *after* the search so a member that was the primary and
    // still exists is found wherever it landed, rather than being clamped to
    // whatever index it happens to occupy now.
    let primary = tracked
        .and_then(|was_primary| {
            selections
                .iter()
                .position(|selection| *selection == was_primary)
        })
        .unwrap_or(primary);
    let primary = if selections.is_empty() {
        0
    } else {
        primary.min(selections.len() - 1)
    };
    (selections, primary)
}

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

/// A cursor plus whatever **set** of selections it is holding, and the verbs
/// that move both.
///
/// The cursor is here rather than on the surface because "moving the cursor
/// extends the selection" is a rule, not a drawing detail: a surface that kept
/// its own cursor would have to re-implement the growth interaction and would
/// eventually get `x` wrong.
///
/// # The set, and why the single-selection API survived it
///
/// Helix selection is a *set*: several cursors, each holding its own selection,
/// and a verb that acts on a selection acts on all of them at once. That is
/// why this holds a `Vec` rather than an `Option` — and roughly fifteen verbs
/// (`C`, `;`, `S`, `%`, the regex surface) are not merely unbound without a
/// set, they are *undefined*, because there is nothing for them to mean
/// something over.
///
/// The one-member API is deliberately kept rather than replaced. Every
/// accessor here that predates the set is **primary-scoped**: it answers for
/// the distinguished member and says so. That is what lets two surfaces with
/// genuinely different points — the terminal addresses viewport cells, the
/// editor addresses byte offsets — keep running the *same* rule set instead of
/// forking one per surface. A surface that needs the whole set asks for
/// [`Self::selections`].
///
/// `Copy` is gone with the `Option`, and nothing depended on it: a `Vec` is
/// heap-allocated, and every caller reached for a field or a `&mut` anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionState<P: SelectionPoint> {
    cursor: P,
    /// Every held selection, sorted by [`Selection::bounds`].
    ///
    /// Sorted rather than kept in creation order so that iteration order and
    /// `bounds()` agree — a member whose anchor moved past a neighbour's would
    /// otherwise make a "members are in document order" claim false.
    selections: Vec<Selection<P>>,
    /// Index into [`Self::selections`] of the distinguished member.
    ///
    /// Meaningless while the set is empty. Tracked *through* a sort rather than
    /// pinned to a position, because a sort moves members: pinning would make
    /// "the primary" mean "whatever ended up at index 0".
    primary: usize,
}

impl<P: SelectionPoint> SelectionState<P> {
    /// Start at `cursor` with nothing selected.
    pub fn new(cursor: P) -> Self {
        Self {
            cursor,
            selections: Vec::new(),
            primary: 0,
        }
    }

    /// Restore a zero- or one-member set.
    ///
    /// The shape a surface stores "a cursor and maybe one selection" already
    /// has. Kept as its own constructor rather than folded into
    /// [`Self::restore_set`] so the common single-selection case does not have
    /// to allocate a `Vec` at the call site.
    pub fn restore(cursor: P, selection: Option<Selection<P>>) -> Self {
        let selections = selection.into_iter().collect();
        Self {
            cursor,
            selections,
            primary: 0,
        }
    }

    /// Restore a whole set, with `primary` naming the distinguished member.
    ///
    /// `primary` is clamped into range rather than trusted: a caller restoring
    /// a set it recorded against older text may name a member that no longer
    /// exists, and an out-of-range index would turn every primary-scoped
    /// accessor into a silent `None`.
    pub fn restore_set(cursor: P, selections: Vec<Selection<P>>, primary: usize) -> Self {
        let mut state = Self {
            cursor,
            selections,
            primary,
        };
        state.normalize();
        state
    }

    /// Every held selection, in document order.
    pub fn selections(&self) -> &[Selection<P>] {
        &self.selections
    }

    /// Index of the distinguished member, or `None` when nothing is held.
    pub fn primary_index(&self) -> Option<usize> {
        (!self.selections.is_empty()).then_some(self.primary)
    }

    /// The distinguished member.
    pub fn primary(&self) -> Option<Selection<P>> {
        self.selections.get(self.primary).copied()
    }

    /// Consume into the stored parts, for a surface that keeps them apart.
    pub fn into_parts(self) -> (P, Vec<Selection<P>>, usize) {
        (self.cursor, self.selections, self.primary)
    }

    /// Drop every member, leaving the cursor alone.
    pub fn clear(&mut self) {
        self.selections.clear();
        self.primary = 0;
    }

    /// Re-sort the set and carry the primary member along with it.
    ///
    /// Delegating to [`normalize_set`] is what keeps a surface that restores a
    /// set from storage on the same ordering the rules enforce here.
    fn normalize(&mut self) {
        let (selections, primary) = normalize_set(mem::take(&mut self.selections), self.primary);
        self.selections = selections;
        self.primary = primary;
    }

    /// The anchor a verb should use for member `index`.
    ///
    /// The primary re-anchors on the **cursor**, which is what `v` after `x`
    /// has always meant: the reader moved somewhere and then asked for
    /// character selection, so it starts where they are rather than back at
    /// the head the line selection left behind. Every other member re-anchors
    /// on its own head, because the cursor is not its position — re-anchoring
    /// N members on one cursor would collapse the set to a single point.
    fn anchor_for(&self, index: usize) -> P {
        if index == self.primary {
            self.cursor
        } else {
            self.selections[index].head
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

    /// The distinguished member, if any.
    ///
    /// **Primary-scoped.** This is the accessor every pre-set call site means;
    /// it is not "the only member", and a caller that wants every member must
    /// say so with [`Self::selections`].
    pub fn selection(&self) -> Option<Selection<P>> {
        self.primary()
    }

    /// Whether any selection is held.
    pub fn is_selected(&self) -> bool {
        !self.selections.is_empty()
    }

    /// Ordered first/last points of the distinguished member.
    pub fn span(&self) -> Option<(P, P)> {
        self.primary().map(|selection| selection.bounds())
    }

    /// Lines spanned by the distinguished member.
    pub fn line_range(&self) -> Option<(P::Line, P::Line)> {
        self.primary().map(|selection| selection.line_range())
    }

    /// Arm character selection (`v`).
    ///
    /// On an empty set this is the whole verb: one member at the cursor.
    /// Otherwise every member re-anchors — the primary on the cursor, the rest
    /// on their own heads.
    pub fn select_char(&mut self) {
        if self.selections.is_empty() {
            self.selections
                .push(Selection::new(self.cursor, SelectionMode::Char));
            self.primary = 0;
            return;
        }
        for index in 0..self.selections.len() {
            let anchor = self.anchor_for(index);
            let selection = &mut self.selections[index];
            selection.anchor = anchor;
            selection.head = anchor;
            selection.mode = SelectionMode::Char;
            selection.growing = false;
        }
        self.normalize();
    }

    /// Arm whole-line selection at the cursor (`x`), growth enabled.
    pub fn select_line(&mut self) {
        if self.selections.is_empty() {
            let mut selection = Selection::new(self.cursor, SelectionMode::Line);
            selection.growing = true;
            self.selections.push(selection);
            self.primary = 0;
            return;
        }
        for index in 0..self.selections.len() {
            let anchor = self.anchor_for(index);
            let selection = &mut self.selections[index];
            selection.anchor = anchor;
            selection.head = anchor;
            selection.mode = SelectionMode::Line;
            selection.growing = true;
        }
        self.normalize();
    }

    /// `v`: arm character selection, or deactivate if already in it.
    ///
    /// Toggling rather than re-arming is what lets `v` be a mode the reader
    /// leaves — and it is why a second `v` with a selection held discards it
    /// instead of re-anchoring on the cursor.
    ///
    /// On a **mixed** set the toggle is set-wide in both directions: it arms
    /// every member, and it clears every member only once *all* of them are
    /// already character selections. Half-clearing is the failure this rules
    /// out — `v v` on a set holding both a line selection and a character
    /// selection would otherwise drop the line member and leave the character
    /// one standing, which reads as a lost cursor rather than a toggle.
    pub fn toggle_char(&mut self) {
        if self.selections.is_empty() {
            self.select_char();
            return;
        }
        if self
            .selections
            .iter()
            .all(|selection| selection.mode == SelectionMode::Char)
        {
            self.clear();
            return;
        }
        self.select_char();
    }

    /// Whether a further `x` would extend the held selection instead of
    /// re-anchoring on the cursor.
    ///
    /// True only when **every** member is a growing line selection. A mixed
    /// set is not uniformly growable, and treating it as growable would let
    /// `x` swallow whole lines for the members the reader had only put a
    /// character selection on.
    ///
    /// The caller resolves the next line's point itself and passes it to
    /// [`Self::grow_to`], because "the next line" is geometry the surface
    /// owns and this module deliberately does not.
    pub fn line_select_extends(&self) -> bool {
        !self.selections.is_empty()
            && self
                .selections
                .iter()
                .all(|selection| selection.mode == SelectionMode::Line && selection.growing)
    }

    /// Extend **every** member's head to `head`, keeping each anchor.
    ///
    /// One point for the whole set. Correct whenever the members share a head
    /// — which after any motion they do, because [`Self::move_to`] gives them
    /// all the same one. A surface that must walk each member to its own
    /// *next* line uses [`Self::grow_all_to`].
    pub fn grow_to(&mut self, head: P) {
        for selection in &mut self.selections {
            selection.head = head;
        }
        self.normalize();
    }

    /// Extend each member's head to a point resolved from that member.
    ///
    /// `resolve` is handed the member *before* it is modified, so a surface
    /// asking "the content start of the line after this member's head" reads
    /// the pre-growth head rather than the one this call is about to write.
    /// That is what makes `x` walk member 0 and member 1 to *their* next lines
    /// instead of to member 0's.
    pub fn grow_all_to(&mut self, mut resolve: impl FnMut(&Selection<P>) -> P) {
        for index in 0..self.selections.len() {
            let head = resolve(&self.selections[index]);
            self.selections[index].head = head;
        }
        self.normalize();
    }

    /// Deliberate cursor movement.
    ///
    /// In character mode the head follows, so the selection grows with the
    /// cursor. In line mode the selection is left exactly where it was and
    /// only its growth is disarmed — this is the rule that makes `x` mean
    /// "select this line" rather than "select everything between here and
    /// wherever I next wander".
    ///
    /// Every member extends, not just one. Helix resolves a motion to a single
    /// destination and moves every cursor to it, keeping each one's own
    /// anchor; a set where only the primary followed the cursor would leave
    /// the others frozen mid-selection.
    pub fn move_to(&mut self, cursor: P) {
        self.repin_cursor(cursor);
        for selection in &mut self.selections {
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
        let extends: Vec<bool> = self
            .selections
            .iter()
            .map(Selection::extends_on_move)
            .collect();
        self.cursor = cursor;
        for (selection, extends) in self.selections.iter_mut().zip(extends) {
            if extends {
                selection.head = cursor;
            }
        }
    }

    /// Drop the held selection, leaving the cursor alone.
    pub fn deactivate(&mut self) {
        self.clear();
    }

    /// Re-map every stored point — cursor, and every member's anchor and head.
    ///
    /// A scrolling viewport moves its content past a fixed frame, so stored
    /// points have to ride along or a selection would silently point at
    /// different text. The surface supplies the mapping because only it knows
    /// what "moved by one row" means for its coordinates.
    pub fn translate(&mut self, map: impl Fn(P) -> P) {
        self.cursor = map(self.cursor);
        for selection in &mut self.selections {
            selection.anchor = map(selection.anchor);
            selection.head = map(selection.head);
        }
        // A mapping that clamps can reorder members — two rows translating to
        // one row leave two members with the same head. Re-normalizing keeps
        // the document-order promise rather than trusting the caller.
        self.normalize();
    }

    // ── Set algebra ──────────────────────────────────────────────────────────
    //
    // Every operation below answers a verb over the *whole* set and reports
    // whether the set actually changed. That return value is load-bearing: it
    // is what lets the caller tell "this did nothing" from "this worked", so a
    // no-op is reported once instead of submitted as an edit that undoes
    // nothing. The M004 `case_range` rule, applied to the set.
    //
    // None of these reads text or resolves a motion, so none needs to know
    // what a point is — that is what keeps this module free of coordinates
    // and usable from the terminal panel as well as the editor.

    /// The head of member `index`.
    ///
    /// For a per-member motion — `C` walks each cursor to *its* next line, so
    /// the surface needs member `index`'s own position rather than the
    /// primary's.
    pub fn selection_point_of(&self, index: usize) -> Option<P> {
        self.selections.get(index).map(|selection| selection.head)
    }

    /// Replace the whole set in one call, reporting whether anything changed.
    ///
    /// The single constructor every verb that *builds* a set uses. Normalized
    /// like [`Self::restore_set`], and with the same reason: a caller that
    /// hands over unsorted members must not be able to leave the set in a
    /// state the rules above would never produce.
    ///
    /// An empty `selections` clears the set; one member is legal and means
    /// "one selection", not "no selection".
    pub fn set_members(
        &mut self,
        cursor: P,
        selections: Vec<Selection<P>>,
        primary: usize,
    ) -> bool {
        let changed =
            cursor != self.cursor || primary != self.primary || selections != self.selections;
        let (selections, primary) = normalize_set(selections, primary);
        self.cursor = cursor;
        self.selections = selections;
        self.primary = primary;
        changed
    }

    /// `;` — collapse the set to a single cursor at the primary's head.
    ///
    /// Helix calls this "collapse selection to a single cursor", and it means
    /// what it says: the selection goes and the cursor stays. Every other
    /// member is discarded, which is the whole point — it is the gesture for
    /// "stop tracking all those other cursors and just work here".
    ///
    /// The cursor is written explicitly rather than assumed already there,
    /// because a set restored from storage may name a primary whose head the
    /// cursor does not share.
    pub fn collapse_to_primary(&mut self) -> bool {
        let Some(selection) = self.selections.get(self.primary).copied() else {
            return false;
        };
        self.cursor = selection.head;
        self.clear();
        true
    }

    /// `Alt-;` — swap every member's anchor and head.
    ///
    /// No re-sort: [`Selection::bounds`] is `min`/`max` of the two points, so
    /// reversing a member cannot move it in document order. Re-normalizing here
    /// would be a sort that provably cannot change anything.
    pub fn flip_all(&mut self) -> bool {
        let mut changed = false;
        for selection in &mut self.selections {
            if selection.anchor != selection.head {
                changed = true;
            }
            mem::swap(&mut selection.anchor, &mut selection.head);
        }
        changed
    }

    /// `Alt-:` — make every member forward (`anchor <= head`).
    ///
    /// Idempotent, and it does not move the cursor: a direction is a property
    /// of a member, not a reason to relocate the reader.
    pub fn ensure_forward(&mut self) -> bool {
        let mut changed = false;
        for selection in &mut self.selections {
            if selection.anchor > selection.head {
                mem::swap(&mut selection.anchor, &mut selection.head);
                changed = true;
            }
        }
        changed
    }

    /// `,` — keep only the primary.
    ///
    /// A one-member set is already "only the primary", so this reports no-op
    /// rather than rewriting an identical set.
    pub fn keep_primary(&mut self) -> bool {
        if self.selections.len() <= 1 {
            return false;
        }
        let primary = self.selections[self.primary];
        self.selections = vec![primary];
        self.primary = 0;
        true
    }

    /// `Alt-,` — remove the primary, keeping every other member.
    ///
    /// Removing the last member leaves an **empty** set, which is a result and
    /// not a failure: the reader asked for one cursor fewer and there was only
    /// one cursor. `clear()` is the honest answer, not a panic.
    ///
    /// A survivor inherits the primary. Picking the member that now occupies
    /// the removed one's index keeps the distinguished cursor as close to the
    /// document position the reader was looking at as the set allows, and is
    /// the same member that would have been primary had the set been built
    /// without the removed one.
    pub fn remove_primary(&mut self) -> bool {
        if self.selections.is_empty() {
            return false;
        }
        let primary = self.primary.min(self.selections.len() - 1);
        let kept: Vec<Selection<P>> = self
            .selections
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != primary)
            .map(|(_, selection)| *selection)
            .collect();
        if kept.is_empty() {
            self.clear();
            return true;
        }
        self.selections = kept;
        self.primary = primary.min(self.selections.len() - 1);
        self.normalize();
        true
    }

    /// `(` / `)` — move which member is the primary.
    ///
    /// `delta` is in members and wraps. The cursor follows, because the cursor
    /// *is* the primary's head: leaving it behind would mean `cursor_byte` and
    /// the primary had silently diverged, and every reader of those two values
    /// — the viewport, LSP reads, the undo entry — would then be addressing a
    /// different selection than the one the editor thinks is primary.
    pub fn rotate_primary(&mut self, delta: isize) -> bool {
        let len = self.selections.len();
        if len <= 1 {
            return false;
        }
        let next = ((self.primary as isize) + delta).rem_euclid(len as isize) as usize;
        if next == self.primary {
            return false;
        }
        self.primary = next;
        self.cursor = self.selections[next].head;
        true
    }

    /// `Alt-minus` — merge every member into one spanning the whole set.
    ///
    /// The span runs from the earliest point any member reaches to the latest,
    /// which necessarily **bridges the gaps** between members — that is what
    /// merging selections means, and it is the same thing Helix does. The
    /// alternative reading, "fold the others into the primary's own range",
    /// would silently discard the members it was asked to merge.
    ///
    /// The merged member is Line mode if **any** member was: a merged Char
    /// selection would leave the lines a Line member covered unrepresented,
    /// which is the mixed-mode hazard [`SelectionMode`] exists to prevent.
    ///
    /// One member is already merged, so this reports no-op.
    pub fn merge_all(&mut self) -> bool {
        if self.selections.len() <= 1 {
            return false;
        }
        let (first, last) = self.span_of_set();
        let mode = self.widest_mode();
        self.selections = vec![Selection {
            anchor: first,
            head: last,
            mode,
            growing: false,
        }];
        self.primary = 0;
        // Forward, so the cursor sits at the span's end. Consistent with the
        // invariant that the cursor is the primary's head rather than a
        // special case for this one verb.
        self.cursor = last;
        true
    }

    /// `Alt-_` — merge members that are **adjacent**, leaving gaps alone.
    ///
    /// `adjacent` is supplied by the caller because "adjacent" is geometry:
    /// two character selections one column apart on the same line touch, two
    /// on adjacent lines touch, and two with a line between them do not — and
    /// only the surface knows how wide a character is. The grouping itself is
    /// a set rule, so it stays here.
    ///
    /// Groups keep document order, so the primary stays attached to its member
    /// through the merge.
    pub fn merge_consecutive(
        &mut self,
        mut adjacent: impl FnMut(&Selection<P>, &Selection<P>) -> bool,
    ) -> bool {
        if self.selections.len() <= 1 {
            return false;
        }
        let mut merged: Vec<Selection<P>> = Vec::with_capacity(self.selections.len());
        let mut changed = false;
        for selection in &self.selections {
            let touches_previous = merged
                .last()
                .is_some_and(|previous| adjacent(previous, selection));
            match merged.last_mut() {
                Some(previous) if touches_previous => {
                    // A group covers from its first member's earliest point to
                    // its last member's latest, and takes the widest mode of
                    // the two, so merging a Line member with a Char one cannot
                    // shrink what the group covers.
                    let (previous_first, _) = previous.bounds();
                    let (_, member_last) = selection.bounds();
                    previous.anchor = previous_first.min(selection.anchor);
                    previous.head = previous.head.max(selection.head).max(member_last);
                    if selection.mode == SelectionMode::Line {
                        previous.mode = SelectionMode::Line;
                    }
                    changed = true;
                }
                _ => merged.push(*selection),
            }
        }
        if !changed {
            return false;
        }
        self.selections = merged;
        self.normalize();
        // The primary's own head did not move; it may now sit inside a wider
        // member. The cursor is the primary's head, so it is unchanged — which
        // is what makes this safe to do without the reader losing their place.
        self.cursor = self.selections[self.primary].head;
        true
    }

    /// Earliest and latest points the whole set reaches.
    ///
    /// Over [`Selection::bounds`] rather than over anchors alone: a member is
    /// a span between two points, and a merge that considered only its anchor
    /// would clip off whichever end the anchor was not on.
    fn span_of_set(&self) -> (P, P) {
        let mut span = self.selections[0].bounds();
        for selection in &self.selections[1..] {
            let (first, last) = selection.bounds();
            span = (span.0.min(first), span.1.max(last));
        }
        span
    }

    /// The coarsest granularity in the set: Line if any member is.
    fn widest_mode(&self) -> SelectionMode {
        if self
            .selections
            .iter()
            .any(|selection| selection.mode == SelectionMode::Line)
        {
            SelectionMode::Line
        } else {
            SelectionMode::Char
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Selection, SelectionMode, SelectionPoint, SelectionState};

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

    fn state_at(row: u16, col: u16) -> SelectionState<Cell> {
        SelectionState::new(Cell(row, col))
    }

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

    // ── The set ──────────────────────────────────────────────────────────

    /// A member, grown to `head`. Line members come up armed for `x`, which is
    /// how `select_line` leaves them — an unarmed line member would make
    /// `line_select_extends` false for the wrong reason and the growth tests
    /// would pass without exercising growth.
    fn held(anchor: Cell, head: Cell, mode: SelectionMode) -> Selection<Cell> {
        let mut selection = super::Selection::new(anchor, mode);
        selection.head = head;
        selection.growing = mode == SelectionMode::Line;
        selection
    }

    /// A three-member set whose members were handed over **out of order**.
    fn unordered_set() -> SelectionState<Cell> {
        SelectionState::restore_set(
            cell(5, 0),
            vec![
                held(cell(9, 0), cell(9, 3), SelectionMode::Char),
                held(cell(1, 0), cell(1, 1), SelectionMode::Char),
                held(cell(5, 0), cell(5, 2), SelectionMode::Char),
            ],
            // The member about to be buried in the middle, not the first one.
            0,
        )
    }

    #[test]
    fn members_are_ordered_by_bounds_not_by_the_order_they_arrived() {
        let state = unordered_set();
        let firsts: Vec<Cell> = state
            .selections()
            .iter()
            .map(|selection| selection.bounds().0)
            .collect();
        assert_eq!(
            firsts,
            vec![cell(1, 0), cell(5, 0), cell(9, 0)],
            "insertion order would leave a sort-by-bounds implementation passing \
             only because this test happened to be written after the sort"
        );
    }

    #[test]
    fn the_primary_travels_with_its_member_through_a_sort() {
        let state = unordered_set();
        // `restore_set` normalizes on the way in, so the primary is already
        // where sorting put it rather than at the index it was named with.
        let primary = state.primary().expect("primary");
        // The member that was primary is the one anchored at (9, 0); after
        // sorting it is the last. A primary pinned to an index rather than to
        // a member would answer about (1, 0) here.
        assert_eq!(primary.anchor, cell(9, 0));
        assert_eq!(state.primary_index(), Some(2));
    }

    #[test]
    fn restoring_a_set_clamps_a_primary_that_names_no_member() {
        let state = SelectionState::restore_set(
            cell(0, 0),
            vec![held(cell(1, 0), cell(1, 1), SelectionMode::Char)],
            7,
        );
        assert_eq!(
            state.primary_index(),
            Some(0),
            "an out-of-range primary must not make every accessor answer None"
        );
        assert!(state.primary().is_some());
    }

    #[test]
    fn movement_extends_every_extendable_member_and_leaves_the_rest_alone() {
        let mut state = SelectionState::restore_set(
            cell(5, 2),
            vec![
                held(cell(1, 0), cell(1, 4), SelectionMode::Char),
                held(cell(5, 0), cell(5, 2), SelectionMode::Line),
                held(cell(9, 0), cell(9, 4), SelectionMode::Char),
            ],
            1,
        );
        state.move_to(cell(7, 1));
        let selections = state.selections();
        assert_eq!(selections.len(), 3, "no member is dropped or merged away");
        // The two character members follow the cursor…
        assert!(selections
            .iter()
            .filter(|selection| selection.mode == SelectionMode::Char)
            .all(|selection| selection.head == cell(7, 1)));
        // …and the line member keeps the extent the reader gave it.
        let line = selections
            .iter()
            .find(|selection| selection.mode == SelectionMode::Line)
            .expect("line member");
        assert_eq!(line.bounds(), (cell(5, 0), cell(5, 2)));
    }

    #[test]
    fn x_growth_is_refused_on_a_mixed_set_so_it_cannot_swallow_whole_lines() {
        let mut state = SelectionState::restore_set(
            cell(4, 0),
            vec![
                held(cell(1, 0), cell(2, 0), SelectionMode::Line),
                held(cell(4, 0), cell(4, 1), SelectionMode::Char),
            ],
            0,
        );
        assert!(
            !state.line_select_extends(),
            "a set that is not uniformly a growing line selection must not be \
             treated as growable: `x` would add lines to a member the reader \
             only put a character selection on"
        );
        // Re-anchoring is the fallback, and it must reach every member.
        state.select_line();
        assert!(state.line_select_extends());
        assert!(state
            .selections()
            .iter()
            .all(|selection| selection.mode == SelectionMode::Line && selection.growing));
    }

    #[test]
    fn x_grows_each_member_to_its_own_next_line() {
        let mut state = SelectionState::restore_set(
            cell(6, 0),
            vec![
                held(cell(2, 0), cell(2, 0), SelectionMode::Line),
                held(cell(6, 0), cell(6, 0), SelectionMode::Line),
            ],
            0,
        );
        assert!(state.line_select_extends());
        state.grow_all_to(|selection| cell(selection.head.0 + 1, 0));
        let ranges: Vec<(u16, u16)> = state
            .selections()
            .iter()
            .map(|selection| selection.line_range())
            .collect();
        assert_eq!(ranges, vec![(2, 3), (6, 7)]);
    }

    #[test]
    fn v_on_a_mixed_set_arms_every_member_rather_than_half_clearing() {
        let mut state = SelectionState::restore_set(
            cell(4, 2),
            vec![
                held(cell(1, 0), cell(2, 0), SelectionMode::Line),
                held(cell(4, 0), cell(4, 2), SelectionMode::Char),
            ],
            0,
        );
        state.toggle_char();
        assert_eq!(state.selections().len(), 2, "no member is dropped");
        assert!(state
            .selections()
            .iter()
            .all(|selection| selection.mode == SelectionMode::Char));
        // Every member is now a character selection, so a second `v` is a
        // real toggle and clears the whole set.
        state.toggle_char();
        assert!(state.selections().is_empty());
    }

    #[test]
    fn a_second_v_on_a_purely_character_set_clears_all_of_it() {
        let mut state = SelectionState::restore_set(
            cell(4, 2),
            vec![
                held(cell(1, 0), cell(1, 3), SelectionMode::Char),
                held(cell(4, 0), cell(4, 2), SelectionMode::Char),
            ],
            0,
        );
        state.toggle_char();
        assert!(state.selections().is_empty());
    }

    #[test]
    fn translate_moves_every_member_and_restores_document_order() {
        // Two members that a clamping translate would otherwise reorder.
        let mut state = SelectionState::restore_set(
            cell(5, 0),
            vec![
                held(cell(5, 0), cell(5, 1), SelectionMode::Char),
                held(cell(6, 0), cell(6, 1), SelectionMode::Char),
            ],
            0,
        );
        state.translate(|Cell(row, col)| Cell(row.saturating_sub(1), col));
        let firsts: Vec<Cell> = state
            .selections()
            .iter()
            .map(|selection| selection.bounds().0)
            .collect();
        assert_eq!(firsts, vec![cell(4, 0), cell(5, 0)]);
        assert_eq!(state.cursor(), cell(4, 0));
    }

    #[test]
    fn the_empty_set_answers_every_accessor_without_panicking() {
        let mut state = SelectionState::restore_set(cell(2, 2), Vec::new(), 3);
        assert_eq!(state.cursor(), cell(2, 2));
        assert!(state.selection().is_none());
        assert!(state.primary().is_none());
        assert!(!state.is_selected());
        assert!(state.span().is_none());
        assert!(state.line_range().is_none());
        assert!(state.selections().is_empty());
        assert_eq!(state.primary_index(), None);
        assert!(!state.line_select_extends());
        // Growing an empty set is a no-op rather than an index panic, and the
        // verbs that populate one still work from empty.
        state.grow_to(cell(9, 9));
        assert!(state.selections().is_empty());
        state.move_to(cell(3, 3));
        state.translate(|cell| cell);
        state.deactivate();
        assert!(state.selections().is_empty());
        state.toggle_char();
        assert_eq!(state.selections().len(), 1);
    }

    #[test]
    fn an_empty_set_still_arms_a_single_member_from_the_cursor() {
        let mut state = state_at(4, 1);
        state.select_line();
        assert_eq!(state.selections().len(), 1);
        assert_eq!(state.primary().map(|s| s.anchor), Some(cell(4, 1)));
        assert!(state.line_select_extends());
    }

    #[test]
    fn deactivate_drops_every_member_at_once() {
        let mut state = unordered_set();
        state.deactivate();
        assert!(state.selections().is_empty());
        assert_eq!(state.cursor(), cell(5, 0), "the cursor is left alone");
    }

    #[test]
    fn into_parts_returns_the_set_ordered_and_the_primary_in_range() {
        let state = unordered_set();
        let (cursor, selections, primary) = state.into_parts();
        assert_eq!(cursor, cell(5, 0));
        assert_eq!(selections.len(), 3);
        assert_eq!(primary, 2);
        assert_eq!(selections[primary].anchor, cell(9, 0));
    }

    // ── Set algebra ──────────────────────────────────────────────────────

    /// A three-member set of single points, primary in the middle.
    ///
    /// Single points because the verbs below are set algebra and the set is
    /// the thing under test; a selection's *span* is `editor`'s job and is
    /// exercised there.
    fn three_members() -> SelectionState<Cell> {
        SelectionState::restore_set(
            cell(5, 0),
            vec![
                held(cell(1, 0), cell(1, 0), SelectionMode::Char),
                held(cell(5, 0), cell(5, 0), SelectionMode::Char),
                held(cell(9, 0), cell(9, 0), SelectionMode::Char),
            ],
            1,
        )
    }

    fn heads(state: &SelectionState<Cell>) -> Vec<Cell> {
        state
            .selections()
            .iter()
            .map(|selection| selection.head)
            .collect()
    }

    #[test]
    fn collapse_keeps_only_the_primary_and_leaves_a_cursor_where_it_was() {
        let mut state = three_members();
        assert!(state.collapse_to_primary());
        assert!(state.selections().is_empty());
        assert_eq!(
            state.cursor(),
            cell(5, 0),
            "the reader stays where the primary was"
        );
        assert!(
            !state.collapse_to_primary(),
            "collapsing with nothing held is a no-op, not a second event"
        );
    }

    #[test]
    fn flip_swaps_every_direction_without_moving_a_single_member() {
        let mut state = SelectionState::restore_set(
            cell(0, 0),
            vec![
                held(cell(1, 2), cell(1, 7), SelectionMode::Char),
                held(cell(4, 9), cell(4, 3), SelectionMode::Char),
            ],
            0,
        );
        let before = state
            .selections()
            .iter()
            .map(|member| member.bounds())
            .collect::<Vec<_>>();
        assert!(state.flip_all());
        assert_eq!(
            state
                .selections()
                .iter()
                .map(|member| member.bounds())
                .collect::<Vec<_>>(),
            before,
            "a member's bounds are `min`/`max` of its two points, so reversing \
             it cannot move it — only change which end leads"
        );
        // Two flips is the identity. It is *not* reported as a no-op, and
        // should not be: `Alt-;` is an involution, not an idempotent
        // operation, so the second call is a real change that happens to
        // restore the first one's effect.
        assert!(state.flip_all());
        assert_eq!(
            state
                .selections()
                .iter()
                .map(|member| member.bounds())
                .collect::<Vec<_>>(),
            before
        );

        // The genuine no-op is a member that **is** its own reverse.
        let mut points = SelectionState::restore_set(
            cell(0, 0),
            vec![
                held(cell(2, 2), cell(2, 2), SelectionMode::Char),
                held(cell(5, 5), cell(5, 5), SelectionMode::Char),
            ],
            0,
        );
        assert!(!points.flip_all(), "a point has no direction to reverse");
    }

    #[test]
    fn ensure_forward_is_idempotent_and_does_not_move_the_cursor() {
        let mut state = SelectionState::restore_set(
            cell(0, 0),
            vec![
                held(cell(1, 7), cell(1, 2), SelectionMode::Char),
                held(cell(4, 3), cell(4, 9), SelectionMode::Char),
            ],
            0,
        );
        assert!(state.ensure_forward());
        assert_eq!(state.cursor(), cell(0, 0), "the cursor is not moved");
        assert!(
            state.selections().iter().all(|m| m.anchor <= m.head),
            "every member ends forward"
        );
        assert!(
            !state.ensure_forward(),
            "an already-forward set is a no-op, so nothing is reported"
        );
    }

    #[test]
    fn keeping_the_primary_of_one_member_is_a_no_op() {
        let mut state = SelectionState::new(cell(2, 0));
        state.select_char();
        let before = state.selections().to_vec();
        assert!(!state.keep_primary());
        assert_eq!(state.selections(), before.as_slice());
    }

    #[test]
    fn removing_the_primary_leaves_a_survivor_that_inherits_it() {
        let mut state = three_members();
        assert!(state.remove_primary());
        assert_eq!(heads(&state), vec![cell(1, 0), cell(9, 0)]);
        assert_eq!(
            state.primary_index(),
            Some(1),
            "the member that took its document position inherits the role"
        );
        assert_eq!(state.cursor(), cell(5, 0), "the cursor itself did not move");
    }

    #[test]
    fn removing_the_last_member_clears_the_set_rather_than_panicking() {
        let mut state = SelectionState::new(cell(4, 2));
        state.select_char();
        assert!(state.remove_primary());
        assert!(state.selections().is_empty());
        assert_eq!(state.primary_index(), None);
        assert_eq!(
            state.cursor(),
            cell(4, 2),
            "the cursor survives the empty set"
        );
    }

    #[test]
    fn rotating_the_primary_wraps_and_carries_the_cursor_with_it() {
        let mut state = three_members();
        // The cursor is the primary's head; rotating without moving it would
        // leave the two disagreeing about which member is focused.
        assert_eq!(state.cursor(), cell(5, 0));
        assert!(state.rotate_primary(1));
        assert_eq!(state.primary_index(), Some(2));
        assert_eq!(state.cursor(), cell(9, 0));
        assert!(state.rotate_primary(1), "wraps past the end");
        assert_eq!(state.primary_index(), Some(0));
        assert_eq!(state.cursor(), cell(1, 0));
        assert!(state.rotate_primary(-1), "and back again");
        assert_eq!(state.primary_index(), Some(2));
    }

    #[test]
    fn rotating_one_member_reports_a_no_op_rather_than_a_lost_cursor() {
        let mut state = SelectionState::new(cell(3, 1));
        state.select_char();
        assert!(!state.rotate_primary(1));
        assert_eq!(state.cursor(), cell(3, 1));
    }

    #[test]
    fn merging_everything_spans_the_whole_set_and_takes_the_widest_mode() {
        let mut state = SelectionState::restore_set(
            cell(1, 0),
            vec![
                held(cell(2, 0), cell(2, 4), SelectionMode::Char),
                held(cell(7, 0), cell(7, 0), SelectionMode::Line),
                held(cell(5, 0), cell(5, 2), SelectionMode::Char),
            ],
            0,
        );
        assert!(state.merge_all());
        assert_eq!(state.selections().len(), 1);
        let merged = state.primary().expect("one member");
        assert_eq!(merged.bounds(), (cell(2, 0), cell(7, 0)));
        assert_eq!(
            merged.mode,
            SelectionMode::Line,
            "a merged Char member would leave the Line member's rows unrepresented"
        );
        assert!(!state.merge_all(), "one member is already merged");
    }

    #[test]
    fn merge_all_of_a_single_member_is_a_no_op() {
        let mut state = SelectionState::new(cell(2, 0));
        state.select_line();
        assert!(!state.merge_all());
    }

    #[test]
    fn merging_consecutive_groups_adjacent_members_and_leaves_gaps() {
        // 0/1 and 4/5 touch (1 <= 5 and 4 <= 1 is false — so they do NOT).
        // Built explicitly so the answer is about the rule, not the fixture.
        let mut state = SelectionState::restore_set(
            cell(0, 0),
            vec![
                held(cell(0, 0), cell(0, 1), SelectionMode::Char),
                held(cell(0, 2), cell(0, 3), SelectionMode::Char),
                held(cell(0, 9), cell(0, 10), SelectionMode::Char),
            ],
            2,
        );
        // Adjacency is supplied by the surface, so the rule is exercised with
        // the geometry spelled out here: same row, and the next column at
        // worst one past where the previous member ended.
        let touching = |previous: &Selection<Cell>, member: &Selection<Cell>| {
            let last = previous.bounds().1;
            let first = member.bounds().0;
            first.0 == last.0 && first.1 <= last.1.saturating_add(1)
        };
        assert!(state.merge_consecutive(touching));
        let groups = state
            .selections()
            .iter()
            .map(|member| member.bounds())
            .collect::<Vec<_>>();
        assert_eq!(
            groups,
            vec![(cell(0, 0), cell(0, 3)), (cell(0, 9), cell(0, 10))],
            "the first pair merged; the one across a gap did not"
        );
        assert_eq!(
            state.primary_index(),
            Some(1),
            "the primary followed its member into the surviving group"
        );
    }

    #[test]
    fn merging_consecutive_reports_a_no_op_when_nothing_is_adjacent() {
        let mut state = three_members();
        assert!(!state.merge_consecutive(|_, _| false));
        assert_eq!(state.selections().len(), 3, "the set is untouched");
    }

    #[test]
    fn set_members_is_the_constructor_and_reports_whether_it_changed_anything() {
        let mut state = SelectionState::new(cell(1, 0));
        assert!(state.set_members(cell(2, 0), vec![], 0));
        assert!(state.selections().is_empty());
        let same = state.set_members(cell(2, 0), vec![], 0);
        assert!(!same, "writing an identical set changes nothing");
        assert!(state.set_members(
            cell(2, 0),
            vec![held(cell(2, 0), cell(2, 0), SelectionMode::Char)],
            0
        ));
        assert!(!state.set_members(
            cell(2, 0),
            vec![held(cell(2, 0), cell(2, 0), SelectionMode::Char)],
            0
        ));
    }

    #[test]
    fn a_members_own_point_is_readable_without_touching_the_primary() {
        let state = three_members();
        assert_eq!(state.selection_point_of(0), Some(cell(1, 0)));
        assert_eq!(state.selection_point_of(2), Some(cell(9, 0)));
        assert_eq!(state.selection_point_of(3), None);
    }

    #[test]
    fn every_set_verb_answers_the_empty_set_without_panicking() {
        let mut state = SelectionState::<Cell>::new(cell(3, 3));
        assert!(!state.collapse_to_primary());
        assert!(!state.flip_all());
        assert!(!state.ensure_forward());
        assert!(!state.keep_primary());
        assert!(!state.remove_primary());
        assert!(!state.rotate_primary(1));
        assert!(!state.merge_all());
        assert!(!state.merge_consecutive(|_, _| true));
        assert_eq!(state.cursor(), cell(3, 3));
    }

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

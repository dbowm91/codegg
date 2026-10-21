//! M006-D: project-scoped file-tree view state.
//!
//! This holds *view* state only. The walk itself lives in
//! [`crate::tui::file_tree`], and opening a selected entry is the caller's
//! job through `open_editor`, which is the only path that turns a path into a
//! document attachment. No document text is retained here.
//!
//! Request/generation discipline matches `EditorState`: a walk that completes
//! after the user switched project tab or asked for a rebuild is discarded
//! rather than applied to the wrong workspace.
//!
//! ## The pane has a scope, and the scope moves
//!
//! [`FileTreeState::root_prefix`] is the directory whose children the pane
//! lists; `""` is the workspace root. Navigation is a **peek** motion, not a
//! re-root: `l` expands the selected directory *in place* and moves the
//! cursor into it, and `h` moves the cursor back out to the parent row and
//! collapses it. The daemon's listing window is three levels deep, so a
//! directory at the window's edge cannot show its children — the scope
//! **slides** instead: the scope becomes that directory's parent and the
//! directory is expanded there. The scope can therefore advance without
//! bound while the *relative* depth of the rows never leaves the window.
//!
//! [`FileTreeState::root_history`] is the chain of scopes the pane has moved
//! through, so a scope slide can be walked back out one level at a time. The
//! current scope is always the last entry, which is what makes "which
//! directory am I looking at" one fact instead of a prefix field and a
//! history field that can disagree.
//!
//! A scope change is a **daemon round-trip**, not a local filter: the scope
//! is carried as `WorkspaceDirectoryListRequestDto::prefix`, and the daemon
//! walks that subtree with the same bounded walker. Filtering an
//! already-loaded listing would have been free, but it would silently drop
//! everything the original walk hit its bounds on, presenting a partial
//! subtree as if it were the directory.
//!
//! ## The panel's own filter is not a scope
//!
//! [`FileTreeState::filter`] narrows the rows the pane *shows*; it never
//! changes what was asked for, so it issues no request and cannot present a
//! partial subtree as a complete one. The panel's mode
//! ([`TreePanelMode`]) is deliberately separate from the app-wide
//! `InputMode`: the composer badge reports the app-level mode, and a
//! panel-local insert must never be reported as the composer's insert.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::tui::app::state::AsyncUiRequestState;
use crate::tui::file_tree::{visible_rows, TreeListing, TreeNodeKind, TreeRow};

/// Page size used when the pane has not rendered yet and therefore has no
/// measured viewport. A first keypress can arrive before the first frame on a
/// very fast paste, and a page verb must never become a no-op there.
pub const FALLBACK_PAGE_ROWS: usize = 10;

/// Deepest indent a row may draw.
///
/// Indentation is orientation, the name is content: past this a nested row
/// would push its name out of a narrow column entirely. Defined once here
/// because the `/files` box and the Ctrl+T pane used to carry independent
/// copies of this number and nothing kept them equal.
///
/// The cap is a *render* bound, not a navigation one. It used to be
/// load-bearing as well, because the indent was the absolute depth and the
/// absolute depth grew with the scope; the indent is now relative to the
/// scope ([`FileTreeState::display_depth`]), so the relative depth cannot
/// grow and the cap only ever bites on a column too narrow for the guide.
pub const MAX_INDENT_DEPTH: usize = 8;

/// How deep to draw one row, and whether it closes its own level.
///
/// Purely presentational: nothing here feeds back into navigation, expansion or
/// selection, which is what keeps the hierarchy fix out of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TreeIndent {
    /// Depth to draw, scope included. `0` draws no guide at all.
    pub depth: usize,
    /// Whether the row is the last at its depth — `└─` rather than `├─`.
    pub last: bool,
}

/// Why the pane is showing what it is showing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FileTreeStatus {
    /// Nothing requested yet.
    #[default]
    Idle,
    /// A walk is in flight.
    Loading,
    /// C004: a rebuild is queued behind an in-flight blocking scan. The
    /// newest request is coalesced as the single pending intent; no new
    /// blocking work spawns until the active worker exits. The event loop
    /// stays responsive; this is not an empty workspace.
    Waiting,
    /// A walk completed for the current root.
    Ready,
    /// The walk could not run (for example the workspace root is unreadable).
    Error(String),
}

/// The tree pane's own input mode.
///
/// Deliberately **not** the app-wide `InputMode`: the composer badge
/// reports the app-level mode, and a panel-local insert must never be
/// reported as the composer's insert — the two are different surfaces with
/// different jobs. The panel's mode only decides which keys the pane claims
/// (see `commands::file_tree::handle_panel_key`); the app-level mode stays
/// `Normal` while the pane filters, so the composer is never left in a mode
/// the user did not enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TreePanelMode {
    /// Navigation keys: `j`/`k`, `l`/`h`, `Enter`, `r`, paging.
    #[default]
    Normal,
    /// The filter is being typed. Printable keys edit the filter, `Backspace`
    /// deletes from it, `Esc` leaves insert mode and **keeps** the filter.
    Insert,
}

/// What a peek motion decided to do.
///
/// The decision is taken on the state (it is pure navigation arithmetic over
/// the current listing); the *effect* that needs the daemon — a scope slide —
/// is applied by the command layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeekStep {
    /// Expand `directory` in place and move the cursor into it. Its children
    /// are already inside the listing window.
    Enter {
        /// The directory to expand and enter.
        directory: String,
    },
    /// The directory's children are outside the listing window: slide the
    /// scope to `scope` (the directory's parent), expand the directory
    /// there, and move the cursor into it.
    Slide {
        /// The new scope: the directory's parent.
        scope: String,
        /// The directory to expand in the new scope.
        directory: String,
    },
    /// Nothing to peek at: a file, or no selection.
    None,
}

/// What the `h`/left motion decided to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeekUp {
    /// The cursor was inside a directory: move it to the parent row and
    /// collapse that directory.
    Leave {
        /// The parent row's path.
        parent: String,
    },
    /// The cursor was on a top-level row and the scope can go up: slide the
    /// scope to `scope` (the current scope's parent) with the old scope root
    /// expanded there.
    Slide {
        /// The new scope.
        scope: String,
        /// The old scope root, expanded in the new scope.
        root: String,
    },
    /// The workspace root: the scope cannot go higher, so the selected
    /// directory collapses in place. Never a dead key.
    Collapse {
        /// The directory to collapse.
        directory: String,
    },
    /// Nothing to leave: no selection, or a file at the workspace root.
    None,
}

#[derive(Debug, Clone, Default)]
pub struct FileTreeState {
    /// Whether the pane is rendered. `Default` is hidden so every pre-existing
    /// TUI test and the whole M006-A surface are unaffected until the user
    /// explicitly opens the tree.
    pub visible: bool,
    /// The workspace root the current listing belongs to, captured at request
    /// time so a completion can be matched against the tab it was issued for.
    pub root: Option<PathBuf>,
    /// Directory paths the user has expanded, workspace-relative.
    pub expanded: BTreeSet<String>,
    /// Workspace-relative path of the selected row.
    pub selected: Option<String>,
    /// Scroll offset into the visible rows.
    pub scroll: usize,
    /// Nodes from the last completed walk.
    pub nodes: Vec<crate::tui::file_tree::TreeNode>,
    /// Truncation notice from the last completed walk.
    pub truncated: Option<String>,
    /// Last observed status.
    pub status: FileTreeStatus,
    /// Request tracking for async walk completions.
    pub request: AsyncUiRequestState,
    /// Bumped on every rebuild and on every project switch, so a completion
    /// for a previous walk is discarded.
    pub generation: u64,
    /// Cooperative cancellation for the in-flight blocking walk.
    ///
    /// C002: set on every rebuild and project reset, checked by
    /// `build_tree_with_cancel` on every entry. An aborted async waiter
    /// cannot stop blocking `read_dir` work; this flag is what bounds the
    /// stranded scan to one more entry. Shared with the spawned blocking
    /// task, so it is deliberately not part of equality or debug output
    /// beyond the pointer.
    pub cancel_flag: Option<Arc<AtomicBool>>,
    /// Directories walked into with `l`, oldest first; the last entry is the
    /// directory this pane is listing right now.
    ///
    /// Starts empty, which reads as the workspace root. `h` pops it. Only
    /// [`Self::set_root_prefix`] and [`Self::ascend_root`] write it, so the
    /// scope can never be changed by anything else.
    pub root_history: Vec<String>,
    /// The scope the in-flight walk was issued for, captured at
    /// `begin_rebuild` time.
    ///
    /// Separate from the current root because they answer different
    /// questions: this one is "what did I ask the daemon for", which is what
    /// an arriving completion is checked against — the daemon echoes the
    /// prefix it answered, so a mismatched completion is discarded instead of
    /// painting a subtree the pane is no longer pointing at.
    pub listing_prefix: String,
    /// Rows the pane can currently show, published by the renderer after it
    /// has computed the viewport.
    ///
    /// Layout, not navigation: `move_selection` cannot scroll sensibly
    /// without knowing how many rows fit, and the renderer is the only place
    /// that knows. `0` means "not measured yet", which pages fall back from.
    pub viewport_rows: usize,
    /// The pane's own input mode.
    ///
    /// Panel-scoped, not app-scoped: see [`TreePanelMode`]. The composer
    /// badge reports `InputMode`, and this one must never leak into it.
    pub panel_mode: TreePanelMode,
    /// The panel's live filter over the current listing.
    ///
    /// Empty means "no filter", which shows every row. The filter narrows
    /// what the pane *shows*; it is not a scope change, so it never issues a
    /// request and never re-scopes the listing.
    pub filter: String,
    /// The directory the cursor must enter once the relist lands.
    ///
    /// A scope slide replaces the listing, so the row the cursor belongs on
    /// (the first child of the directory the user entered) is not known
    /// until the daemon answers. The path is carried here and applied when
    /// the listing arrives.
    pub pending_cursor: Option<String>,
    /// The file opened from this pane, awaiting the editor handoff.
    ///
    /// The open is asynchronous, so the handoff cannot happen when the key
    /// is pressed: the document has to be attached first. The command layer
    /// records the path here, and the completion path calls
    /// `commands::file_tree::complete_editor_open_handoff`, which only
    /// hands over if this is still the document that was opened.
    pub pending_editor_handoff: Option<String>,
}

impl FileTreeState {
    /// Every row the listing holds, ignoring the panel's filter.
    ///
    /// The expansion set decides which rows exist at all; the filter only
    /// decides which of them the pane *shows*.
    pub fn all_rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        visible_rows(&self.nodes, &self.expanded, &mut rows);
        rows
    }

    /// Rows the user can currently move between.
    ///
    /// The listing the daemon returned, narrowed by the panel's live filter.
    /// An empty filter matches everything, so this is the plain listing.
    ///
    /// The selection is clamped rather than dropped, so a collapse or a
    /// shrink never leaves the tree with no reachable row.
    pub fn rows(&self) -> Vec<TreeRow> {
        let rows = self.all_rows();
        if self.filter.is_empty() {
            return rows;
        }
        let needle = self.filter.to_lowercase();
        rows.into_iter()
            .filter(|row| matches_filter(row, &needle))
            .collect()
    }

    /// Whether `row` survives the panel's live filter.
    ///
    /// Case-insensitive, and it matches the file *name* as well as the path:
    /// typing `mod` finds `mod.rs` whether the pane sits at the workspace
    /// root or four directories deep.
    pub fn filter_matches(&self, row: &TreeRow) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        matches_filter(row, &self.filter.to_lowercase())
    }

    /// Keep the cursor on a row the filter still shows.
    ///
    /// The filter can hide the selected row; the cursor then falls back to
    /// the first row that survives, so the pane never claims a selection the
    /// user cannot see.
    pub fn reanchor_selection(&mut self) {
        let rows = self.rows();
        let Some(first) = rows.first() else {
            self.selected = None;
            return;
        };
        if self
            .selected
            .as_deref()
            .is_some_and(|path| rows.iter().any(|row| row.path == path))
        {
            return;
        }
        self.selected = Some(first.path.clone());
    }

    /// Index of the selected row within [`Self::rows`].
    pub fn selected_index(&self) -> Option<usize> {
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let index = self
            .selected
            .as_deref()
            .and_then(|path| rows.iter().position(|row| row.path == path))
            .unwrap_or(0);
        Some(index.min(rows.len() - 1))
    }

    /// The selected row, if any.
    pub fn selected_row(&self) -> Option<TreeRow> {
        let index = self.selected_index()?;
        self.rows().into_iter().nth(index)
    }

    /// Move the selection by `delta` rows, clamped to the visible range.
    pub fn move_selection(&mut self, delta: isize) {
        let rows = self.rows();
        if rows.is_empty() {
            self.selected = None;
            return;
        }
        let current = self.selected_index().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, rows.len() as isize - 1) as usize;
        self.selected = Some(rows[next].path.clone());
        self.reveal_selected(next, rows.len());
    }

    /// How many rows the pane can show right now.
    ///
    /// [`Self::viewport_rows`] is published by the renderer, and it is `0`
    /// until the first frame has been drawn. A page verb must never be a
    /// no-op because the pane has not been measured yet, so the unmeasured
    /// case falls back to [`FALLBACK_PAGE_ROWS`].
    pub fn viewport_height(&self) -> usize {
        if self.viewport_rows == 0 {
            FALLBACK_PAGE_ROWS
        } else {
            self.viewport_rows
        }
    }

    /// Scroll so the selected row stays inside the measured window.
    ///
    /// The clamp is against the **window**, not against the row count: a
    /// scroll offset of `total - 1` is the last row of a listing, which
    /// means a page of it has scrolled off the top and the pane shows one
    /// row at the bottom. The offset may therefore never exceed
    /// `total - viewport_height`, and the selected row is revealed inside
    /// that window.
    pub fn reveal_selected(&mut self, index: usize, total: usize) {
        if total == 0 {
            self.scroll = 0;
            return;
        }
        let height = self.viewport_height();
        let max_scroll = total.saturating_sub(height);
        self.scroll = self.scroll.min(max_scroll);
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + height {
            self.scroll = index + 1 - height;
        }
    }

    /// Scroll so `index` is visible within a `height`-row viewport.
    pub fn ensure_visible(&mut self, index: usize, height: usize) {
        if height == 0 {
            return;
        }
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + height {
            self.scroll = index + 1 - height;
        }
    }

    /// Rows one `Ctrl+d`/`Ctrl+u` press moves.
    ///
    /// Two thirds of the viewport, so consecutive pages keep a row of overlap
    /// — a full page scrolls *past* a name the user was reaching for, and a
    /// paging verb that overshoots reads as a broken one.
    pub fn page_step(&self) -> isize {
        ((self.viewport_height() * 2) / 3).max(1) as isize
    }

    /// Page the cursor by `direction` pages: `1` down, `-1` up.
    ///
    /// `Ctrl+d`/`Ctrl+u` on a listing long enough to need paging. The
    /// selection moves a full page *and* the scroll follows it, because a
    /// page verb that moved the highlight without moving the listing would
    /// scroll only after the cursor had already left the screen.
    ///
    /// The page is measured against the pane's own height, falling back to
    /// [`FALLBACK_PAGE_ROWS`] before the first frame: passing `0` to
    /// [`Self::ensure_visible`] would make the verb a no-op, which is how a
    /// `Ctrl+d` could leave the listing scrolled off the page.
    pub fn page_selection(&mut self, direction: isize) {
        if self.rows().is_empty() {
            return;
        }
        self.move_selection(direction * self.page_step());
        if let Some(index) = self.selected_index() {
            self.ensure_visible(index, self.viewport_height());
        }
    }

    /// Expand or collapse the selected directory.
    ///
    /// Returns the path that changed, so a caller can report it. A file
    /// selection is a no-op: expanding a file is not a meaningful action.
    ///
    /// The *verb* this used to be bound to is retired: `l` expands and
    /// enters, `h` leaves and collapses, so the standalone toggle has no
    /// key left. What is left is the state-level helper the peek motion
    /// uses — expand a directory, or collapse one.
    pub fn toggle_selected(&mut self) -> Option<String> {
        let row = self.selected_row()?;
        if row.kind != TreeNodeKind::Directory {
            return None;
        }
        if self.expanded.contains(&row.path) {
            self.expanded.remove(&row.path);
        } else {
            self.expanded.insert(row.path.clone());
        }
        Some(row.path)
    }

    /// Decide what `l`/right/Enter does with the current selection.
    ///
    /// A directory whose children are still inside the listing window is
    /// expanded in place and the cursor moves into it. A directory at the
    /// window's edge (relative depth 2) cannot show its children, so the
    /// scope slides to its parent and the directory is expanded there.
    ///
    /// A file is not a directory: there is nothing to peek into.
    pub fn peek_into_selected(&self) -> PeekStep {
        let Some(row) = self.selected_row() else {
            return PeekStep::None;
        };
        self.peek_into(&row.path)
    }

    /// Decide what `l`/right/Enter does with `path`.
    ///
    /// The decision is taken over the whole listing, not the filtered view:
    /// the filter decides what the pane *shows*, and a path the filter
    /// happens to hide is still a directory the daemon listed.
    pub fn peek_into(&self, path: &str) -> PeekStep {
        let Some(row) = self.all_rows().into_iter().find(|row| row.path == path) else {
            return PeekStep::None;
        };
        if row.kind != TreeNodeKind::Directory {
            return PeekStep::None;
        }
        if self.relative_depth(&row) >= 2 {
            return PeekStep::Slide {
                scope: parent_path(&row.path),
                directory: row.path,
            };
        }
        PeekStep::Enter {
            directory: row.path,
        }
    }

    /// Decide what `h`/left/Backspace does with the current selection.
    ///
    /// The cursor inside a directory moves to its parent row and that
    /// directory collapses. The cursor on a top-level row slides the scope
    /// up, with the old scope root expanded in the new scope. At the
    /// workspace root there is no scope to slide to, so the selected
    /// directory collapses in place — never a dead key.
    pub fn peek_up(&self) -> PeekUp {
        let rows = self.rows();
        let Some(index) = self.selected_index() else {
            return PeekUp::None;
        };
        let Some(row) = rows.get(index) else {
            return PeekUp::None;
        };
        if self.relative_depth(row) > 0 {
            let Some(parent) = rows[..index].iter().rev().find(|c| c.depth < row.depth) else {
                return PeekUp::None;
            };
            return PeekUp::Leave {
                parent: parent.path.clone(),
            };
        }
        let scope_root = self.root_prefix().to_string();
        if !scope_root.is_empty() {
            return PeekUp::Slide {
                scope: parent_path(&scope_root),
                root: scope_root,
            };
        }
        if row.kind == TreeNodeKind::Directory && row.expanded {
            return PeekUp::Collapse {
                directory: row.path.clone(),
            };
        }
        PeekUp::None
    }

    /// Expand `directory` and move the cursor into it.
    ///
    /// The cursor lands on the directory's first child, which is only known
    /// once the directory is expanded and the rows are re-read.
    pub fn expand_and_enter(&mut self, directory: &str) {
        self.expanded.insert(directory.to_string());
        self.enter_directory(directory);
    }

    /// Move the cursor into `directory`: onto its first child when one is
    /// visible, otherwise onto the directory itself.
    ///
    /// A directory that is not a row leaves the cursor alone: pointing the
    /// selection at a path nothing shows would leave the pane with no
    /// highlighted row at all.
    pub fn enter_directory(&mut self, directory: &str) {
        let rows = self.rows();
        let Some(index) = rows.iter().position(|row| row.path == directory) else {
            return;
        };
        match rows.get(index + 1) {
            Some(next) if next.depth > rows[index].depth => {
                self.selected = Some(next.path.clone());
            }
            _ => self.selected = Some(directory.to_string()),
        }
    }

    /// Move the cursor to the parent row and collapse that directory.
    pub fn leave_into(&mut self, parent: &str) {
        self.expanded.remove(parent);
        self.selected = Some(parent.to_string());
    }

    /// Collapse `directory` in place, leaving the cursor where it is.
    pub fn collapse(&mut self, directory: &str) {
        self.expanded.remove(directory);
    }

    /// Expand `root` in the current scope and put the cursor on it.
    ///
    /// The scope slide: the old scope root becomes a top-level row of the
    /// new scope, expanded, with the cursor on it.
    pub fn expand_and_select(&mut self, root: &str) {
        self.expanded.insert(root.to_string());
        self.selected = Some(root.to_string());
    }

    /// Ask for the cursor to enter `directory` once the relist lands.
    ///
    /// A scope slide replaces the listing, so the row the cursor belongs on
    /// is not known until the daemon answers.
    pub fn set_pending_cursor(&mut self, directory: &str) {
        self.pending_cursor = Some(directory.to_string());
    }

    /// Take the pending cursor target, if any.
    pub fn take_pending_cursor(&mut self) -> Option<String> {
        self.pending_cursor.take()
    }

    /// Record that the keyboard must move to the editor once the open of
    /// `path` completes.
    pub fn set_pending_editor_handoff(&mut self, path: &str) {
        self.pending_editor_handoff = Some(path.to_string());
    }

    /// Take the pending editor handoff, if any.
    pub fn take_pending_editor_handoff(&mut self) -> Option<String> {
        self.pending_editor_handoff.take()
    }

    /// Step the pane's scope back one level through the recorded history.
    ///
    /// This is the explicit "back" affordance the scope chain makes
    /// possible: the cursor lands on the directory just left behind, so
    /// stepping out and back in returns to the same place.
    ///
    /// It is **not** the `h`/left-arrow motion any more — that is
    /// [`Self::peek_up`], which slides the scope to the *parent* of the
    /// current scope and expands the old scope root there. The two answer
    /// different questions: this one walks back the way the pane came, the
    /// other walks one level shallower.
    ///
    /// `false` means the pane is already showing the workspace root, where
    /// "one level up" does not exist.
    pub fn ascend_root(&mut self) -> bool {
        if self.root_history.len() < 2 {
            return false;
        }
        let left = self
            .root_history
            .pop()
            .expect("length checked immediately above");
        self.reset_scope();
        // The directory we came out of is a row again in the new root; if the
        // daemon reports it gone (it was removed between two listings) the
        // listing pass falls back to the first row rather than stranding the
        // cursor on a path that is no longer on screen.
        self.selected = Some(left);
        true
    }

    /// Point the pane at `prefix` as its root, remembering the way back.
    ///
    /// Returns `false` when that is already the root, so the caller can skip
    /// a round-trip rather than relist the same directory.
    ///
    /// Ancestry is preserved when the move stays inside the current root (a
    /// jump to a descendant leaves `h` able to walk back out along the way it
    /// came) and reset when it does not, because a history claiming the pane
    /// arrived from a directory it never listed would make `h` go somewhere
    /// the user cannot predict.
    pub fn set_root_prefix(&mut self, prefix: &str) -> bool {
        let prefix = prefix.trim_matches('/').to_string();
        if prefix == self.root_prefix() {
            return false;
        }
        if !prefix.starts_with(&format!("{}/", self.root_prefix())) {
            // Re-seeded with the workspace root before the new entry: a chain
            // that starts at `src` has no way back out of it, which is how
            // `h` silently became a no-op after the first `l`.
            self.root_history.clear();
            self.root_history.push(String::new());
        }
        self.root_history.push(prefix);
        self.reset_scope();
        true
    }

    /// The directory this pane is listing: `""` for the workspace root.
    pub fn root_prefix(&self) -> &str {
        self.root_history.last().map(String::as_str).unwrap_or("")
    }

    /// How deep to *display* `row`.
    ///
    /// **Relative to the scope.** A walk of a subtree counts depth from the
    /// subtree (`crate::directory_listing::walk_directory` seeds the walk
    /// with the subtree as its base), so the top level of a listing scoped
    /// with `l` arrives at exactly the depth the workspace root's own top
    /// level arrives at. The indent is therefore the row's own depth: the
    /// scope's position is carried by the scope path, which the pane already
    /// shows, and the guide keeps describing the *current* view.
    ///
    /// This is also what makes a deep scope usable. The scope can slide
    /// arbitrarily far down — the listing window is three levels, but the
    /// scope is unbounded — and the old absolute depth grew with it until
    /// the clamp flattened exactly the deep scopes the peek motion exists to
    /// reach. The relative depth cannot grow: it is bounded by the window.
    ///
    /// Display arithmetic over an unchanged listing: it reads the row and
    /// clamps, and touches nothing else. Clamping to
    /// [`MAX_INDENT_DEPTH`] keeps a name from being pushed off a narrow
    /// column by a guide the column cannot hold.
    pub fn display_depth(&self, row: &TreeRow) -> usize {
        row.depth.min(MAX_INDENT_DEPTH)
    }

    /// How deep `row` sits **relative to the current scope**.
    ///
    /// The scope's own children are relative depth 0, their children 1, and
    /// their children 2 — the three levels the daemon's listing window
    /// carries. A row at relative depth 2 is the last one whose children
    /// are still inside the window.
    pub fn relative_depth(&self, row: &TreeRow) -> usize {
        row.depth.saturating_sub(1)
    }

    /// How many directory components the current scope sits below the
    /// workspace root: `0` at the root, `1` one `l` deep, and so on.
    pub fn scope_depth(&self) -> usize {
        self.root_prefix()
            .split('/')
            .filter(|part| !part.is_empty())
            .count()
    }

    /// How one visible row should be indented.
    ///
    /// Depth and last-ness travel together because both renderers need them
    /// together and neither may compute them alone: the `/files` box and the
    /// Ctrl+T pane once each derived `"  ".repeat(row.depth.min(8))` locally,
    /// which is how they came to disagree about what "one level in" looked
    /// like. `rows` is the caller's visible-row slice and `index` addresses it.
    pub fn indent_for(&self, rows: &[TreeRow], index: usize) -> TreeIndent {
        let Some(row) = rows.get(index) else {
            return TreeIndent::default();
        };
        TreeIndent {
            depth: self.display_depth(row),
            // The rows are a pre-order flattening, so a row's *next* row is
            // what answers this: deeper means the row has children still to
            // come, same-or-shallower means this was the last entry of the
            // directory the row sits in. The end of the listing is last too.
            //
            // Only the next row, never the rest of the slice: "every row after
            // me is deeper" holds for a whole chain of parents, so testing all
            // of them would close every level at once.
            last: rows
                .get(index + 1)
                .is_none_or(|next| next.depth <= row.depth),
        }
    }

    /// True when the pane is showing the workspace root rather than a
    /// directory inside it.
    pub fn at_workspace_root(&self) -> bool {
        self.root_prefix().is_empty()
    }

    /// Drop everything that describes the *contents* of the current scope.
    ///
    /// Expansions are workspace-relative so they stay valid across a relist,
    /// but a subtree listing re-walks from a different base and a cursor
    /// pointing into rows that are about to be replaced would resolve to
    /// whatever happens to sort first. An empty pane reading `Loading…` beats
    /// a populated one showing the directory the user just walked out of.
    fn reset_scope(&mut self) {
        self.nodes.clear();
        self.expanded.clear();
        self.selected = None;
        self.scroll = 0;
        self.truncated = None;
        self.pending_cursor = None;
    }

    /// Go back one level *within* the current scope.
    ///
    /// The in-scope half of the `h` motion ([`Self::peek_up`]): the cursor
    /// inside a directory moves to its parent row and that directory
    /// collapses; at the workspace root there is no scope to slide to, so
    /// the selected directory collapses in place.
    ///
    /// A top-level row in a scope that *can* slide is not handled here —
    /// that is the scope slide, which the command layer owns because it
    /// has to issue the daemon request.
    pub fn leave_selected(&mut self) -> Option<String> {
        match self.peek_up() {
            PeekUp::Leave { parent } => {
                self.leave_into(&parent);
                Some(parent)
            }
            PeekUp::Collapse { directory } => {
                self.collapse(&directory);
                Some(directory)
            }
            PeekUp::Slide { .. } | PeekUp::None => None,
        }
    }

    /// Apply a completed walk, if it belongs to the current generation, root,
    /// and scope.
    ///
    /// A stale completion is dropped rather than applied, which is what keeps
    /// a tree from rendering against a workspace the user has left. The
    /// daemon echoes the prefix it answered for, so a listing that came back
    /// for a *different* subtree is refused on the same grounds even when its
    /// generation happens to match — `l` twice in quick succession issues two
    /// requests, and only the scope identifies which one answered.
    pub fn apply_listing(
        &mut self,
        generation: u64,
        request_id: u64,
        root: PathBuf,
        listing: TreeListing,
    ) -> bool {
        if generation != self.generation
            || self.root.as_ref() != Some(&root)
            || !self.request.finish(request_id)
        {
            return false;
        }
        if listing.prefix != self.listing_prefix {
            // The request id is spent, so this completion can never be
            // re-applied: drop it and let the caller drive the next request.
            return false;
        }
        self.nodes = listing.nodes;
        self.truncated = listing.truncated_reason;
        self.status = FileTreeStatus::Ready;
        // Drop expansions for directories that no longer exist so a rebuild
        // cannot leave the tree holding a path it will never match again.
        let present = collect_directory_paths(&self.nodes);
        self.expanded.retain(|path| present.contains(path));
        let rows = self.rows();
        if self
            .selected
            .as_deref()
            .is_none_or(|s| !rows.iter().any(|row| row.path == s))
        {
            self.selected = rows.first().map(|row| row.path.clone());
        }
        // A scope slide knows where the cursor goes only once the relist
        // lands: the directory the user entered is a row of the *new*
        // scope, and its first child is what the cursor belongs on.
        if let Some(target) = self.take_pending_cursor() {
            self.enter_directory(&target);
        }
        self.reveal_selected(self.selected_index().unwrap_or(0), rows.len());
        true
    }

    /// Record a failed walk for the current generation and root.
    pub fn apply_error(
        &mut self,
        generation: u64,
        request_id: u64,
        root: &PathBuf,
        message: String,
    ) -> bool {
        if generation != self.generation
            || self.root.as_ref() != Some(root)
            || !self.request.finish(request_id)
        {
            return false;
        }
        self.nodes.clear();
        self.selected = None;
        self.expanded.clear();
        self.truncated = None;
        self.status = FileTreeStatus::Error(message);
        true
    }

    /// Begin a rebuild against the current root and current scope.
    ///
    /// The generation bump is what invalidates any walk already in flight,
    /// and the scope is captured here so an arriving completion can be
    /// checked against what was actually asked for. The previous walk's
    /// cooperative flag is set first, so a superseded blocking scan exits at
    /// its next entry rather than running to the examined-entry cap.
    pub fn begin_rebuild(&mut self, root: PathBuf) -> (u64, u64) {
        self.cancel_in_flight();
        if self.root.as_deref() != Some(root.as_path()) {
            // A new workspace resets everything: a path from the previous
            // project is meaningless in the next.
            self.root_history.clear();
            self.reset_scope();
        }
        if self.root_history.is_empty() {
            self.root_history.push(String::new());
        }
        self.root = Some(root);
        self.listing_prefix = self.root_prefix().to_string();
        self.generation = self.generation.wrapping_add(1);
        self.status = FileTreeStatus::Loading;
        self.truncated = None;
        self.cancel_flag = Some(Arc::new(AtomicBool::new(false)));
        let request_id = self.request.begin();
        (self.generation, request_id)
    }

    /// The cooperative flag for the walk started by [`Self::begin_rebuild`].
    ///
    /// Moved into the `spawn_blocking` task by the command layer. `None`
    /// when no rebuild has begun (the command layer then refuses to walk).
    pub fn take_cancel_flag(&self) -> Option<Arc<AtomicBool>> {
        self.cancel_flag.clone()
    }

    /// Signal any in-flight blocking walk to stop at its next entry.
    ///
    /// Idempotent: safe to call when nothing is walking. Called by
    /// `begin_rebuild`, `reset_for_project`, and TUI shutdown paths that
    /// drop file-tree work.
    pub fn cancel_in_flight(&mut self) {
        if let Some(flag) = self.cancel_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.cancel_flag = None;
    }

    /// Reset for a project switch.
    pub fn reset_for_project(&mut self) {
        self.cancel_in_flight();
        self.generation = self.generation.wrapping_add(1);
        self.root = None;
        self.reset_scope();
        self.root_history.clear();
        self.listing_prefix.clear();
        self.status = FileTreeStatus::Idle;
    }
}

/// The parent of a workspace-relative path.
///
/// `"src/tui"` → `"src"`, `"src"` → `""` (the workspace root). The scope
/// slides along this relation, and the listing window is what decides when
/// a slide is needed.
fn parent_path(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

/// Whether `row` survives `needle`, which is already lowercased.
///
/// The name and the path are both matched: the name is what the user
/// remembers, and the path is what disambiguates two rows with the same
/// name in different directories.
fn matches_filter(row: &TreeRow, needle: &str) -> bool {
    row.name.to_lowercase().contains(needle) || row.path.to_lowercase().contains(needle)
}

fn collect_directory_paths(nodes: &[crate::tui::file_tree::TreeNode]) -> BTreeSet<String> {
    fn recurse(nodes: &[crate::tui::file_tree::TreeNode], out: &mut BTreeSet<String>) {
        for node in nodes {
            if node.kind == TreeNodeKind::Directory {
                out.insert(node.path.clone());
            }
            if let Some(children) = node.children.as_deref() {
                recurse(children, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    recurse(nodes, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::file_tree::build_tree;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(root: &std::path::Path, relative: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, b"x").expect("write");
    }

    fn state_with(root: &std::path::Path) -> FileTreeState {
        let mut state = FileTreeState::default();
        let (generation, request_id) = state.begin_rebuild(root.to_path_buf());
        state.apply_listing(generation, request_id, root.to_path_buf(), build_tree(root));
        state
    }

    /// One node of a hand-built listing.
    ///
    /// The daemon's window is three levels deep, so a listing carries rows
    /// at depth 1, 2 and 3 — the depth a row would have *inside* the scope
    /// it was asked for.
    fn node(
        path: &str,
        depth: usize,
        kind: TreeNodeKind,
        children: Option<Vec<crate::tui::file_tree::TreeNode>>,
    ) -> crate::tui::file_tree::TreeNode {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        crate::tui::file_tree::TreeNode {
            name,
            path: path.to_string(),
            depth,
            kind,
            children,
        }
    }

    /// A state showing `nodes` for the workspace root.
    fn state_with_listing(
        root: &std::path::Path,
        nodes: Vec<crate::tui::file_tree::TreeNode>,
    ) -> FileTreeState {
        scoped_state(root, "", nodes)
    }

    /// A state showing `nodes` for `scope`.
    ///
    /// The scope is set *after* the first rebuild, because a rebuild for a
    /// new root resets the scope chain — the same order the command layer
    /// uses (`set_root_prefix` then `rebuild`).
    fn scoped_state(
        root: &std::path::Path,
        scope: &str,
        nodes: Vec<crate::tui::file_tree::TreeNode>,
    ) -> FileTreeState {
        let mut state = FileTreeState::default();
        let _ = state.begin_rebuild(root.to_path_buf());
        if !scope.is_empty() {
            state.set_root_prefix(scope);
        }
        let (generation, request_id) = state.begin_rebuild(root.to_path_buf());
        let listing = crate::tui::file_tree::TreeListing {
            prefix: scope.to_string(),
            nodes,
            truncated: false,
            truncated_reason: None,
            examined: 0,
        };
        assert!(
            state.apply_listing(generation, request_id, root.to_path_buf(), listing),
            "the listing belongs to the scope it was asked for"
        );
        state
    }

    /// The worked example from the peek design:
    ///
    /// ```text
    /// /workspace
    ///   src/
    ///     tui/
    ///       editor.rs
    ///       app/          <- the window's edge: its children are not here
    ///   Cargo.toml
    /// ```
    fn worked_example_listing() -> Vec<crate::tui::file_tree::TreeNode> {
        vec![
            node(
                "src",
                1,
                TreeNodeKind::Directory,
                Some(vec![node(
                    "src/tui",
                    2,
                    TreeNodeKind::Directory,
                    Some(vec![
                        node("src/tui/editor.rs", 3, TreeNodeKind::File, None),
                        node("src/tui/app", 3, TreeNodeKind::Directory, None),
                    ]),
                )]),
            ),
            node("Cargo.toml", 1, TreeNodeKind::File, None),
        ]
    }

    #[test]
    fn default_is_hidden_and_empty_so_m006a_surface_is_untouched() {
        let state = FileTreeState::default();
        assert!(!state.visible, "the pane must be hidden by default");
        assert!(state.rows().is_empty());
        assert!(state.selected.is_none());
        assert_eq!(state.status, FileTreeStatus::Idle);
    }

    #[test]
    fn a_listing_populates_rows_and_selects_the_first() {
        let root = temp();
        write(root.path(), "a/b.txt");
        write(root.path(), "z.txt");

        let state = state_with(root.path());
        assert_eq!(state.status, FileTreeStatus::Ready);
        assert_eq!(
            state
                .rows()
                .iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "z.txt"]
        );
        assert_eq!(state.selected.as_deref(), Some("a"));
    }

    #[test]
    fn selection_moves_and_clamps_at_both_ends() {
        let root = temp();
        write(root.path(), "b");
        write(root.path(), "c");
        // "a" is a directory so the walk sorts it ahead of the two files.
        std::fs::create_dir_all(root.path().join("a")).expect("mkdir");
        write(root.path(), "a/inner.txt");

        let mut state = state_with(root.path());
        assert_eq!(state.selected_index(), Some(0));
        state.move_selection(-1);
        assert_eq!(state.selected_index(), Some(0), "clamps at the top");
        state.move_selection(1);
        assert_eq!(state.selected.as_deref(), Some("b"));
        state.move_selection(1);
        assert_eq!(state.selected.as_deref(), Some("c"));
        state.move_selection(1);
        assert_eq!(state.selected.as_deref(), Some("c"), "clamps at the bottom");
    }

    #[test]
    fn toggle_expands_and_collapses_a_directory_but_ignores_a_file() {
        let root = temp();
        write(root.path(), "dir/inner.txt");
        write(root.path(), "file.txt");

        let mut state = state_with(root.path());
        assert_eq!(state.selected.as_deref(), Some("dir"));
        assert!(!state.rows().iter().any(|r| r.path == "dir/inner.txt"));

        assert_eq!(state.toggle_selected().as_deref(), Some("dir"));
        assert!(state.expanded.contains("dir"));
        let expanded = state.rows();
        // The directory row reports itself expanded; the child file becomes
        // visible but is not itself "expanded" — those are different things.
        assert!(expanded.iter().any(|r| r.path == "dir" && r.expanded));
        assert!(expanded.iter().any(|r| r.path == "dir/inner.txt"));
        assert!(!expanded
            .iter()
            .any(|r| r.path == "dir/inner.txt" && r.expanded));

        assert_eq!(state.toggle_selected().as_deref(), Some("dir"));
        assert!(!state.expanded.contains("dir"));
        assert!(!state.rows().iter().any(|r| r.path == "dir/inner.txt"));

        state.selected = Some("file.txt".to_string());
        assert_eq!(state.toggle_selected(), None, "a file is not expandable");
    }

    #[test]
    fn descending_makes_the_directory_the_pane_root_and_drops_the_old_rows() {
        let root = temp();
        write(root.path(), "dir/a.txt");
        write(root.path(), "dir/b.txt");
        write(root.path(), "top.txt");

        let mut state = state_with(root.path());
        assert_eq!(state.root_prefix(), "");
        assert_eq!(state.selected.as_deref(), Some("dir"));

        assert!(state.set_root_prefix("dir"));
        assert_eq!(state.root_prefix(), "dir");
        assert!(
            state.nodes.is_empty(),
            "a scope change must not leave the old listing on screen under the new title"
        );
        assert_eq!(
            state.selected, None,
            "the cursor means nothing in a new root"
        );
        assert!(
            state.expanded.is_empty(),
            "expansions are re-derived from the new base, not inherited"
        );

        // The daemon relists the subtree; the cursor lands on its first row.
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        assert_eq!(state.listing_prefix, "dir", "the walk was issued for it");
        state.apply_listing(
            generation,
            request_id,
            root.path().to_path_buf(),
            crate::tui::file_tree::build_tree(root.path()),
        );
        assert_eq!(
            state.root_prefix(),
            "dir",
            "a relist does not move the root"
        );
    }

    #[test]
    fn descending_to_the_current_root_does_nothing_and_says_so() {
        let root = temp();
        write(root.path(), "dir/a.txt");

        let mut state = state_with(root.path());
        assert!(
            !state.set_root_prefix(""),
            "the workspace root is already the root; the caller must not relist"
        );
        assert!(state.set_root_prefix("dir"));
        assert!(
            !state.set_root_prefix("dir"),
            "`l` on the root you are already listing is a no-op, not a round-trip"
        );
        assert_eq!(state.root_prefix(), "dir");
    }

    #[test]
    fn descending_into_an_empty_directory_is_still_a_scope_change() {
        let root = temp();
        std::fs::create_dir_all(root.path().join("empty")).expect("mkdir");

        let mut state = state_with(root.path());
        assert_eq!(state.selected.as_deref(), Some("empty"));
        assert!(state.set_root_prefix("empty"));
        assert_eq!(state.root_prefix(), "empty");
        assert!(
            state.selected.is_none(),
            "an empty root has no rows to select; the pane shows an empty listing, not a stranded cursor"
        );
    }

    #[test]
    fn ascending_pops_one_level_and_lands_the_cursor_where_it_came_from() {
        let root = temp();
        write(root.path(), "a/b/c/deep.txt");

        let mut state = state_with(root.path());
        assert!(state.set_root_prefix("a"));
        assert!(state.set_root_prefix("a/b"));
        assert!(state.set_root_prefix("a/b/c"));
        assert_eq!(state.root_prefix(), "a/b/c");

        // Back out: the root is the parent and the cursor is the directory
        // just left behind, which is a row again in the new listing.
        assert!(state.ascend_root());
        assert_eq!(state.root_prefix(), "a/b");
        assert_eq!(state.selected.as_deref(), Some("a/b/c"));

        assert!(state.ascend_root());
        assert_eq!(state.root_prefix(), "a");
        assert_eq!(state.selected.as_deref(), Some("a/b"));

        assert!(state.ascend_root());
        assert_eq!(state.root_prefix(), "");
        assert_eq!(state.selected.as_deref(), Some("a"));

        // At the workspace root there is no level to leave, and `h` must not
        // be a key that quietly does nothing at the one depth every listing
        // starts at — the command falls back to the in-scope verb.
        assert!(!state.ascend_root(), "the workspace root has no parent");
    }

    #[test]
    fn ancestry_is_kept_inside_the_current_root_and_reset_outside_it() {
        let root = temp();
        write(root.path(), "a/b/c.txt");

        let mut state = state_with(root.path());
        assert!(state.set_root_prefix("a"));
        // A jump that skips a level is still a descent from `a`, so `h` can
        // walk back out through `b` on the way it came.
        assert!(state.set_root_prefix("a/b/c"));
        assert_eq!(state.root_history, vec!["", "a", "a/b/c"]);

        // Leaving the current root drops the way back: `h` would otherwise
        // walk out through a directory the pane never listed.
        assert!(state.set_root_prefix("other"));
        assert_eq!(state.root_history, vec!["", "other"]);
        assert_eq!(state.root_prefix(), "other");
    }

    #[test]
    fn a_listing_for_another_scope_is_refused_even_at_the_current_generation() {
        let root = temp();
        write(root.path(), "a/inner.txt");

        let mut state = state_with(root.path());
        assert!(state.set_root_prefix("a"));
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        let mut listing = crate::tui::file_tree::build_tree(root.path());
        // A completion that came back for a different subtree than the one
        // requested: `l` twice in quick succession produces exactly this,
        // and only the echoed prefix says which one answered.
        listing.prefix = "a/inner".to_string();

        assert!(
            !state.apply_listing(generation, request_id, root.path().to_path_buf(), listing),
            "a mismatched scope must be dropped rather than painted"
        );
        assert!(state.nodes.is_empty());
        assert_eq!(state.listing_prefix, "a");
    }

    #[test]
    fn leave_collapses_an_open_directory_in_place() {
        let root = temp();
        write(root.path(), "dir/a.txt");

        let mut state = state_with(root.path());
        state.expanded.insert("dir".to_string());
        assert_eq!(state.selected.as_deref(), Some("dir"));

        // At the workspace root there is no scope to slide to, so `h`
        // collapses the selected directory where it stands.
        assert_eq!(state.leave_selected().as_deref(), Some("dir"));
        assert!(!state.expanded.contains("dir"));
        assert_eq!(state.selected.as_deref(), Some("dir"));
    }

    #[test]
    fn leave_steps_out_to_the_parent_directory_and_collapses_it() {
        let root = temp();
        write(root.path(), "dir/nested/deep.txt");

        let mut state = state_with(root.path());
        state.expanded.insert("dir".to_string());
        state.expanded.insert("dir/nested".to_string());
        state.selected = Some("dir/nested/deep.txt".to_string());

        // From a nested file, `h` moves to the parent row and collapses the
        // directory it is leaving.
        assert_eq!(state.leave_selected().as_deref(), Some("dir/nested"));
        assert_eq!(state.selected.as_deref(), Some("dir/nested"));
        assert!(
            !state.expanded.contains("dir/nested"),
            "the directory being left is collapsed"
        );

        // One more step leaves `dir` the same way.
        assert_eq!(state.leave_selected().as_deref(), Some("dir"));
        assert_eq!(state.selected.as_deref(), Some("dir"));
        assert!(!state.expanded.contains("dir"));
    }

    #[test]
    fn leave_at_the_workspace_root_is_a_no_op_rather_than_an_error() {
        let root = temp();
        write(root.path(), "a/b.txt");
        write(root.path(), "top.txt");

        let mut state = state_with(root.path());
        assert_eq!(state.selected.as_deref(), Some("a"));
        // A top-level directory that is not expanded has no parent to go to.
        assert_eq!(state.leave_selected(), None);
        assert_eq!(state.selected.as_deref(), Some("a"), "selection survives");

        state.selected = Some("top.txt".to_string());
        assert_eq!(state.leave_selected(), None);
        assert_eq!(state.selected.as_deref(), Some("top.txt"));
    }

    /// The scroll offset is a **window** offset, not a row count.
    ///
    /// The old clamp was `total - 1`, which is the last row of the listing:
    /// a page verb that left the cursor there scrolled the whole listing off
    /// the page and showed one row at the bottom of the pane.
    #[test]
    fn the_scroll_offset_never_leaves_the_measured_window() {
        let root = temp();
        for index in 0..40 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut state = state_with(root.path());
        state.viewport_rows = 10;
        let total = state.rows().len();

        for _ in 0..10 {
            state.page_selection(1);
            assert!(
                state.scroll <= total - state.viewport_height(),
                "the offset may never exceed total - viewport, got {}",
                state.scroll
            );
            let index = state.selected_index().expect("a row is selected");
            assert!(
                index >= state.scroll && index < state.scroll + state.viewport_height(),
                "the cursor has to stay on screen, got {index} at {}",
                state.scroll
            );
        }
        assert_eq!(state.scroll, total - state.viewport_height());
    }

    /// A keypress can arrive before the first frame, when the pane has no
    /// measured height. The page verb falls back to a page size and still
    /// keeps the listing on the page.
    #[test]
    fn a_page_before_the_first_frame_uses_the_fallback_and_stays_on_the_page() {
        let root = temp();
        for index in 0..40 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut state = state_with(root.path());
        assert_eq!(state.viewport_rows, 0, "nothing has rendered yet");
        let total = state.rows().len();

        state.page_selection(1);
        assert!(
            state.selected_index().unwrap_or(0) > 0,
            "the page verb must not be a no-op before the first frame"
        );
        assert!(
            state.scroll <= total.saturating_sub(FALLBACK_PAGE_ROWS),
            "the fallback page still bounds the offset, got {}",
            state.scroll
        );
    }

    /// The filter narrows the rows the pane shows, on the name and on the
    /// path, and it does not care about case.
    #[test]
    fn the_filter_matches_the_name_and_the_path_case_insensitively() {
        let root = temp();
        write(root.path(), "Main.rs");
        write(root.path(), "docs/readme.md");

        let mut state = state_with(root.path());
        state.selected = Some("docs".to_string());
        state.toggle_selected();
        let paths = |state: &FileTreeState| {
            state
                .rows()
                .iter()
                .map(|row| row.path.clone())
                .collect::<Vec<String>>()
        };

        assert_eq!(
            paths(&state),
            vec![
                "docs".to_string(),
                "docs/readme.md".to_string(),
                "Main.rs".to_string()
            ],
            "no filter yet"
        );

        state.filter = "main".to_string();
        assert_eq!(
            paths(&state),
            vec!["Main.rs".to_string()],
            "the name, case-folded"
        );

        state.filter = "DOCS/".to_string();
        assert_eq!(
            paths(&state),
            vec!["docs/readme.md".to_string()],
            "the path, case-folded"
        );

        state.filter = "readme".to_string();
        assert_eq!(paths(&state), vec!["docs/readme.md".to_string()]);

        state.filter = "nothing-matches-this".to_string();
        assert!(state.rows().is_empty());
    }

    /// The filter hides rows, so the cursor has to follow: a selection the
    /// user cannot see is a selection they cannot reason about.
    #[test]
    fn the_cursor_follows_the_filter_onto_a_row_that_is_still_shown() {
        let root = temp();
        write(root.path(), "Main.rs");
        write(root.path(), "other.txt");

        let mut state = state_with(root.path());
        assert_eq!(state.selected.as_deref(), Some("Main.rs"));

        state.filter = "other".to_string();
        state.reanchor_selection();
        assert_eq!(
            state.selected.as_deref(),
            Some("other.txt"),
            "the cursor moves to a row the filter still shows"
        );
    }

    #[test]
    fn ensure_visible_scrolls_both_directions() {
        let mut state = FileTreeState::default();
        state.ensure_visible(5, 3);
        assert_eq!(state.scroll, 3, "scrolling down to reveal 5 in 3 rows");
        state.ensure_visible(1, 3);
        assert_eq!(state.scroll, 1, "scrolling up to reveal 1");
        state.ensure_visible(2, 0);
        assert_eq!(state.scroll, 1, "a zero-height viewport is a no-op");
    }

    #[test]
    fn a_stale_generation_completion_is_discarded() {
        let root = temp();
        write(root.path(), "a.txt");
        let mut state = FileTreeState::default();
        let (first, first_request) = state.begin_rebuild(root.path().to_path_buf());
        // A second rebuild supersedes the first.
        let (second, second_request) = state.begin_rebuild(root.path().to_path_buf());
        assert_ne!(first, second);

        assert!(
            !state.apply_listing(
                first,
                first_request,
                root.path().to_path_buf(),
                build_tree(root.path())
            ),
            "the older walk must be refused"
        );
        assert_eq!(state.status, FileTreeStatus::Loading);
        assert!(state.rows().is_empty(), "no nodes from a stale walk");
        // The current request is still applicable, proving the refusal above
        // was about staleness and not a permanently stuck state.
        assert!(state.apply_listing(
            second,
            second_request,
            root.path().to_path_buf(),
            build_tree(root.path())
        ));
        assert_eq!(state.status, FileTreeStatus::Ready);
    }

    #[test]
    fn a_completion_for_another_root_is_discarded() {
        let first_root = temp();
        let second_root = temp();
        write(first_root.path(), "from-first.txt");
        write(second_root.path(), "from-second.txt");

        let mut state = FileTreeState::default();
        let (generation, request_id) = state.begin_rebuild(second_root.path().to_path_buf());
        assert!(
            !state.apply_listing(
                generation,
                request_id,
                first_root.path().to_path_buf(),
                build_tree(first_root.path())
            ),
            "a walk for a different workspace must be refused"
        );
        assert!(state.apply_listing(
            generation,
            request_id,
            second_root.path().to_path_buf(),
            build_tree(second_root.path())
        ));
        assert!(state.rows().iter().any(|r| r.name == "from-second.txt"));
    }

    #[test]
    fn switching_projects_resets_selection_expansion_and_root() {
        let first = temp();
        let second = temp();
        write(first.path(), "a/b.txt");
        write(second.path(), "c.txt");

        let mut state = state_with(first.path());
        state.toggle_selected();
        assert!(state.expanded.contains("a"));
        assert!(state.set_root_prefix("a"));
        assert_eq!(state.root_prefix(), "a");

        let (generation, request_id) = state.begin_rebuild(second.path().to_path_buf());
        state.apply_listing(
            generation,
            request_id,
            second.path().to_path_buf(),
            build_tree(second.path()),
        );
        assert!(
            state.expanded.is_empty(),
            "a path from the previous project must not survive"
        );
        assert_eq!(state.selected.as_deref(), Some("c.txt"));
        assert!(
            state.at_workspace_root(),
            "a directory from the previous project is meaningless in the next"
        );
    }

    #[test]
    fn descending_from_a_never_rebuilt_pane_can_still_walk_back_out() {
        // `Default` has an empty root history, so `root_prefix()` reads as the
        // workspace root without one having been pushed. The first `l` must
        // still leave a chain that `h` can pop — otherwise the very first
        // descent in a pane is one-way.
        let mut state = FileTreeState::default();
        assert!(state.at_workspace_root());
        assert!(state.set_root_prefix("src"));
        assert_eq!(state.root_history, vec!["", "src"]);
        assert!(state.ascend_root());
        assert_eq!(state.root_prefix(), "");
        assert!(!state.ascend_root());
    }

    #[test]
    fn paging_moves_the_cursor_and_the_scroll_together() {
        let root = temp();
        for index in 0..40 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut state = state_with(root.path());
        state.viewport_rows = 10;
        assert_eq!(state.page_step(), 6, "two thirds of a ten-row pane");

        assert_eq!(state.selected_index(), Some(0));
        assert_eq!(state.scroll, 0);

        state.page_selection(1);
        assert_eq!(state.selected_index(), Some(6));
        assert_eq!(
            state.scroll, 0,
            "the first page still fits without scrolling"
        );

        state.page_selection(1);
        assert_eq!(state.selected_index(), Some(12));
        assert_eq!(
            state.scroll, 3,
            "the listing scrolls to keep the cursor on screen"
        );

        // Paging back is the same minimal scroll in reverse: row 6 is still
        // inside the rows on screen, so the listing holds still instead of
        // jumping a whole page for a cursor that never left it.
        state.page_selection(-1);
        assert_eq!(state.selected_index(), Some(6));
        assert_eq!(state.scroll, 3);

        state.page_selection(-1);
        assert_eq!(state.selected_index(), Some(0));
        assert_eq!(state.scroll, 0);

        // Clamped at the top rather than wrapping or panicking.
        state.page_selection(-1);
        assert_eq!(state.selected_index(), Some(0));
    }

    #[test]
    fn paging_without_a_measured_viewport_still_moves() {
        let root = temp();
        for index in 0..30 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut state = state_with(root.path());
        assert_eq!(state.viewport_rows, 0, "nothing has rendered yet");
        assert_eq!(state.page_step(), FALLBACK_PAGE_ROWS as isize * 2 / 3);

        // A keypress can arrive before the first frame; a page verb that is a
        // no-op there is worse than an approximate step.
        state.page_selection(1);
        assert!(state.selected_index().unwrap_or(0) > 0);
    }

    #[test]
    fn paging_an_empty_or_short_listing_is_harmless() {
        let root = temp();
        let mut empty = FileTreeState::default();
        empty.page_selection(1);
        empty.page_selection(-1);

        write(root.path(), "only.txt");
        let mut short = state_with(root.path());
        short.viewport_rows = 40;
        short.page_selection(1);
        assert_eq!(short.selected_index(), Some(0));
        short.page_selection(-1);
        assert_eq!(short.selected_index(), Some(0));
    }

    #[test]
    fn rebuild_drops_expansions_for_directories_that_disappeared() {
        let root = temp();
        write(root.path(), "gone/inner.txt");
        let mut state = state_with(root.path());
        state.toggle_selected();
        assert!(state.expanded.contains("gone"));

        std::fs::remove_dir_all(root.path().join("gone")).expect("rm");
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        state.apply_listing(
            generation,
            request_id,
            root.path().to_path_buf(),
            build_tree(root.path()),
        );
        assert!(state.expanded.is_empty());
    }

    #[test]
    fn reset_for_project_invalidates_an_in_flight_walk() {
        let root = temp();
        write(root.path(), "a.txt");
        let mut state = FileTreeState::default();
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        state.reset_for_project();
        assert!(
            !state.apply_listing(
                generation,
                request_id,
                root.path().to_path_buf(),
                build_tree(root.path())
            ),
            "a walk from before the switch must be refused"
        );
        assert_eq!(state.status, FileTreeStatus::Idle);
    }

    #[test]
    fn an_error_is_shown_and_clears_the_listing() {
        let root = temp();
        write(root.path(), "a.txt");
        let mut state = state_with(root.path());
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        assert!(state.apply_error(
            generation,
            request_id,
            &root.path().to_path_buf(),
            "workspace root is unreadable".to_string()
        ));
        assert!(matches!(state.status, FileTreeStatus::Error(_)));
        assert!(state.rows().is_empty());
        assert!(state.selected.is_none());
    }

    #[test]
    fn a_rebuild_cooperatively_cancels_the_previous_walk() {
        use std::sync::atomic::Ordering;
        let first = temp();
        let second = temp();
        let mut state = FileTreeState::default();
        let _ = state.begin_rebuild(first.path().to_path_buf());
        let previous = state.take_cancel_flag().expect("flag exists");
        assert!(!previous.load(Ordering::Relaxed));

        // A second rebuild supersedes the first: the old flag is set, so a
        // blocking scan holding it exits at its next entry.
        let _ = state.begin_rebuild(second.path().to_path_buf());
        assert!(
            previous.load(Ordering::Relaxed),
            "the superseded walk must be signalled"
        );
        assert!(state.take_cancel_flag().is_some());
    }

    #[test]
    fn reset_for_project_cancels_an_in_flight_walk() {
        use std::sync::atomic::Ordering;
        let root = temp();
        let mut state = FileTreeState::default();
        let _ = state.begin_rebuild(root.path().to_path_buf());
        let flag = state.take_cancel_flag().expect("flag exists");
        state.reset_for_project();
        assert!(flag.load(Ordering::Relaxed));
        assert!(state.take_cancel_flag().is_none());
    }

    #[test]
    fn a_hundred_rapid_rebuilds_leave_one_generation_and_no_stranded_flag() {
        let root = temp();
        let mut state = FileTreeState::default();
        let mut flags = Vec::new();
        for _ in 0..100 {
            let _ = state.begin_rebuild(root.path().to_path_buf());
            flags.push(state.take_cancel_flag().expect("flag exists"));
        }
        // Every superseded flag was signalled; only the latest is live.
        use std::sync::atomic::Ordering;
        let (superseded, latest) = flags.split_at(100 - 1);
        assert!(superseded.iter().all(|f| f.load(Ordering::Relaxed)));
        assert!(latest.iter().all(|f| !f.load(Ordering::Relaxed)));
        assert_eq!(state.generation, 100);
    }

    #[test]
    fn truncation_notice_survives_into_state() {
        let root = temp();
        // Exceed the node budget.
        for i in 0..(crate::tui::file_tree::MAX_TREE_NODES + 10) {
            write(root.path(), &format!("f{i:06}.txt"));
        }
        let state = state_with(root.path());
        assert!(
            state.truncated.is_some(),
            "a truncated walk must carry its notice into the view state"
        );
    }

    /// The indent is **relative to the scope**.
    ///
    /// A walk of a subtree counts depth from the subtree, so a scoped
    /// listing's top level arrives at the depth the workspace root's own top
    /// level arrives at. Adding the scope's components back made the
    /// absolute depth grow with every scope slide, and the clamp flattened
    /// exactly the deep scopes the peek motion exists to reach. The scope's
    /// position is carried by the scope path instead.
    #[test]
    fn display_depth_is_relative_to_the_scope() {
        let root = temp();
        write(root.path(), "a/b/c/deep.txt");
        let state = state_with(root.path());
        let row = |depth: usize| TreeRow {
            path: "x".to_string(),
            name: "x".to_string(),
            depth,
            kind: TreeNodeKind::File,
            expanded: false,
        };

        assert_eq!(state.scope_depth(), 0, "the root scope has no components");
        assert_eq!(state.display_depth(&row(0)), 0);
        assert_eq!(state.display_depth(&row(3)), 3);

        let mut scoped = FileTreeState::default();
        assert!(scoped.set_root_prefix("a"));
        assert_eq!(scoped.scope_depth(), 1);
        assert_eq!(
            scoped.display_depth(&row(1)),
            1,
            "a scoped listing's top level draws at the top level"
        );
        assert_eq!(scoped.display_depth(&row(3)), 3);

        assert!(scoped.set_root_prefix("a/b"));
        assert_eq!(scoped.scope_depth(), 2);
        assert_eq!(
            scoped.display_depth(&row(1)),
            1,
            "the scope is carried by the scope path, not by the guide"
        );
    }

    /// Past the cap the answer stops growing. A name is content and a guide is
    /// orientation, so the guide is what runs out of room first — silently
    /// pushing the name off a narrow column is the failure this prevents.
    ///
    /// The cap is a *render* bound now: the relative depth cannot grow past
    /// the listing window, so it only ever bites on a column too narrow for
    /// the guide.
    #[test]
    fn display_depth_clamps_at_the_one_cap_both_renderers_share() {
        let mut state = FileTreeState::default();
        assert!(state.set_root_prefix("a/b/c/d/e/f/g/h/i/j/k"));
        let deep = TreeRow {
            path: "x".to_string(),
            name: "x".to_string(),
            depth: 9,
            kind: TreeNodeKind::File,
            expanded: false,
        };
        assert_eq!(state.display_depth(&deep), MAX_INDENT_DEPTH);
    }

    /// Depth is display arithmetic and must stay that way: reading it on every
    /// row may not move the cursor, open a directory, or change what the walk
    /// would hand back. A hierarchy fix that reached into navigation would make
    /// "visually deeper" and "reachable differently" the same change.
    ///
    /// Under the peek motion the two are *deliberately* different: the guide
    /// is relative to the scope, and reaching a deeper row is a navigation
    /// decision (`l`), never a consequence of where the row is drawn.
    #[test]
    fn display_depth_is_a_read_and_changes_nothing_about_navigation() {
        let root = temp();
        write(root.path(), "dir/nested/a.txt");
        write(root.path(), "top.txt");

        let mut state = state_with(root.path());
        state.selected = Some("dir".to_string());
        assert!(
            state.toggle_selected().is_some(),
            "expanding a directory adds its children as rows"
        );
        let before_rows = state.rows();
        let before_selected = state.selected.clone();
        let before_expanded = state.expanded.clone();
        let before_scroll = state.scroll;
        assert!(before_rows.len() > 2, "expansion added a child row");

        let depths: Vec<usize> = before_rows
            .iter()
            .map(|row| state.display_depth(row))
            .collect();
        assert!(depths.iter().any(|depth| *depth > 0), "something is nested");

        assert_eq!(state.rows(), before_rows, "rows must be untouched");
        assert_eq!(state.selected, before_selected, "the cursor must not move");
        assert_eq!(state.expanded, before_expanded, "expansion must not change");
        assert_eq!(state.scroll, before_scroll, "the viewport must not move");
    }

    /// `l` on a directory whose children are still inside the listing
    /// window: expand it in place and move the cursor into it.
    #[test]
    fn peeking_into_a_directory_expands_it_in_place_and_enters_it() {
        let root = temp();
        let mut state = state_with_listing(root.path(), worked_example_listing());
        state.selected = Some("src".to_string());

        assert_eq!(
            state.peek_into_selected(),
            PeekStep::Enter {
                directory: "src".to_string()
            }
        );

        state.expand_and_enter("src");
        assert!(state.expanded.contains("src"));
        assert_eq!(
            state.selected.as_deref(),
            Some("src/tui"),
            "the cursor moves into the directory, onto its first child"
        );
        assert_eq!(
            state.root_prefix(),
            "",
            "the scope does not move when the children are inside the window"
        );
    }

    /// `l` on a directory at the window's edge: its children cannot be
    /// fetched, so the scope slides to its parent and the directory is
    /// expanded there.
    #[test]
    fn peeking_into_a_directory_at_the_window_edge_slides_the_scope() {
        let root = temp();
        let mut state = state_with_listing(root.path(), worked_example_listing());
        state.expanded.insert("src".to_string());
        state.expanded.insert("src/tui".to_string());
        state.selected = Some("src/tui/app".to_string());

        assert_eq!(
            state.peek_into_selected(),
            PeekStep::Slide {
                scope: "src/tui".to_string(),
                directory: "src/tui/app".to_string(),
            }
        );
    }

    /// `l` on a file is not a motion: there is nothing to peek into.
    #[test]
    fn peeking_into_a_file_is_a_no_op() {
        let root = temp();
        let mut state = state_with_listing(root.path(), worked_example_listing());
        state.selected = Some("Cargo.toml".to_string());

        assert_eq!(state.peek_into_selected(), PeekStep::None);
    }

    /// `h` from inside a directory: the cursor moves to the parent row and
    /// that directory collapses.
    #[test]
    fn leaving_a_directory_moves_to_the_parent_row_and_collapses_it() {
        let root = temp();
        let mut state = state_with_listing(root.path(), worked_example_listing());
        state.expanded.insert("src".to_string());
        state.expanded.insert("src/tui".to_string());
        state.selected = Some("src/tui/editor.rs".to_string());

        assert_eq!(
            state.peek_up(),
            PeekUp::Leave {
                parent: "src/tui".to_string()
            }
        );
    }

    /// `h` on a top-level row: the scope slides up, with the old scope root
    /// expanded in the new scope.
    #[test]
    fn leaving_a_top_level_row_slides_the_scope_up() {
        let root = temp();
        let mut state = scoped_state(
            root.path(),
            "src/tui",
            vec![node(
                "src/tui/app",
                1,
                TreeNodeKind::Directory,
                Some(vec![node(
                    "src/tui/app/mod.rs",
                    2,
                    TreeNodeKind::File,
                    None,
                )]),
            )],
        );
        state.selected = Some("src/tui/app".to_string());

        assert_eq!(
            state.peek_up(),
            PeekUp::Slide {
                scope: "src".to_string(),
                root: "src/tui".to_string(),
            }
        );
    }

    /// `h` at the workspace root: the scope cannot go higher, so the
    /// selected directory collapses in place. Never a dead key.
    #[test]
    fn leaving_a_top_level_row_at_the_workspace_root_collapses_in_place() {
        let root = temp();
        let mut state = state_with_listing(root.path(), worked_example_listing());
        state.expanded.insert("src".to_string());
        state.selected = Some("src".to_string());

        assert_eq!(
            state.peek_up(),
            PeekUp::Collapse {
                directory: "src".to_string()
            }
        );
    }
}

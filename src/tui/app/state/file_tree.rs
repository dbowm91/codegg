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
//! ## The pane has a root, and it is not always the workspace root
//!
//! [`FileTreeState::root_history`] is the chain of directories the user has
//! walked into with `l`, newest last, so `h` can step back out one level at a
//! time and land the cursor on the directory it just left. The current listing
//! scope is always the last entry, which is what makes "which directory am I
//! looking at" one fact instead of a prefix field and a history field that
//! can disagree.
//!
//! Re-rooting is a **daemon round-trip**, not a local filter: the scope is
//! carried as `WorkspaceDirectoryListRequestDto::prefix`, and the daemon
//! walks that subtree with the same bounded walker. Filtering an
//! already-loaded listing would have been free, but it would silently drop
//! everything the original walk hit its bounds on, presenting a partial
//! subtree as if it were the directory.

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
}

impl FileTreeState {
    /// Rows the user can currently move between.
    ///
    /// The selection is clamped rather than dropped, so a collapse or a
    /// shrink never leaves the tree with no reachable row.
    pub fn rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        visible_rows(&self.nodes, &self.expanded, &mut rows);
        rows
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

    /// Scroll so the selected row stays inside a `height`-row viewport.
    pub fn reveal_selected(&mut self, index: usize, total: usize) {
        if total == 0 {
            self.scroll = 0;
            return;
        }
        // No viewport height is known here; the renderer passes the real one
        // through `ensure_visible`. This keeps the invariant that the selected
        // row is not scrolled past the end of the listing.
        let last_page_start = total.saturating_sub(1);
        self.scroll = self.scroll.min(last_page_start);
        if index < self.scroll {
            self.scroll = index;
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
        let rows = if self.viewport_rows == 0 {
            FALLBACK_PAGE_ROWS
        } else {
            self.viewport_rows
        };
        ((rows * 2) / 3).max(1) as isize
    }

    /// Page the cursor by `direction` pages: `1` down, `-1` up.
    ///
    /// `Ctrl+d`/`Ctrl+u` on a listing long enough to need paging. The
    /// selection moves a full page *and* the scroll follows it, because a
    /// page verb that moved the highlight without moving the listing would
    /// scroll only after the cursor had already left the screen.
    pub fn page_selection(&mut self, direction: isize) {
        if self.rows().is_empty() {
            return;
        }
        self.move_selection(direction * self.page_step());
        if let Some(index) = self.selected_index() {
            self.ensure_visible(index, self.viewport_rows);
        }
    }

    /// Expand or collapse the selected directory.
    ///
    /// Returns the path that changed, so a caller can report it. A file
    /// selection is a no-op: expanding a file is not a meaningful action.
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

    /// Step the pane's root back one level.
    ///
    /// This is the `h`/left-arrow verb, and the counterpart of
    /// [`Self::descend_selected`]: the cursor lands on the directory just
    /// left behind, so stepping out and back in returns to the same place.
    ///
    /// `false` means the pane is already showing the workspace root, where
    /// "one level up" does not exist. The caller falls back to
    /// [`Self::leave_selected`] there rather than making `h` inert at the one
    /// depth every listing starts at.
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
    }

    /// Go back one level *within* the current scope.
    ///
    /// Two cases, in order:
    ///
    /// 1. The cursor is on an expanded directory — collapse it and stay put.
    /// 2. Otherwise step the cursor to the nearest preceding shallower row,
    ///    which is the parent directory of whatever is selected.
    ///
    /// A top-level entry has no parent, so case 2 is a no-op there rather
    /// than an error: backing out of the workspace root means nowhere to go,
    /// and silently clamping is the honest answer.
    ///
    /// This is the `h` fallback at the workspace root, where
    /// [`Self::ascend_root`] has nothing to do.
    pub fn leave_selected(&mut self) -> Option<String> {
        let rows = self.rows();
        let index = self.selected_index()?;
        let row = rows.get(index)?;
        if row.kind == TreeNodeKind::Directory && row.expanded {
            self.expanded.remove(&row.path);
            return Some(row.path.clone());
        }
        let parent = rows[..index]
            .iter()
            .rev()
            .find(|candidate| candidate.depth < row.depth)?;
        let path = parent.path.clone();
        self.selected = Some(path.clone());
        self.reveal_selected(index.saturating_sub(1), rows.len());
        Some(path)
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

        // `h` on an expanded directory closes it and stays put.
        assert_eq!(state.leave_selected().as_deref(), Some("dir"));
        assert!(!state.expanded.contains("dir"));
        assert_eq!(state.selected.as_deref(), Some("dir"));
    }

    #[test]
    fn leave_steps_out_to_the_parent_directory() {
        let root = temp();
        write(root.path(), "dir/nested/deep.txt");

        let mut state = state_with(root.path());
        state.expanded.insert("dir".to_string());
        state.expanded.insert("dir/nested".to_string());
        state.selected = Some("dir/nested/deep.txt".to_string());

        // From a nested file, `h` walks out one level at a time.
        assert_eq!(state.leave_selected().as_deref(), Some("dir/nested"));
        assert_eq!(state.selected.as_deref(), Some("dir/nested"));

        // `dir/nested` is still open, so the next `h` closes it in place
        // rather than jumping again — backing out of an open directory means
        // closing it, and only a *closed* one steps to the parent.
        assert_eq!(state.leave_selected().as_deref(), Some("dir/nested"));
        assert!(!state.expanded.contains("dir/nested"));
        assert_eq!(state.selected.as_deref(), Some("dir/nested"));

        assert_eq!(state.leave_selected().as_deref(), Some("dir"));
        assert_eq!(state.selected.as_deref(), Some("dir"));
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
}

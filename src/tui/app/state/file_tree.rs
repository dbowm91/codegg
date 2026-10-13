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

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::tui::app::state::AsyncUiRequestState;
use crate::tui::file_tree::{visible_rows, TreeListing, TreeNodeKind, TreeRow};

/// Why the pane is showing what it is showing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FileTreeStatus {
    /// Nothing requested yet.
    #[default]
    Idle,
    /// A walk is in flight.
    Loading,
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

    /// Enter the selected directory: expand it and step onto its first child.
    ///
    /// This is the `l`/right-arrow verb. It is deliberately distinct from
    /// [`Self::toggle_selected`], which is what Enter uses: both end up
    /// expanding the same directory, but only this one also *moves* the
    /// cursor, because "go into this directory" and "reveal what is in it"
    /// are different intentions and should leave the cursor somewhere
    /// different.
    ///
    /// A file selection is a no-op — there is no directory to enter — and an
    /// empty directory keeps its selection on itself rather than dropping it,
    /// so the cursor is never left somewhere unreachable.
    pub fn enter_selected(&mut self) -> Option<String> {
        let rows = self.rows();
        let index = self.selected_index()?;
        let row = rows.get(index)?;
        if row.kind != TreeNodeKind::Directory {
            return None;
        }
        let path = row.path.clone();
        // Already open: there is nowhere deeper to go from here.
        if !row.expanded {
            self.expanded.insert(path.clone());
        }
        // Re-flatten: expanding can append rows after this one.
        let rows = self.rows();
        // A directory with no children still stays selected, so an empty
        // directory is visibly opened rather than appearing inert.
        let next = rows
            .get(index + 1)
            .filter(|child| child.depth > row.depth)
            .map(|child| child.path.clone())
            .unwrap_or_else(|| path.clone());
        self.selected = Some(next.clone());
        self.reveal_selected(
            rows.iter()
                .position(|row| row.path == next)
                .unwrap_or(index),
            rows.len(),
        );
        Some(path)
    }

    /// Go back one level.
    ///
    /// This is the `h`/left-arrow verb. Two cases, in order:
    ///
    /// 1. The cursor is on an expanded directory — collapse it and stay put.
    /// 2. Otherwise step the cursor to the nearest preceding shallower row,
    ///    which is the parent directory of whatever is selected.
    ///
    /// A top-level entry has no parent, so case 2 is a no-op there rather
    /// than an error: backing out of the workspace root means nowhere to go,
    /// and silently clamping is the honest answer.
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

    /// Apply a completed walk, if it belongs to the current generation and
    /// root.
    ///
    /// A stale completion is dropped rather than applied, which is what keeps
    /// a tree from rendering against a workspace the user has left.
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

    /// Begin a rebuild against `root`.
    ///
    /// The generation bump is what invalidates any walk already in flight.
    pub fn begin_rebuild(&mut self, root: PathBuf) -> (u64, u64) {
        if self.root.as_deref() != Some(root.as_path()) {
            // A new workspace resets everything: a path from the previous
            // project is meaningless in the next.
            self.expanded.clear();
            self.selected = None;
            self.scroll = 0;
        }
        self.root = Some(root);
        self.generation = self.generation.wrapping_add(1);
        self.status = FileTreeStatus::Loading;
        self.truncated = None;
        let request_id = self.request.begin();
        (self.generation, request_id)
    }

    /// Reset for a project switch.
    pub fn reset_for_project(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.root = None;
        self.nodes.clear();
        self.expanded.clear();
        self.selected = None;
        self.scroll = 0;
        self.truncated = None;
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
    fn enter_steps_into_a_directory_and_onto_its_first_child() {
        let root = temp();
        write(root.path(), "dir/a.txt");
        write(root.path(), "dir/b.txt");
        write(root.path(), "top.txt");

        let mut state = state_with(root.path());
        assert_eq!(state.selected.as_deref(), Some("dir"));

        // `l` expands *and* moves the cursor, which is what distinguishes it
        // from the Enter/toggle verb that leaves the cursor where it was.
        assert_eq!(state.enter_selected().as_deref(), Some("dir"));
        assert!(state.expanded.contains("dir"));
        assert_eq!(state.selected.as_deref(), Some("dir/a.txt"));

        // Expanding *without* moving (what Enter does) and then `l`: entering
        // an already-open directory is idempotent, not a double-toggle that
        // would collapse what the user just walked into.
        state.expanded.insert("dir".to_string());
        state.selected = Some("dir".to_string());
        assert_eq!(state.enter_selected().as_deref(), Some("dir"));
        assert!(
            state.expanded.contains("dir"),
            "`l` must not collapse a directory it just walked into"
        );
        assert_eq!(state.selected.as_deref(), Some("dir/a.txt"));

        // A file has no directory to enter.
        state.selected = Some("top.txt".to_string());
        assert_eq!(state.enter_selected(), None);
        assert_eq!(state.selected.as_deref(), Some("top.txt"));
    }

    #[test]
    fn enter_on_an_empty_directory_keeps_the_cursor_rather_than_dropping_it() {
        let root = temp();
        std::fs::create_dir_all(root.path().join("empty")).expect("mkdir");

        let mut state = state_with(root.path());
        assert_eq!(state.selected.as_deref(), Some("empty"));
        assert_eq!(state.enter_selected().as_deref(), Some("empty"));
        assert_eq!(
            state.selected.as_deref(),
            Some("empty"),
            "an empty directory must stay selected so the cursor is never stranded"
        );
        assert!(state.expanded.contains("empty"));
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
        assert_eq!(state.selected.as_deref(), Some("dir"));

        state.enter_selected();
        assert_eq!(state.selected.as_deref(), Some("dir/nested"));
        state.enter_selected();
        assert_eq!(state.selected.as_deref(), Some("dir/nested/deep.txt"));

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
    fn switching_projects_resets_selection_and_expansion() {
        let first = temp();
        let second = temp();
        write(first.path(), "a/b.txt");
        write(second.path(), "c.txt");

        let mut state = state_with(first.path());
        state.toggle_selected();
        assert!(state.expanded.contains("a"));

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

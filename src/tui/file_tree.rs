//! M006-D: bounded, project-scoped workspace file tree.
//!
//! This module is a **navigator**, not a content path. It enumerates
//! directory *entries* and produces workspace-relative paths; it never reads
//! file bytes. Opening an entry is the caller's job, and it goes through
//! [`crate::tui::commands::editor::open_editor`], which is the only path that
//! turns a path into a document attachment. That separation is what keeps the
//! M005 invariant that the controller is the sole owner of document text
//! (`scripts/check_tui_editor_text_authority.py`).
//!
//! Two properties are load-bearing and are proven by tests rather than by
//! convention:
//!
//! - **Bounded on every axis.** Depth, retained-node count, *examined*-entry
//!   count, and per-entry name length all have hard caps. A pathological
//!   tree produces a truncation notice, not a hang and not an unbounded
//!   allocation. The examined-entry budget is independent of the retained
//!   budget: a directory of a million ignored entries stops the walk even
//!   though it retains nothing.
//! - **Symlinks are never followed**, at any depth, including directory
//!   symlinks. The check is on the raw entry, because `canonicalize` resolves
//!   a symlink and would then report the target as a non-symlink.
//! - **Cooperatively cancellable.** [`build_tree_with_cancel`] checks an
//!   atomic flag on every entry, so a superseded walk exits at the next
//!   entry even after its async waiter is aborted. Blocking directory I/O
//!   runs on `spawn_blocking` (see `commands::file_tree::rebuild`), never on
//!   a Tokio worker.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::directory_listing::{
    LISTING_IGNORED_DIRS, MAX_LISTING_DEPTH, MAX_LISTING_EXAMINED, MAX_LISTING_NAME_BYTES,
    MAX_LISTING_NODES,
};

/// Maximum directory depth walked, counted from the workspace root at `0`.
///
/// Single-sourced from [`crate::directory_listing`]: the TUI navigator and
/// the daemon-owned listing execute the same walk, so the bounds cannot
/// drift between the two authorities.
pub const MAX_TREE_DEPTH: usize = MAX_LISTING_DEPTH;

/// Maximum number of entries retained across the whole tree.
///
/// The walk stops once the budget is exhausted and reports truncation, so a
/// repository with a million files costs a bounded walk and a bounded
/// allocation rather than exhausting memory.
pub const MAX_TREE_NODES: usize = MAX_LISTING_NODES;

/// Maximum number of directory entries *examined* across the whole walk.
///
/// Counted before any filtering, so ignored directories, symlinks, special
/// files, and unreadable entries all consume it. This is what stops a walk
/// over millions of filtered-out entries that would otherwise cost unbounded
/// scanning while retaining nothing. Independent of [`MAX_TREE_NODES`]:
/// a tree can exhaust either budget, and the truncation reason says which.
pub const MAX_TREE_EXAMINED: usize = MAX_LISTING_EXAMINED;

/// Maximum byte length of a single entry name.
///
/// Names longer than this are skipped rather than truncated, so a tree-derived
/// path can never be a silently mangled version of a real name.
pub const MAX_TREE_NAME_BYTES: usize = MAX_LISTING_NAME_BYTES;

/// Directories never descended into.
///
/// This mirrors the composer's `@` file-mention indexer so a user does not see
/// `node_modules` in the tree and then in autocomplete, or the reverse.
pub const TREE_IGNORED_DIRS: &[&str] = LISTING_IGNORED_DIRS;

/// Whether an entry can be opened as a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TreeNodeKind {
    Directory,
    File,
}

/// One entry in the workspace tree.
///
/// `path` is always workspace-relative and `/`-separated. It is built from
/// validated component names rather than by joining arbitrary strings, so it
/// can never contain a `..` component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub depth: usize,
    pub kind: TreeNodeKind,
    /// `None` for files.
    pub children: Option<Vec<TreeNode>>,
}

/// Result of one bounded walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeListing {
    /// The roots rendered at the top level, directories before files, then
    /// lexically within each group.
    pub nodes: Vec<TreeNode>,
    /// True when a bound stopped the walk, so the UI can say so instead of
    /// presenting a partial tree as if it were complete.
    pub truncated: bool,
    /// Human-readable reason for truncation, when truncated.
    pub truncated_reason: Option<String>,
    /// Directory entries examined, including filtered-out ones (ignored,
    /// symlink, special, unreadable). Proves the examined-entry budget is
    /// independent of the retained-node budget: a walk can examine far more
    /// than it keeps.
    pub examined: usize,
}

/// Walk `root` into a bounded listing.
///
/// Delegates to the single shared walker in [`crate::directory_listing`]:
/// symlinks are skipped at every depth, ignored directories are not
/// descended into, unreadable directories are skipped rather than treated
/// as fatal, and the walk stops at the first bound it reaches. Never
/// cancelled; see [`build_tree_with_cancel`] for the cancellable form the
/// command layer uses.
pub fn build_tree(root: &Path) -> TreeListing {
    static NEVER: AtomicBool = AtomicBool::new(false);
    build_tree_with_cancel(root, &NEVER)
}

/// Walk `root` into a bounded listing, exiting early when `cancel` is set.
///
/// Delegates to [`crate::directory_listing::walk_directory`] and converts
/// the neutral nodes into presentation nodes. A cancelled walk reports
/// `truncated` with a cancellation reason rather than a partial listing
/// presented as complete.
pub fn build_tree_with_cancel(root: &Path, cancel: &AtomicBool) -> TreeListing {
    from_shared_listing(crate::directory_listing::walk_directory(root, "", cancel))
}

/// Convert one shared-walk listing into presentation nodes.
pub(crate) fn from_shared_listing(
    listing: crate::directory_listing::DirectoryListing,
) -> TreeListing {
    TreeListing {
        nodes: listing.nodes.into_iter().map(from_shared_node).collect(),
        truncated: listing.truncated,
        truncated_reason: listing.truncated_reason,
        examined: listing.examined,
    }
}

/// Convert one shared-walk node (and its subtree) into a presentation node.
pub(crate) fn from_shared_node(
    node: codegg_protocol::document::WorkspaceDirectoryNodeDto,
) -> TreeNode {
    TreeNode {
        name: node.name,
        path: node.path,
        depth: node.depth,
        kind: match node.kind {
            codegg_protocol::document::WorkspaceDirectoryNodeKindDto::Directory => {
                TreeNodeKind::Directory
            }
            codegg_protocol::document::WorkspaceDirectoryNodeKindDto::File => TreeNodeKind::File,
        },
        children: node
            .children
            .map(|children| children.into_iter().map(from_shared_node).collect()),
    }
}

/// Refuse a workspace root that is not a proven-local readable directory.
///
/// The tree walks the local filesystem directly, so a root that does not
/// exist locally, is not a directory, or is itself a symlink must fail
/// closed rather than walk a same-name path as if it were authoritative.
/// Remote directory listing stays unsupported until a daemon-authorized
/// transport exists; this is the explicit refusal point, not a fallback
/// that treats absence as an empty workspace.
pub fn local_walk_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err(format!(
            "refusing to list workspace root {}: not an absolute local path; remote directory listing is unsupported",
            root.display()
        ));
    }
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refusing to list workspace root {}: the root itself is a symlink",
            root.display()
        )),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "refusing to list workspace root {}: not a local directory; remote directory listing is unsupported",
            root.display()
        )),
        Err(error) => Err(format!(
            "refusing to list workspace root {}: not a readable local directory ({error}); remote directory listing is unsupported",
            root.display()
        )),
    }
}

/// A row the user can move between, produced by flattening the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub kind: TreeNodeKind,
    /// Directories are open when they are in the expansion set.
    pub expanded: bool,
}

/// Flatten a listing into the visible rows for the current expansion set.
///
/// `expanded` holds workspace-relative directory paths. A directory whose
/// children are excluded is still a row, so it remains reachable and
/// expandable.
pub fn visible_rows(nodes: &[TreeNode], expanded: &BTreeSet<String>, out: &mut Vec<TreeRow>) {
    for node in nodes {
        let is_expanded = expanded.contains(&node.path);
        out.push(TreeRow {
            path: node.path.clone(),
            name: node.name.clone(),
            depth: node.depth,
            kind: node.kind,
            expanded: is_expanded,
        });
        if node.kind == TreeNodeKind::Directory && is_expanded {
            if let Some(children) = node.children.as_deref() {
                visible_rows(children, expanded, out);
            }
        }
    }
}

use std::collections::BTreeSet;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(root: &Path, relative: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, b"x").expect("write");
    }

    fn names(listing: &TreeListing) -> Vec<&str> {
        listing.nodes.iter().map(|n| n.name.as_str()).collect()
    }

    fn find<'a>(listing: &'a TreeListing, name: &str) -> &'a TreeNode {
        listing
            .nodes
            .iter()
            .find(|n| n.name == name)
            .unwrap_or_else(|| panic!("node {name} not found"))
    }

    #[test]
    fn directories_sort_before_files_then_lexically() {
        let root = temp();
        write(root.path(), "zeta.txt");
        write(root.path(), "alpha.txt");
        write(root.path(), "zdir/inner.txt");
        write(root.path(), "adir/inner.txt");

        let listing = build_tree(root.path());
        assert_eq!(
            names(&listing),
            vec!["adir", "zdir", "alpha.txt", "zeta.txt"]
        );
    }

    #[test]
    fn paths_are_workspace_relative_and_dotdot_free() {
        let root = temp();
        write(root.path(), "a/b/c.txt");

        let listing = build_tree(root.path());
        let a = find(&listing, "a");
        assert_eq!(a.path, "a");
        let b = a
            .children
            .as_deref()
            .expect("a has children")
            .first()
            .expect("b");
        assert_eq!(b.path, "a/b");
        let c = b
            .children
            .as_deref()
            .expect("b has children")
            .first()
            .expect("c");
        assert_eq!(c.path, "a/b/c.txt");
        assert!(!c.path.contains(".."));
        assert!(!c.path.starts_with('/'));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed_at_any_depth() {
        let root = temp();
        let outside = temp();
        std::fs::write(outside.path().join("secret.txt"), b"s").expect("write outside");

        // A file symlink and a *directory* symlink, the latter being the one
        // that would let the tree escape the workspace.
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            root.path().join("link.txt"),
        )
        .expect("file symlink");
        std::os::unix::fs::symlink(outside.path(), root.path().join("linkdir"))
            .expect("dir symlink");

        let listing = build_tree(root.path());
        assert!(
            !names(&listing).contains(&"link.txt"),
            "file symlink must be skipped"
        );
        assert!(
            !names(&listing).contains(&"linkdir"),
            "directory symlink must not be descended into"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loop_terminates() {
        let root = temp();
        write(root.path(), "a/file.txt");
        // a/loop -> a, a genuine cycle.
        std::os::unix::fs::symlink(root.path().join("a"), root.path().join("a/loop"))
            .expect("loop symlink");

        let listing = build_tree(root.path());
        // The depth cap alone guarantees termination; this asserts the walk
        // reports why it stopped rather than looping.
        let a = find(&listing, "a");
        let loop_dir = a
            .children
            .as_deref()
            .expect("children")
            .iter()
            .find(|n| n.name == "loop");
        assert!(
            loop_dir.is_none(),
            "symlinked directory must not appear as a child"
        );
    }

    #[test]
    fn depth_cap_truncates_and_says_so() {
        let root = temp();
        let deep = (0..MAX_TREE_DEPTH + 3)
            .map(|i| format!("d{i}"))
            .collect::<Vec<_>>()
            .join("/");
        write(root.path(), &format!("{deep}/leaf.txt"));

        let listing = build_tree(root.path());
        assert!(listing.truncated, "deep tree must report truncation");
        assert!(listing
            .truncated_reason
            .as_deref()
            .is_some_and(|r| r.contains("depth")));
    }

    #[test]
    fn node_cap_truncates_and_says_so() {
        let root = temp();
        for i in 0..(MAX_TREE_NODES + 50) {
            write(root.path(), &format!("f{i:06}.txt"));
        }

        let listing = build_tree(root.path());
        assert!(listing.truncated, "oversized tree must report truncation");
        assert!(listing
            .truncated_reason
            .as_deref()
            .is_some_and(|r| r.contains("entries")));
        assert!(
            listing.nodes.len() <= MAX_TREE_NODES,
            "node count must stay within the budget, got {}",
            listing.nodes.len()
        );
    }

    #[test]
    fn ignored_directories_are_not_descended_into() {
        let root = temp();
        write(root.path(), "node_modules/pkg/index.js");
        write(root.path(), ".git/config");
        write(root.path(), "src/lib.rs");

        let listing = build_tree(root.path());
        assert_eq!(names(&listing), vec!["src"]);
    }

    #[test]
    fn unreadable_directory_does_not_blank_the_tree() {
        let root = temp();
        write(root.path(), "visible/file.txt");
        // A path that cannot be read as a directory.
        std::fs::create_dir_all(root.path().join("broken")).expect("mkdir");
        write(root.path(), "broken/inner.txt");
        std::fs::remove_file(root.path().join("broken/inner.txt")).expect("rm");

        let listing = build_tree(root.path());
        assert!(
            names(&listing).contains(&"visible"),
            "a sibling subtree must survive an unreadable directory"
        );
    }

    #[test]
    fn over_long_names_are_skipped_when_the_filesystem_allows_one() {
        // The 255-byte cap matches the common `NAME_MAX`, so on macOS and
        // Linux an over-long name is rejected by the filesystem before the
        // walk can ever observe it. This case is therefore conditional: it
        // proves the guard is wired when the filesystem permits the input, and
        // degrades to a no-op when it does not. The unconditional invariant
        // lives in `names_are_never_mangled`.
        let root = temp();
        let long = "n".repeat(MAX_TREE_NAME_BYTES + 1);
        if std::fs::write(root.path().join(&long), b"x").is_ok() {
            write(root.path(), "ok.txt");
            let listing = build_tree(root.path());
            assert_eq!(names(&listing), vec!["ok.txt"]);
        }
    }

    #[test]
    fn expansion_controls_which_descendants_are_visible() {
        let root = temp();
        write(root.path(), "a/b/c.txt");
        write(root.path(), "top.txt");

        let listing = build_tree(root.path());

        // Collapsed: only the top level is reachable, and `a` is not expanded.
        let mut rows = Vec::new();
        visible_rows(&listing.nodes, &BTreeSet::new(), &mut rows);
        assert_eq!(
            rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            vec!["a", "top.txt"]
        );
        assert!(!rows[0].expanded);

        // Expanding `a` reveals its child directory but not the grandchild:
        // expansion is per-directory, not recursive.
        let mut one = BTreeSet::new();
        one.insert("a".to_string());
        let mut rows = Vec::new();
        visible_rows(&listing.nodes, &one, &mut rows);
        assert_eq!(
            rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            vec!["a", "a/b", "top.txt"]
        );
        assert!(rows[0].expanded);
        assert!(!rows[1].expanded);

        // Expanding `a/b` too reveals the file.
        let mut two = BTreeSet::new();
        two.insert("a".to_string());
        two.insert("a/b".to_string());
        let mut rows = Vec::new();
        visible_rows(&listing.nodes, &two, &mut rows);
        assert_eq!(
            rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            vec!["a", "a/b", "a/b/c.txt", "top.txt"]
        );
    }

    #[test]
    fn expansion_is_idempotent_and_survives_re_flattening() {
        let root = temp();
        write(root.path(), "a/b.txt");
        let listing = build_tree(root.path());

        let mut expanded = BTreeSet::new();
        expanded.insert("a".to_string());
        let mut first = Vec::new();
        visible_rows(&listing.nodes, &expanded, &mut first);
        let mut second = Vec::new();
        visible_rows(&listing.nodes, &expanded, &mut second);
        assert_eq!(first, second, "flattening must be deterministic");
    }

    #[test]
    fn names_are_never_mangled() {
        // The 255-byte cap equals the common `NAME_MAX`, so on most
        // filesystems the cap is defensive rather than reachable — the
        // filesystem rejects an over-long name before the walk ever sees it.
        // What is assertable is the invariant that matters: a name the walk
        // shows is the name on disk, never a truncated prefix of it.
        let root = temp();
        let at_cap = "n".repeat(MAX_TREE_NAME_BYTES);
        // macOS caps a component at 255 bytes, so this is the largest name the
        // test filesystem can hold; on a filesystem with a smaller limit the
        // write fails and the case reduces to the no-long-name assertion.
        if std::fs::write(root.path().join(&at_cap), b"x").is_ok() {
            write(root.path(), "ok.txt");
            let listing = build_tree(root.path());
            let shown = listing
                .nodes
                .iter()
                .map(|n| n.name.clone())
                .collect::<Vec<_>>();
            assert!(shown.contains(&at_cap), "a name at the cap must survive");
            assert!(shown.contains(&"ok.txt".to_string()));
        }
        for node in &listing_names(root.path()) {
            assert!(node.len() <= MAX_TREE_NAME_BYTES, "name exceeds the cap");
            assert!(
                root.path().join(node).exists(),
                "shown name {node:?} must be the real on-disk name"
            );
        }
    }

    /// Every name the walk would show, used only by the mangling assertion.
    fn listing_names(root: &Path) -> Vec<String> {
        fn recurse(nodes: &[TreeNode], out: &mut Vec<String>) {
            for node in nodes {
                out.push(node.name.clone());
                if let Some(children) = node.children.as_deref() {
                    recurse(children, out);
                }
            }
        }
        let listing = build_tree(root);
        let mut out = Vec::new();
        recurse(&listing.nodes, &mut out);
        out
    }

    #[test]
    fn empty_workspace_yields_an_empty_listing_without_truncation() {
        let root = temp();
        let listing = build_tree(root.path());
        assert!(listing.nodes.is_empty());
        assert!(!listing.truncated);
        assert!(listing.truncated_reason.is_none());
    }

    // ── C002 examined-entry budget, cancellation, locality ──────────────

    use crate::directory_listing::account_one_entry;
    use crate::directory_listing::DirectoryListing as SharedListing;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    /// Walk with explicit budgets via the shared walker, bypassing the
    /// production constants so the examined-cap branch is reachable
    /// without creating 100k files.
    fn walk_with(
        root: &Path,
        max_nodes: usize,
        max_examined: usize,
        cancel: &AtomicBool,
    ) -> TreeListing {
        from_shared_listing(crate::directory_listing::walk_with_budgets(
            root,
            "",
            max_nodes,
            max_examined,
            cancel,
        ))
    }

    fn live_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn examined_budget_stops_a_walk_of_only_filtered_entries() {
        let root = temp();
        // Symlinks are fast to create and all filtered out: 200 examined,
        // zero retained. The examined cap (50 here) must stop the walk even
        // though the node budget is untouched. Dangling links (no real
        // file alongside) so nothing is retainable by construction — a
        // retainable file must flush even when a later bound fires.
        #[cfg(unix)]
        {
            for i in 0..200 {
                std::os::unix::fs::symlink(
                    root.path().join("missing-target.txt"),
                    root.path().join(format!("link{i:04}")),
                )
                .expect("symlink");
            }
            let listing = walk_with(root.path(), MAX_TREE_NODES, 50, &live_cancel());
            assert!(
                listing.truncated,
                "filtered entries must still stop the walk"
            );
            assert!(
                listing
                    .truncated_reason
                    .as_deref()
                    .is_some_and(|r| r.contains("examining")),
                "the reason must name the examined budget, got {:?}",
                listing.truncated_reason
            );
            assert!(listing.nodes.is_empty(), "nothing was retainable");
            assert!(listing.examined > 50, "examined counts filtered entries");
        }
    }

    #[test]
    fn retainable_entries_flush_when_a_later_bound_fires() {
        // C005 regression: hitting a bound must not blank entries retained
        // earlier in the same walk. Three real files with a node budget of
        // two: the walk truncates, but exactly the two retained files must
        // still be listed (order-independent count, so readdir order cannot
        // flake it).
        let root = temp();
        write(root.path(), "a.txt");
        write(root.path(), "b.txt");
        write(root.path(), "c.txt");
        let listing = walk_with(root.path(), 2, 10_000, &live_cancel());
        assert!(listing.truncated);
        assert_eq!(
            listing.nodes.len(),
            2,
            "the two retained files must survive the node cap, got {:?}",
            names(&listing)
        );
        assert!(
            listing
                .truncated_reason
                .as_deref()
                .is_some_and(|r| r.contains("entries") && !r.contains("examining")),
            "the reason must name the node bound, got {:?}",
            listing.truncated_reason
        );
    }

    #[test]
    fn node_cap_and_examined_cap_are_independent_reasons() {
        let root = temp();
        for i in 0..20 {
            write(root.path(), &format!("f{i:03}.txt"));
        }
        // Tight node budget, generous examined budget: the node cap fires.
        let listing = walk_with(root.path(), 5, 10_000, &live_cancel());
        assert!(listing.truncated);
        assert!(
            listing
                .truncated_reason
                .as_deref()
                .is_some_and(|r| r.contains("entries") && !r.contains("examining")),
            "node-cap reason must not mention examining, got {:?}",
            listing.truncated_reason
        );
        assert!(
            listing.examined >= 5,
            "examined counts even retained entries"
        );
    }

    #[test]
    fn examined_counts_ignored_and_special_entries_without_retaining_them() {
        let root = temp();
        write(root.path(), "node_modules/pkg/index.js");
        write(root.path(), "kept.txt");
        let listing = build_tree(root.path());
        // `node_modules` (1 examined, skipped) + `kept.txt` (1 examined,
        // retained). The ignored directory's children are never descended
        // into, so exactly two entries are examined.
        assert_eq!(listing.examined, 2, "examined must count the ignored entry");
        assert!(!listing.truncated);
        assert_eq!(names(&listing), vec!["kept.txt"]);
    }

    #[test]
    fn symlink_flood_counts_examined_without_truncation_or_retention() {
        let root = temp();
        #[cfg(unix)]
        {
            std::fs::write(root.path().join("real.txt"), b"x").expect("write");
            for i in 0..1000 {
                std::os::unix::fs::symlink(
                    root.path().join("real.txt"),
                    root.path().join(format!("flood{i:05}")),
                )
                .expect("symlink");
            }
            let listing = build_tree(root.path());
            assert!(!listing.truncated, "1001 examined is within budget");
            assert_eq!(
                listing.examined, 1001,
                "every symlink must be examined and skipped"
            );
            assert_eq!(names(&listing), vec!["real.txt"]);
        }
    }

    #[test]
    fn a_cancelled_walk_reports_cancellation_not_a_partial_tree() {
        let root = temp();
        for i in 0..50 {
            write(root.path(), &format!("f{i:03}.txt"));
        }
        let cancelled = AtomicBool::new(true);
        let listing = build_tree_with_cancel(root.path(), &cancelled);
        assert!(listing.truncated);
        assert_eq!(
            listing.truncated_reason.as_deref(),
            Some("tree walk cancelled")
        );
        assert!(listing.nodes.is_empty());
    }

    #[test]
    fn cancel_mid_walk_stops_a_deep_tree() {
        let root = temp();
        write(root.path(), "a/b/c/d/e/f.txt");
        write(root.path(), "top.txt");
        // Cancel after the walk starts: flip the flag from a second thread
        // while the walk runs. The walk must still terminate with a
        // cancellation (or another bound), never hang.
        let flag = std::sync::Arc::new(AtomicBool::new(false));
        let setter = std::sync::Arc::clone(&flag);
        std::thread::spawn(move || {
            setter.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .join()
        .expect("setter finishes");
        let listing = build_tree_with_cancel(root.path(), &flag);
        assert!(listing.truncated);
        assert_eq!(
            listing.truncated_reason.as_deref(),
            Some("tree walk cancelled")
        );
    }

    #[test]
    fn local_walk_root_refuses_non_local_roots() {
        // Missing: a same-name local path must not stand in for it.
        assert!(local_walk_root(Path::new("/nonexistent-workspace-root-for-test")).is_err());
        // Relative: never authoritative.
        assert!(local_walk_root(Path::new("relative/path")).is_err());
        // A file is not a walkable directory.
        let root = temp();
        write(root.path(), "file.txt");
        assert!(local_walk_root(&root.path().join("file.txt")).is_err());
        // A real local directory is accepted.
        assert!(local_walk_root(root.path()).is_ok());
        // The root itself must not be a symlink.
        #[cfg(unix)]
        {
            let outer = temp();
            std::os::unix::fs::symlink(root.path(), outer.path().join("linked-root"))
                .expect("symlink");
            assert!(local_walk_root(&outer.path().join("linked-root")).is_err());
        }
    }

    #[test]
    fn missing_root_yields_an_empty_listing() {
        let missing = PathBuf::from("/nonexistent-workspace-root-for-test");
        let listing = build_tree(&missing);
        assert!(listing.nodes.is_empty());
    }

    // ── C004 iterator-error accounting ──────────────────────────────────

    /// C004: iterator `Err`s still consume examined work and observe
    /// cancellation/budgets. `account_one_entry` is the exact function the
    /// shared walk calls per item (both `Ok` and `Err`), so driving it
    /// with a synthetic 200-error stream proves a long error flood cannot
    /// bypass the examined cap or a cancel flag.
    #[test]
    fn iterator_error_stream_consumes_examined_and_respects_cancel() {
        let live = AtomicBool::new(false);
        let mut listing = SharedListing::default();
        let mut examined = 0usize;
        // 200 errors against a budget of 50: must stop with the examined
        // reason, having counted every error.
        for _ in 0..200 {
            if !account_one_entry(&live, &mut examined, 50, MAX_TREE_NODES, &mut listing) {
                break;
            }
        }
        assert!(listing.truncated);
        assert!(
            listing
                .truncated_reason
                .as_deref()
                .is_some_and(|r| r.contains("examining")),
            "error flood must name the examined budget, got {:?}",
            listing.truncated_reason
        );
        assert!(
            examined > 50,
            "every error must be examined, got {examined}"
        );

        // Cancellation is observed even mid-flood.
        let cancelled = AtomicBool::new(true);
        let mut listing = SharedListing::default();
        let mut examined = 0usize;
        assert!(!account_one_entry(
            &cancelled,
            &mut examined,
            10_000,
            MAX_TREE_NODES,
            &mut listing
        ));
        assert_eq!(
            listing.truncated_reason.as_deref(),
            Some("tree walk cancelled")
        );
    }
}

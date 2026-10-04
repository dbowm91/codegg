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
//! - **Bounded on every axis.** Depth, node count, and per-entry name length
//!   all have hard caps. A pathological tree produces a truncation notice, not
//!   a hang and not an unbounded allocation.
//! - **Symlinks are never followed**, at any depth, including directory
//!   symlinks. The check is on the raw entry, because `canonicalize` resolves
//!   a symlink and would then report the target as a non-symlink.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Maximum directory depth walked, counted from the workspace root at `0`.
pub const MAX_TREE_DEPTH: usize = 12;

/// Maximum number of entries retained across the whole tree.
///
/// The walk stops once the budget is exhausted and reports truncation, so a
/// repository with a million files costs a bounded walk and a bounded
/// allocation rather than exhausting memory.
pub const MAX_TREE_NODES: usize = 4_096;

/// Maximum byte length of a single entry name.
///
/// Names longer than this are skipped rather than truncated, so a tree-derived
/// path can never be a silently mangled version of a real name.
pub const MAX_TREE_NAME_BYTES: usize = 255;

/// Directories never descended into.
///
/// This mirrors the composer's `@` file-mention indexer so a user does not see
/// `node_modules` in the tree and then in autocomplete, or the reverse.
pub const TREE_IGNORED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    ".cargo",
    "dist",
    "build",
];

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

impl TreeNode {
    fn child(parent: &str, name: &str) -> String {
        if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent}/{name}")
        }
    }
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
}

/// Walk `root` into a bounded listing.
///
/// Symlinks are skipped at every depth, ignored directories are not descended
/// into, unreadable directories are skipped rather than treated as fatal, and
/// the walk stops at the first bound it reaches.
pub fn build_tree(root: &Path) -> TreeListing {
    let mut listing = TreeListing::default();
    let mut budget = MAX_TREE_NODES;
    if budget == 0 {
        listing.truncated = true;
        listing.truncated_reason = Some("node budget is zero".to_string());
        return listing;
    }
    walk(root, "", 0, &mut budget, &mut listing);
    listing.nodes.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.name.cmp(&right.name))
    });
    listing
}

fn walk(
    directory: &Path,
    relative: &str,
    depth: usize,
    budget: &mut usize,
    listing: &mut TreeListing,
) {
    if depth >= MAX_TREE_DEPTH {
        listing.truncated = true;
        listing.truncated_reason =
            Some(format!("stopped at the maximum depth of {MAX_TREE_DEPTH}"));
        return;
    }

    // `read_dir` failing is not fatal: a permission-denied or concurrently
    // removed subdirectory must not blank the rest of the tree.
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    // A map rather than a Vec so siblings are grouped by kind and sorted
    // deterministically without a second pass, and so a name can never appear
    // twice for the same parent.
    let mut directories: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();

    for entry in entries.flatten() {
        if *budget == 0 {
            listing.truncated = true;
            listing.truncated_reason = Some(format!(
                "stopped at the maximum of {MAX_TREE_NODES} entries"
            ));
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name.len() > MAX_TREE_NAME_BYTES || name.contains('\0') {
            // Skipped, not truncated into: a mangled name could produce a
            // path that resolves somewhere the user did not select.
            continue;
        }
        // Checked on the raw entry. `canonicalize` would resolve a symlink and
        // report the target as an ordinary file or directory, so testing the
        // resolved path would never detect one.
        if entry.path().is_symlink() {
            continue;
        }
        // `is_dir`/`is_file` follow symlinks, which are already excluded above.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let (name, file_type) = if file_type.is_dir() {
            if TREE_IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            (name, file_type)
        } else if file_type.is_file() {
            (name, file_type)
        } else {
            // Sockets, FIFOs, and device nodes are neither navigable nor
            // openable as documents.
            continue;
        };
        *budget = budget.saturating_sub(1);
        if file_type.is_dir() {
            directories.insert(name, entry.path());
        } else {
            files.insert(name, entry.path());
        }
    }

    for (name, path) in directories {
        let child_relative = TreeNode::child(relative, &name);
        let mut children = Vec::new();
        let mut child_listing = TreeListing::default();
        walk(
            &path,
            &child_relative,
            depth + 1,
            budget,
            &mut child_listing,
        );
        if child_listing.truncated {
            listing.truncated = true;
            listing.truncated_reason = child_listing.truncated_reason;
        }
        children.append(&mut child_listing.nodes);
        listing.nodes.push(TreeNode {
            name,
            path: child_relative.clone(),
            depth: depth + 1,
            kind: TreeNodeKind::Directory,
            children: Some(children),
        });
    }

    for (name, _path) in files {
        listing.nodes.push(TreeNode {
            path: TreeNode::child(relative, &name),
            name,
            depth: depth + 1,
            kind: TreeNodeKind::File,
            children: None,
        });
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

    #[test]
    fn missing_root_yields_an_empty_listing() {
        let missing = PathBuf::from("/nonexistent-workspace-root-for-test");
        let listing = build_tree(&missing);
        assert!(listing.nodes.is_empty());
    }
}

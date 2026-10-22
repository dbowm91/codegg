//! C005 (ADR-0022): the single daemon-neutral workspace directory walker.
//!
//! This module owns the bounded traversal that both the TUI navigator and
//! the daemon-owned `WorkspaceDirectoryList` handler execute. There is
//! exactly one walk authority: `src/tui/file_tree.rs` delegates to
//! [`walk_directory`] and converts the result into its presentation types,
//! and `src/core/daemon_documents.rs` calls [`walk_directory`] inside a
//! `spawn_blocking` worker under the daemon's single-flight admission.
//!
//! Load-bearing properties (proven by the delegating `file_tree` tests and
//! the daemon handler tests, not by convention):
//!
//! - **Bounded on every axis.** Depth, retained-node count,
//!   *examined*-entry count, and per-entry name length all have hard caps.
//! - **One listing is a bounded *window*, not the whole tree.** See
//!   [`MAX_LISTING_DEPTH`] for why the depth cap is small and what replaces
//!   unbounded nesting as the way to reach a deeper entry.
//! - **Symlinks are never followed**, at any depth, including directory
//!   symlinks. The check is on the raw entry.
//! - **Cooperatively cancellable.** `cancel` is observed on every entry.
//! - **No file bytes are read.** Only entry metadata becomes nodes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use codegg_protocol::document::{WorkspaceDirectoryNodeDto, WorkspaceDirectoryNodeKindDto};

/// Maximum directory depth **embedded** in one listing, counted from the walk
/// root at `0`.
///
/// This is a *window* size, not a navigation limit, and that distinction is
/// the whole reason it is small. A listing is scoped: `WorkspaceDirectoryList`
/// carries a `prefix`, and `l` re-walks the selected directory as a new root
/// (`FileTreeState::set_root_prefix`). So a three-level window is not a cap on
/// how deep the user can browse — it is how much of any subtree one response
/// carries.
///
/// It used to be 12, and a deep embed made the response cap bind on the wrong
/// axis. The wire enforcer keeps the longest **top-level prefix** that fits
/// `MAX_DIRECTORY_LISTING_RESPONSE_BYTES` (ADR-0022), so one directory with a
/// deep subtree monopolises the budget and every sibling root is trimmed away.
/// In a repository whose first alphabetical directory was a large one, a
/// 512 KiB budget retained exactly **one** root node and the pane showed a
/// single unusable row: no second row to select, nothing to expand. Raising
/// [`MAX_LISTING_NODES`] did not help — the binding constraint was bytes, not
/// node count.
///
/// Bounding the embedded depth keeps every root node small, so the prefix trim
/// keeps *all* of them. Measured on a 75,956-entry repository: depth 12
/// retained 1 root, depth 3 retains all 37 roots in 290 KiB.
pub const MAX_LISTING_DEPTH: usize = 3;

/// The reason the walk reports when the depth window stopped it.
///
/// Single-sourced so the TUI can recognise it: the depth window is a
/// *navigational* bound, not a loss. The panel slides its scope when a
/// directory's children fall outside the window, so the user can always
/// reach further — a notice about it would report a limit that is not one.
pub fn depth_stop_reason() -> String {
    format!("stopped at the maximum depth of {MAX_LISTING_DEPTH}")
}

/// Whether a truncation reason is **only** the depth window.
///
/// Every other bound — the retained-node budget, the examined-entry budget,
/// the wire budget — hides entries the panel cannot show at all, and those
/// stay visible. The depth window alone is a scope-relative shape the panel
/// navigates around, so it is not reported.
pub fn is_depth_stop_reason(reason: &str) -> bool {
    reason == depth_stop_reason()
}

/// Maximum number of entries retained across the whole walk.
///
/// This bounds one **listing**, which is one directory scope. It is not a cap
/// on the size of a repository: browsing past it is `l`, which re-walks a
/// subtree against this same budget. The number is unchanged from C002
/// deliberately — with [`MAX_LISTING_DEPTH`] bounding the embedding, the
/// retained count is dominated by the breadth of the listed scope rather than
/// the depth of the workspace, so raising it would buy payload without buying
/// reach.
pub const MAX_LISTING_NODES: usize = 4_096;

/// Maximum number of directory entries *examined* across the whole walk,
/// counted before any filtering and independent of [`MAX_LISTING_NODES`].
pub const MAX_LISTING_EXAMINED: usize = 100_000;

/// Maximum byte length of a single entry name. Longer names are skipped
/// rather than truncated.
pub const MAX_LISTING_NAME_BYTES: usize = 255;

/// Directories never descended into. Mirrors the composer's `@`
/// file-mention indexer.
///
/// `.codegg` is this application's own per-project runtime state — sessions,
/// tool-program results, run records, python traces — and it lives *inside* the
/// tree it describes. In a CodeGG checkout it was the single largest entry in
/// the workspace (21,482 entries) and, being second alphabetically, it consumed
/// the entire retained-node budget before any source directory was walked. The
/// other walkers in this repository already skipped it
/// (`python_script::snapshot`, `python_script::executor`,
/// `research::sources::local_repo`); this list had drifted away from them.
pub const LISTING_IGNORED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    ".cargo",
    "dist",
    "build",
    ".codegg",
];

/// Result of one bounded walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryListing {
    /// Top-level nodes, directories before files, then lexically.
    pub nodes: Vec<WorkspaceDirectoryNodeDto>,
    /// True when a bound stopped the walk.
    pub truncated: bool,
    /// Human-readable reason for truncation, when truncated.
    pub truncated_reason: Option<String>,
    /// Entries examined, including filtered-out ones.
    pub examined: usize,
}

/// Walk `target` into a bounded listing, exiting early when `cancel` is set.
///
/// `relative_base` is the workspace-relative path of `target` itself
/// (`""` for the workspace root, or a validated `prefix` for a subtree
/// listing). Child paths are built from validated component names, so they
/// can never contain a `..` component.
pub fn walk_directory(target: &Path, relative_base: &str, cancel: &AtomicBool) -> DirectoryListing {
    let mut listing = DirectoryListing::default();
    let mut budget = MAX_LISTING_NODES;
    let mut examined = 0usize;
    if budget == 0 {
        listing.truncated = true;
        listing.truncated_reason = Some("node budget is zero".to_string());
        return listing;
    }
    walk(
        target,
        relative_base,
        0,
        &mut budget,
        &mut examined,
        MAX_LISTING_EXAMINED,
        cancel,
        &mut listing,
    );
    listing.examined = examined;
    listing.nodes.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.name.cmp(&right.name))
    });
    listing
}

/// Account for one iterated directory entry (including iterator `Err`s).
///
/// Every item yielded by `read_dir` — `Ok` or `Err` — consumes iteration
/// work and must observe cancellation and both budgets. Returns `false`
/// when the walk must stop (truncation reason already recorded).
pub(crate) fn account_one_entry(
    cancel: &AtomicBool,
    examined: &mut usize,
    max_examined: usize,
    budget: usize,
    listing: &mut DirectoryListing,
) -> bool {
    if cancel.load(Ordering::Relaxed) {
        listing.truncated = true;
        listing.truncated_reason = Some("tree walk cancelled".to_string());
        return false;
    }
    *examined = examined.saturating_add(1);
    if *examined > max_examined {
        listing.truncated = true;
        listing.truncated_reason = Some(format!(
            "stopped after examining the maximum of {max_examined} entries"
        ));
        return false;
    }
    if budget == 0 {
        listing.truncated = true;
        listing.truncated_reason = Some(format!(
            "stopped at the maximum of {MAX_LISTING_NODES} entries"
        ));
        return false;
    }
    true
}

/// Test hook: walk with explicit budgets, bypassing the production
/// constants so cap branches are reachable without creating 100k files.
#[cfg(test)]
pub(crate) fn walk_with_budgets(
    target: &Path,
    relative_base: &str,
    max_nodes: usize,
    max_examined: usize,
    cancel: &AtomicBool,
) -> DirectoryListing {
    let mut listing = DirectoryListing::default();
    let mut budget = max_nodes;
    let mut examined = 0usize;
    walk(
        target,
        relative_base,
        0,
        &mut budget,
        &mut examined,
        max_examined,
        cancel,
        &mut listing,
    );
    listing.examined = examined;
    listing
}

fn child_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    directory: &Path,
    relative: &str,
    depth: usize,
    budget: &mut usize,
    examined: &mut usize,
    max_examined: usize,
    cancel: &AtomicBool,
    listing: &mut DirectoryListing,
) {
    if cancel.load(Ordering::Relaxed) {
        listing.truncated = true;
        listing.truncated_reason = Some("tree walk cancelled".to_string());
        return;
    }
    if depth >= MAX_LISTING_DEPTH {
        listing.truncated = true;
        listing.truncated_reason = Some(depth_stop_reason());
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

    // Do not use `entries.flatten()`. An `Err` item still consumed
    // iteration work (a directory entry that could not be read), so it must
    // consume the examined budget and observe cancellation, or a long stream
    // of errors bypasses both bounds. Count first, then skip.
    for next in entries {
        // Examined before any filtering: ignored, symlink, special,
        // unreadable, and iterator-error entries all cost scanning even
        // though they retain nothing.
        //
        // C005 fix: a bound hit here must `break`, not `return`. Returning
        // skips the flush loops below and blanks the entries retained so
        // far — a node-budget truncation always presented an empty tree.
        // Breaking keeps everything retained up to the bound alongside the
        // explicit truncation reason.
        if !account_one_entry(cancel, examined, max_examined, *budget, listing) {
            break;
        }
        let Ok(entry) = next else {
            // The entry could not be read (concurrent removal, I/O error).
            // Counted above; skipped without retaining anything.
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name.len() > MAX_LISTING_NAME_BYTES || name.contains('\0') {
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
            if LISTING_IGNORED_DIRS.contains(&name.as_str()) {
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
        // Same C005 flush rule as above: stop descending, but still emit
        // the siblings retained so far with the truncation reason.
        if cancel.load(Ordering::Relaxed) {
            listing.truncated = true;
            listing.truncated_reason = Some("tree walk cancelled".to_string());
            break;
        }
        let child_relative = child_path(relative, &name);
        let mut child_listing = DirectoryListing::default();
        walk(
            &path,
            &child_relative,
            depth + 1,
            budget,
            examined,
            max_examined,
            cancel,
            &mut child_listing,
        );
        if child_listing.truncated {
            listing.truncated = true;
            listing.truncated_reason = child_listing.truncated_reason;
        }
        listing.nodes.push(WorkspaceDirectoryNodeDto {
            name,
            path: child_relative,
            depth: depth + 1,
            kind: WorkspaceDirectoryNodeKindDto::Directory,
            children: Some(child_listing.nodes),
        });
    }

    for (name, _path) in files {
        listing.nodes.push(WorkspaceDirectoryNodeDto {
            path: child_path(relative, &name),
            name,
            depth: depth + 1,
            kind: WorkspaceDirectoryNodeKindDto::File,
            children: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(root: &Path, relative: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, b"x").expect("write");
    }

    #[test]
    fn subtree_walk_prefixes_paths_with_the_relative_base() {
        let root = temp();
        write(root.path(), "sub/inner.txt");
        write(root.path(), "top.txt");
        let cancel = AtomicBool::new(false);
        let listing = walk_directory(&root.path().join("sub"), "sub", &cancel);
        assert!(!listing.truncated);
        assert_eq!(listing.nodes.len(), 1);
        let node = &listing.nodes[0];
        assert_eq!(node.path, "sub/inner.txt");
        assert_eq!(node.depth, 1);
        assert_eq!(node.kind, WorkspaceDirectoryNodeKindDto::File);
    }

    #[test]
    fn root_walk_matches_the_historical_ordering_contract() {
        let root = temp();
        write(root.path(), "zeta.txt");
        write(root.path(), "alpha.txt");
        write(root.path(), "zdir/inner.txt");
        write(root.path(), "adir/inner.txt");
        let cancel = AtomicBool::new(false);
        let listing = walk_directory(root.path(), "", &cancel);
        let names: Vec<&str> = listing.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["adir", "zdir", "alpha.txt", "zeta.txt"]);
    }

    #[test]
    fn cancellation_is_observed_before_any_io() {
        let root = temp();
        write(root.path(), "a.txt");
        let cancel = AtomicBool::new(true);
        let listing = walk_directory(root.path(), "", &cancel);
        assert!(listing.truncated);
        assert_eq!(
            listing.truncated_reason.as_deref(),
            Some("tree walk cancelled")
        );
    }
}

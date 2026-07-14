//! Conflict model for in-progress repository operations.
//!
//! Phase F introduces a typed `ConflictEntry` representation of unmerged
//! paths so that tools, TUI, and projection can reason about them without
//! parsing raw git output. This module does NOT auto-resolve conflicts;
//! editing agents are expected to update the worktree file, mark paths as
//! resolved via `git add <path>`, then continue the underlying operation.

use serde::{Deserialize, Serialize};

/// Kind of conflict status reported by `git status` porcelain codes.
///
/// Names reflect the ordering on the X/Y axis (X = index/ours,
/// Y = worktree/theirs) per `git status --porcelain=v2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictKind {
    /// `UU` — both sides modified.
    BothModified,
    /// `AA` — both sides added.
    BothAdded,
    /// `DD` — both sides deleted.
    BothDeleted,
    /// `AU` — added by us, modified by theirs.
    AddedByUs,
    /// `UA` — modified by us, added by theirs.
    AddedByTheirs,
    /// `DU` — deleted by us, modified by theirs.
    DeletedByUs,
    /// `UD` — modified by us, deleted by theirs.
    DeletedByTheirs,
    /// Unrecognized XY code.
    Unknown,
}

impl ConflictKind {
    /// One-line description suitable for projection.
    pub fn describe(&self) -> &'static str {
        match self {
            Self::BothModified => "both modified",
            Self::BothAdded => "both added",
            Self::BothDeleted => "both deleted",
            Self::AddedByUs => "added by us",
            Self::AddedByTheirs => "added by theirs",
            Self::DeletedByUs => "deleted by us",
            Self::DeletedByTheirs => "deleted by theirs",
            Self::Unknown => "unrecognized",
        }
    }

    /// Whether resolution is likely to require manual content editing.
    pub fn requires_manual_edit(&self) -> bool {
        matches!(
            self,
            Self::BothModified | Self::AddedByTheirs | Self::DeletedByUs
        )
    }
}

/// Categorical shape describing how two sides of a conflict related.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictShape {
    /// Two ordinary files with overlapping content or rename to a target.
    File,
    /// Path renamed in our version (old path known).
    Rename,
    /// Path deleted in one side and modified/added in the other.
    Delete,
    /// One side replaced this path with a directory.
    DirectoryReplacement,
    /// Submodule pointer conflict.
    Submodule,
    /// Path no longer in original layout (e.g. file replaced by module).
    UntrackedReplaced,
}

impl ConflictShape {
    pub fn describe(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::DirectoryReplacement => "directory replacement",
            Self::Submodule => "submodule",
            Self::UntrackedReplaced => "untracked replaces tracked",
        }
    }
}

/// Object id (SHA) for one side of a conflict. Empty when no blob/tree id
/// was recorded (e.g. deletes or worktree-side adds).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictObjectId {
    /// SHA1 hex of the blob/tree reference. None when missing.
    pub sha: Option<String>,
    /// File mode (100644, 100755, 040000, 120000). None when missing.
    pub mode: Option<String>,
}

impl ConflictObjectId {
    pub fn present(sha: impl Into<String>, mode: impl Into<String>) -> Self {
        Self {
            sha: Some(sha.into()),
            mode: Some(mode.into()),
        }
    }

    pub fn absent() -> Self {
        Self {
            sha: None,
            mode: None,
        }
    }

    pub fn is_present(&self) -> bool {
        self.sha.is_some()
    }
}

/// Typed representation of one conflicted path discovered via
/// `git status` porcelain output and (optionally) `git ls-files -u`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictEntry {
    /// Repository-relative path (relative to the repository root).
    pub path: String,
    /// Status code (XY) reported by git.
    pub status_code: String,
    /// Classified conflict kind.
    pub kind: ConflictKind,
    /// Shape describing how the two sides related.
    pub shape: ConflictShape,
    /// Base blob/tree information (the common ancestor). None when
    /// the path did not exist before the conflicting operation.
    pub base: ConflictObjectId,
    /// Our/local blob/tree information.
    pub ours: ConflictObjectId,
    /// Their/remote blob/tree information.
    pub theirs: ConflictObjectId,
    /// Original path before rename (only when `shape = Rename`).
    pub original_path: Option<String>,
    /// Whether the worktree file contains conflict markers
    /// (`<<<<<<<`, `=======`, `>>>>>>>`).
    pub has_conflict_markers: bool,
    /// Whether the path is currently staged as resolved (i.e.
    /// appears in `git status` only on the staged side after
    /// `git add <path>`).
    pub staged_resolved: bool,
    /// Whether this conflict involves a submodule pointer.
    pub submodule: bool,
    /// Recommended next legal operations from a tooling perspective.
    pub recommended_actions: Vec<RecommendedConflictAction>,
}

/// Recommended legal operations for a conflicted path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum RecommendedConflictAction {
    /// Use the file-editing tool to rewrite the worktree file content.
    EditMarkers,
    /// Run `git add <path>` after manual edits.
    StageResolution,
    /// Run `git checkout --ours <path>` to take the local version.
    TakeOurs,
    /// Run `git checkout --theirs <path>` to take the remote version.
    TakeTheirs,
    /// Run `git rm <path>` for delete/delete conflicts.
    RemovePath,
    /// Run `git add -u <path>` if path will be kept in one form.
    UpdateIndex,
}

impl RecommendedConflictAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::EditMarkers => "edit markers",
            Self::StageResolution => "git add <path>",
            Self::TakeOurs => "git checkout --ours <path>",
            Self::TakeTheirs => "git checkout --theirs <path>",
            Self::RemovePath => "git rm <path>",
            Self::UpdateIndex => "git add -u <path>",
        }
    }

    /// Whether the action is destructive (mutates the index/worktree
    /// without manual content editing).
    pub fn is_destructive(&self) -> bool {
        matches!(
            self,
            Self::TakeOurs | Self::TakeTheirs | Self::RemovePath | Self::UpdateIndex
        )
    }
}

/// Aggregate conflict model for an active operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictReport {
    /// All conflicted paths.
    pub entries: Vec<ConflictEntry>,
    /// Total count (matches `entries.len()`).
    pub total: usize,
    /// Count of paths that contain conflict markers in the worktree.
    pub unmerged_with_markers: usize,
    /// Count of paths staged as resolved (post-`git add`).
    pub resolved: usize,
    /// Count of submodule conflicts.
    pub submodule_conflicts: usize,
}

impl ConflictReport {
    pub fn from_entries(entries: Vec<ConflictEntry>) -> Self {
        let unmerged_with_markers = entries
            .iter()
            .filter(|e| e.has_conflict_markers && !e.staged_resolved)
            .count();
        let resolved = entries.iter().filter(|e| e.staged_resolved).count();
        let submodule_conflicts = entries.iter().filter(|e| e.submodule).count();
        let total = entries.len();
        Self {
            entries,
            total,
            unmerged_with_markers,
            resolved,
            submodule_conflicts,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True if every conflict has been staged as resolved.
    pub fn all_resolved(&self) -> bool {
        !self.entries.is_empty() && self.resolved == self.entries.len()
    }
}

/// Map a porcelain v2 XY code to a typed conflict kind.
pub fn classify_conflict_code(xy: &str) -> ConflictKind {
    if xy.len() < 2 {
        return ConflictKind::Unknown;
    }
    let bytes = xy.as_bytes();
    let x = bytes[0];
    let y = bytes[1];
    match (x, y) {
        (b'U', b'U') => ConflictKind::BothModified,
        (b'A', b'A') => ConflictKind::BothAdded,
        (b'D', b'D') => ConflictKind::BothDeleted,
        (b'A', b'U') => ConflictKind::AddedByUs,
        (b'U', b'A') => ConflictKind::AddedByTheirs,
        (b'D', b'U') => ConflictKind::DeletedByUs,
        (b'U', b'D') => ConflictKind::DeletedByTheirs,
        _ => ConflictKind::Unknown,
    }
}

/// Best-effort detection of conflict markers in a UTF-8 text payload.
/// We deliberately only look at the full markers (`<<<<<<<`, `=======`,
/// `>>>>>>>`) — comments and unrelated lines containing `<` or `>` are
/// not flagged.
pub fn buffer_contains_conflict_markers(content: &str) -> bool {
    content.contains("<<<<<<<") && content.contains("=======") && content.contains(">>>>>>>")
}

/// Test whether a file looks binary (and therefore unlikely to contain
/// textual conflict markers). Uses NUL-byte heuristic — line-based binary
/// detection would be heavier and is not required here.
pub fn looks_binary(content: &[u8]) -> bool {
    content.contains(&0)
}

/// Recommended action set for a given conflict kind. Conservative —
/// editing agents should still inspect the actual markers before
/// deciding on `take ours` / `take theirs`.
pub fn default_actions_for(
    kind: ConflictKind,
    shape: ConflictShape,
) -> Vec<RecommendedConflictAction> {
    let mut out = Vec::new();
    match (kind, shape) {
        (ConflictKind::BothDeleted, _) => {
            out.push(RecommendedConflictAction::RemovePath);
        }
        (ConflictKind::BothAdded, ConflictShape::Submodule)
        | (ConflictKind::BothModified, ConflictShape::Submodule) => {
            out.push(RecommendedConflictAction::EditMarkers);
            out.push(RecommendedConflictAction::StageResolution);
            out.push(RecommendedConflictAction::TakeOurs);
            out.push(RecommendedConflictAction::TakeTheirs);
        }
        (ConflictKind::BothAdded, _) | (_, ConflictShape::File) | (_, ConflictShape::Rename) => {
            out.push(RecommendedConflictAction::EditMarkers);
            out.push(RecommendedConflictAction::StageResolution);
            out.push(RecommendedConflictAction::TakeOurs);
            out.push(RecommendedConflictAction::TakeTheirs);
        }
        (kind, ConflictShape::Delete) => {
            let _ = kind;
            out.push(RecommendedConflictAction::TakeOurs);
            out.push(RecommendedConflictAction::TakeTheirs);
            out.push(RecommendedConflictAction::RemovePath);
        }
        (ConflictKind::AddedByUs, _) | (ConflictKind::AddedByTheirs, _) => {
            out.push(RecommendedConflictAction::TakeOurs);
            out.push(RecommendedConflictAction::TakeTheirs);
            out.push(RecommendedConflictAction::RemovePath);
            out.push(RecommendedConflictAction::EditMarkers);
            out.push(RecommendedConflictAction::StageResolution);
        }
        (ConflictKind::DeletedByUs, _) | (ConflictKind::DeletedByTheirs, _) => {
            out.push(RecommendedConflictAction::TakeOurs);
            out.push(RecommendedConflictAction::TakeTheirs);
            out.push(RecommendedConflictAction::RemovePath);
        }
        _ => {
            out.push(RecommendedConflictAction::EditMarkers);
            out.push(RecommendedConflictAction::StageResolution);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_conflict_codes() {
        assert_eq!(classify_conflict_code("UU"), ConflictKind::BothModified);
        assert_eq!(classify_conflict_code("AA"), ConflictKind::BothAdded);
        assert_eq!(classify_conflict_code("DD"), ConflictKind::BothDeleted);
        assert_eq!(classify_conflict_code("AU"), ConflictKind::AddedByUs);
        assert_eq!(classify_conflict_code("UA"), ConflictKind::AddedByTheirs);
        assert_eq!(classify_conflict_code("DU"), ConflictKind::DeletedByUs);
        assert_eq!(classify_conflict_code("UD"), ConflictKind::DeletedByTheirs);
        assert_eq!(classify_conflict_code("??"), ConflictKind::Unknown);
    }

    #[test]
    fn detects_conflict_markers() {
        let text = "line one\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n";
        assert!(buffer_contains_conflict_markers(text));
    }

    #[test]
    fn non_conflict_text_returns_false() {
        let text = "<<<<<<< not a conflict\njust some text\n";
        assert!(!buffer_contains_conflict_markers(text));
    }

    #[test]
    fn binary_detection_uses_nul_byte() {
        assert!(looks_binary(b"abc\0def"));
        assert!(!looks_binary(b"abcdef"));
    }

    #[test]
    fn default_actions_for_both_modified_includes_markers() {
        let actions = default_actions_for(ConflictKind::BothModified, ConflictShape::File);
        assert!(actions.contains(&RecommendedConflictAction::EditMarkers));
        assert!(actions.contains(&RecommendedConflictAction::StageResolution));
    }

    #[test]
    fn default_actions_for_both_deleted_is_remove() {
        let actions = default_actions_for(ConflictKind::BothDeleted, ConflictShape::File);
        assert_eq!(actions, vec![RecommendedConflictAction::RemovePath]);
    }

    #[test]
    fn conflict_report_aggregates() {
        let entries = vec![
            ConflictEntry {
                path: "a".into(),
                status_code: "UU".into(),
                kind: ConflictKind::BothModified,
                shape: ConflictShape::File,
                base: ConflictObjectId::absent(),
                ours: ConflictObjectId::absent(),
                theirs: ConflictObjectId::absent(),
                original_path: None,
                has_conflict_markers: true,
                staged_resolved: false,
                submodule: false,
                recommended_actions: vec![],
            },
            ConflictEntry {
                path: "b".into(),
                status_code: "AA".into(),
                kind: ConflictKind::BothAdded,
                shape: ConflictShape::Submodule,
                base: ConflictObjectId::absent(),
                ours: ConflictObjectId::absent(),
                theirs: ConflictObjectId::absent(),
                original_path: None,
                has_conflict_markers: false,
                staged_resolved: true,
                submodule: true,
                recommended_actions: vec![],
            },
        ];
        let report = ConflictReport::from_entries(entries);
        assert_eq!(report.total, 2);
        assert_eq!(report.unmerged_with_markers, 1);
        assert_eq!(report.resolved, 1);
        assert_eq!(report.submodule_conflicts, 1);
    }
}

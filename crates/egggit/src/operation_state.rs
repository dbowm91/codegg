//! Repository operation-state discovery.
//!
//! Phase F extends `egggit` with a structured model for in-progress
//! repository operations (merge, rebase, cherry-pick, revert, bisect,
//! sequencer-driven operations, and apply-mailbox).
//!
//! Each variant carries the origin, current step, target refs, conflicted
//! paths, and recommended legal actions where safely discoverable from
//! Git plumbing. When filesystem sentinel files are version-sensitive we
//! isolate that logic in testable helpers and expose sanitized state-path
//! diagnostics without leaking internal layout.

use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Operation families recognized by the structured state model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum OperationFamily {
    /// No active operation; clean or normal working tree.
    None,
    /// `git merge` (or `git pull` in merge mode) in progress.
    Merge,
    /// `git rebase` (interactive or scripted) in progress.
    Rebase,
    /// `git cherry-pick` sequence in progress.
    CherryPick,
    /// `git revert` sequence in progress.
    Revert,
    /// `git bisect` in progress.
    Bisect,
    /// `git am` (apply mailbox) in progress.
    ApplyMailbox,
    /// `git revert` or `git cherry-pick` driven by the sequencer.
    Sequencer,
    /// Recognized sentinel file but not modeled above (e.g. older Git layouts).
    Unknown,
}

impl OperationFamily {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::CherryPick => "cherry-pick",
            Self::Revert => "revert",
            Self::Bisect => "bisect",
            Self::ApplyMailbox => "apply-mailbox",
            Self::Sequencer => "sequencer",
            Self::Unknown => "unknown",
        }
    }

    /// Whether `continue` is a legal Git-supported action.
    pub fn supports_continue(&self) -> bool {
        matches!(
            self,
            Self::Merge
                | Self::Rebase
                | Self::CherryPick
                | Self::Revert
                | Self::ApplyMailbox
                | Self::Sequencer
        )
    }

    /// Whether `abort` is a legal Git-supported action.
    pub fn supports_abort(&self) -> bool {
        matches!(
            self,
            Self::Merge
                | Self::Rebase
                | Self::CherryPick
                | Self::Revert
                | Self::ApplyMailbox
                | Self::Sequencer
        )
    }

    /// Whether `skip` is a legal Git-supported action.
    pub fn supports_skip(&self) -> bool {
        matches!(self, Self::Rebase | Self::CherryPick | Self::Revert)
    }
}

/// Recovery actions that are legal against a particular operation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RecoveryAction {
    Continue,
    Abort,
    Skip,
}

impl RecoveryAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Abort => "abort",
            Self::Skip => "skip",
        }
    }
}

/// Typed state for a multi-step operation (cherry-pick, revert).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SequenceState {
    /// Original HEAD before the operation started (for diagnostics).
    pub original_head: Option<String>,
    /// Current HEAD (may differ from `original_head` after each step).
    pub current_head: Option<String>,
    /// Target being cherry-picked/reverted, if discoverable.
    pub target: Option<String>,
    /// Sequential steps still pending (best-effort, may not be available).
    pub pending_steps: Vec<String>,
    /// Whether the operation completed cleanly (no further steps).
    pub finished: bool,
}

/// Typed state for an in-progress rebase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RebaseState {
    /// Original HEAD before the operation started.
    pub original_head: Option<String>,
    /// Current HEAD (advances after each successful pick).
    pub current_head: Option<String>,
    /// Upstream (where commits are being rebased onto).
    pub upstream: Option<String>,
    /// Branch under rewrite (often HEAD before the rebase).
    pub onto_branch: Option<String>,
    /// 1-based index of the next step (`done`/`msgnum` files).
    pub current_step: Option<u32>,
    /// Total number of steps when safely discoverable.
    pub total_steps: Option<u32>,
    /// Whether the rebase is interactive (`git rebase -i`).
    pub interactive: bool,
    /// Whether `rebase-apply` (non-interactive) is in use.
    pub apply_mode: bool,
}

/// Typed state for an in-progress merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MergeState {
    /// HEAD before the merge started.
    pub original_head: Option<String>,
    /// Branch being merged in (from `MERGE_HEAD`).
    pub other_head: Option<String>,
    /// Optional merge message (from `MERGE_MSG`).
    pub message: Option<String>,
    /// Whether the merge is still in progress with conflicts.
    pub in_progress: bool,
}

/// Typed state for an active `git bisect` run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BisectState {
    /// Currently testing commit (from `BISECT_LOG`).
    pub current: Option<String>,
    /// Bad boundary (last known-bad commit).
    pub bad: Option<String>,
    /// Good boundary (last known-good commit).
    pub good: Option<String>,
    /// Number of revisions left to test (best-effort).
    pub remaining: Option<u32>,
}

/// Typed state for an in-progress `git am`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ApplyState {
    /// Current HEAD before the apply.
    pub original_head: Option<String>,
    /// Current step (from `next` file).
    pub current_step: Option<u32>,
    /// Last applied commit (from `last` file).
    pub last_applied: Option<String>,
    /// Total number of steps when safely discoverable.
    pub total_steps: Option<u32>,
}

/// Typed state for sequencer-driven operations (cherry-pick/revert).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SequencerState {
    /// Original HEAD before the sequence started.
    pub original_head: Option<String>,
    /// Current HEAD as the sequencer advances.
    pub current_head: Option<String>,
    /// `action` field from `sequencer/todo` (pick/revert/...).
    pub action: Option<String>,
    /// Subject line from `sequencer/todo`.
    pub subject: Option<String>,
    /// 1-based index of the next step.
    pub current_step: Option<u32>,
    /// Total number of steps when discoverable.
    pub total_steps: Option<u32>,
}

/// Sanitized diagnostics for a recognized-but-unmodeled state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnknownOperationState {
    /// Sentinel filename observed (e.g. `REBASE_HEAD`).
    pub sentinel: String,
    /// Sanitized hint message (no internal path leaking).
    pub hint: String,
}

/// Structured representation of an in-progress repository operation.
///
/// `None` indicates a clean repository (or one without sentinel files).
/// All other variants carry actionable structure suitable for tool
/// prompts, projections, and TUI surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RepositoryOperationState {
    /// No active operation.
    #[default]
    None,
    /// `git merge` in progress.
    Merge(MergeState),
    /// `git rebase` in progress.
    Rebase(RebaseState),
    /// `git cherry-pick` sequence in progress.
    CherryPick(SequenceState),
    /// `git revert` sequence in progress.
    Revert(SequenceState),
    /// `git bisect` in progress.
    Bisect(BisectState),
    /// `git am` (apply mailbox) in progress.
    ApplyMailbox(ApplyState),
    /// Sequencer-driven operation in progress.
    Sequencer(SequencerState),
    /// Recognized sentinel without an internal model.
    Unknown(UnknownOperationState),
}

impl RepositoryOperationState {
    /// `true` when no operation is in progress (clean repository).
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Operation family for routing/UI.
    pub fn family(&self) -> OperationFamily {
        match self {
            Self::None => OperationFamily::None,
            Self::Merge(_) => OperationFamily::Merge,
            Self::Rebase(_) => OperationFamily::Rebase,
            Self::CherryPick(_) => OperationFamily::CherryPick,
            Self::Revert(_) => OperationFamily::Revert,
            Self::Bisect(_) => OperationFamily::Bisect,
            Self::ApplyMailbox(_) => OperationFamily::ApplyMailbox,
            Self::Sequencer(_) => OperationFamily::Sequencer,
            Self::Unknown(_) => OperationFamily::Unknown,
        }
    }

    /// Operation type label suitable for UI.
    pub fn label(&self) -> &'static str {
        self.family().label()
    }

    /// Recovery actions Git legally supports against this state.
    pub fn available_actions(&self) -> Vec<RecoveryAction> {
        let family = self.family();
        let mut out = Vec::new();
        if family.supports_continue() {
            out.push(RecoveryAction::Continue);
        }
        if family.supports_abort() {
            out.push(RecoveryAction::Abort);
        }
        if family.supports_skip() {
            out.push(RecoveryAction::Skip);
        }
        out
    }

    /// Whether `action` is a legal recovery action against this state.
    pub fn action_available(&self, action: RecoveryAction) -> bool {
        self.available_actions().contains(&action)
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_optional_u32(path: &Path) -> Option<u32> {
    read_trimmed(path).and_then(|s| s.parse().ok())
}

fn head_short(value: &str) -> String {
    if value.len() >= 7 {
        value[..7].to_string()
    } else {
        value.to_string()
    }
}

/// Detect operation state from `.git/` plumbing files. Pure inspection;
/// makes no mutation. Returns `RepositoryOperationState::None` when no
/// sentinel is present, or `RepositoryOperationState::Unknown` with a
/// sanitized hint for unrecognized sentinels.
pub fn detect_repository_operation_state(git_dir: &Path) -> RepositoryOperationState {
    let git_dir = git_dir.to_path_buf();

    // Sequencer sentinel: present whenever cherry-pick or revert is active
    // under newer Git (≥2.25). Always takes priority over the legacy
    // CHERRY_PICK_HEAD / REVERT_HEAD sentinels.
    let sequencer_todo = git_dir.join("sequencer").join("todo");
    if sequencer_todo.exists() {
        let head_short = read_trimmed(&git_dir.join("HEAD"));
        let original_head = read_trimmed(&git_dir.join("ORIG_HEAD"));
        let todo_content = std::fs::read_to_string(&sequencer_todo).ok();
        let todo_first_line = todo_content
            .as_ref()
            .and_then(|t| t.lines().next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let (action, subject) = match todo_first_line.as_deref() {
            Some(line) => {
                let (act, subj) = if let Some(rest) = line.strip_prefix("pick ") {
                    ("pick", rest.to_string())
                } else if let Some(rest) = line.strip_prefix("revert ") {
                    ("revert", rest.to_string())
                } else {
                    (
                        line.split_whitespace().next().unwrap_or("pick"),
                        String::new(),
                    )
                };
                (
                    Some(act.to_string()),
                    if subj.is_empty() { None } else { Some(subj) },
                )
            }
            None => (None, None),
        };
        return RepositoryOperationState::Sequencer(SequencerState {
            original_head,
            current_head: head_short,
            action,
            subject,
            current_step: None,
            total_steps: todo_content
                .as_ref()
                .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count() as u32),
        });
    }

    // Legacy cherry-pick HEAD (overlap with sequencer above, but older git).
    if git_dir.join("CHERRY_PICK_HEAD").exists() {
        let head = read_trimmed(&git_dir.join("HEAD"));
        let original_head = read_trimmed(&git_dir.join("ORIG_HEAD"));
        return RepositoryOperationState::CherryPick(SequenceState {
            original_head,
            current_head: head,
            target: read_trimmed(&git_dir.join("CHERRY_PICK_HEAD")),
            pending_steps: Vec::new(),
            finished: false,
        });
    }

    // Legacy revert HEAD.
    if git_dir.join("REVERT_HEAD").exists() {
        let head = read_trimmed(&git_dir.join("HEAD"));
        let original_head = read_trimmed(&git_dir.join("ORIG_HEAD"));
        return RepositoryOperationState::Revert(SequenceState {
            original_head,
            current_head: head,
            target: read_trimmed(&git_dir.join("REVERT_HEAD")),
            pending_steps: Vec::new(),
            finished: false,
        });
    }

    // Merge in progress.
    if git_dir.join("MERGE_HEAD").exists() {
        let _head = read_trimmed(&git_dir.join("HEAD"));
        let original_head = read_trimmed(&git_dir.join("ORIG_HEAD"));
        let other_head = read_trimmed(&git_dir.join("MERGE_HEAD"));
        let message = read_trimmed(&git_dir.join("MERGE_MSG"))
            .map(|mut s| {
                // MERGE_MSG contains a header and the message body. Strip the
                // "Merge branch '...'" header line for compactness.
                if let Some(idx) = s.find('\n') {
                    s = s[idx + 1..].to_string();
                }
                s
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        return RepositoryOperationState::Merge(MergeState {
            original_head,
            other_head,
            message,
            in_progress: true,
        });
    }

    // Rebase: prefer rebase-merge (interactive), fall back to rebase-apply.
    let rebase_merge = git_dir.join("rebase-merge");
    let rebase_apply = git_dir.join("rebase-apply");
    if rebase_merge.exists() {
        let head = read_trimmed(&git_dir.join("head-name"))
            .or_else(|| read_trimmed(&git_dir.join("HEAD")));
        let original_head = read_trimmed(&git_dir.join("ORIG_HEAD"));
        let upstream = read_trimmed(&rebase_merge.join("onto-name"));
        let onto_branch = read_trimmed(&rebase_merge.join("onto"));
        let current_step = read_optional_u32(&rebase_merge.join("msgnum"));
        let total_steps = std::fs::read_dir(&rebase_merge).ok().map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|s| s.starts_with("000") && s.len() == 3)
                        .unwrap_or(false)
                })
                .count() as u32
        });
        return RepositoryOperationState::Rebase(RebaseState {
            original_head,
            current_head: head.map(|h| head_short(&h)),
            upstream,
            onto_branch,
            current_step,
            total_steps,
            interactive: true,
            apply_mode: false,
        });
    }
    if rebase_apply.exists() {
        let head = read_trimmed(&git_dir.join("HEAD"));
        let original_head = read_trimmed(&git_dir.join("ORIG_HEAD"));
        let upstream = read_trimmed(&rebase_apply.join("onto-name"));
        let current_step = read_optional_u32(&rebase_apply.join("next"));
        let total_steps = std::fs::read_dir(&rebase_apply).ok().map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|s| s.starts_with("000") && s.len() == 3)
                        .unwrap_or(false)
                })
                .count() as u32
        });
        return RepositoryOperationState::Rebase(RebaseState {
            original_head,
            current_head: head.map(|h| head_short(&h)),
            upstream,
            onto_branch: None,
            current_step,
            total_steps,
            interactive: false,
            apply_mode: true,
        });
    }
    if git_dir.join("REBASE_HEAD").exists() {
        let head = read_trimmed(&git_dir.join("REBASE_HEAD"));
        let original_head = read_trimmed(&git_dir.join("ORIG_HEAD"));
        return RepositoryOperationState::Rebase(RebaseState {
            original_head,
            current_head: head.map(|h| head_short(&h)),
            upstream: None,
            onto_branch: None,
            current_step: None,
            total_steps: None,
            interactive: false,
            apply_mode: false,
        });
    }

    if git_dir.join("BISECT_LOG").exists() {
        let head = read_trimmed(&git_dir.join("HEAD"));
        let (bad, good) = parse_bisect_log(&git_dir);
        return RepositoryOperationState::Bisect(BisectState {
            current: head,
            bad,
            good,
            remaining: None,
        });
    }

    // Apply mailbox
    if git_dir.join("rebase-apply").join("next").exists()
        && git_dir.join("rebase-apply").join("last").exists()
        && !git_dir.join("rebase-apply").join("head-name").exists()
    {
        let last_applied = read_trimmed(&git_dir.join("rebase-apply").join("last"));
        let current_step = read_optional_u32(&git_dir.join("rebase-apply").join("next"));
        let total_steps = std::fs::read_dir(git_dir.join("rebase-apply"))
            .ok()
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_name()
                            .to_str()
                            .map(|s| s.starts_with("000") && s.len() == 3)
                            .unwrap_or(false)
                    })
                    .count() as u32
            });
        return RepositoryOperationState::ApplyMailbox(ApplyState {
            original_head: read_trimmed(&git_dir.join("ORIG_HEAD")),
            current_step,
            last_applied,
            total_steps,
        });
    }

    RepositoryOperationState::None
}

/// Parse `BISECT_LOG` for last-known bad/good commit boundaries.
fn parse_bisect_log(git_dir: &Path) -> (Option<String>, Option<String>) {
    let path = git_dir.join("BISECT_LOG");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return (None, None);
    };
    let mut bad: Option<String> = None;
    let mut good: Option<String> = None;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("git bisect bad ") {
            bad = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("git bisect good ") {
            good = Some(rest.trim().to_string());
        }
    }
    (bad, good)
}

/// Convenience: detect operation state for a given worktree path.
///
/// Resolves `.git` (file or directory) under the root. Returns
/// `EgggitError::NotARepository` when no `.git` is present.
pub fn detect_operation_state_for_root(
    root: &Path,
) -> Result<RepositoryOperationState, EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }
    let git_dir = resolve_git_dir(root)
        .ok_or_else(|| EgggitError::NotARepository(root.display().to_string()))?;
    Ok(detect_repository_operation_state(&git_dir))
}

/// Resolve the canonical `.git` directory for `root`. Handles the case
/// where `.git` is a file pointing into a linked worktree.
fn resolve_git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    if dot_git.is_file() {
        if let Ok(content) = std::fs::read_to_string(&dot_git) {
            let line = content.trim();
            if let Some(rest) = line.strip_prefix("gitdir: ") {
                let rest = rest.trim();
                // Absolute path
                let p = Path::new(rest);
                if p.is_absolute() {
                    return Some(p.to_path_buf());
                }
                // Relative to root
                return Some(root.join(rest));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_dir(dir: &Path) -> PathBuf {
        dir.join(".git")
    }

    fn write(path: &Path, value: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, value).unwrap();
    }

    #[test]
    fn none_when_no_sentinel() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = detect_repository_operation_state(&git_dir(dir.path()));
        assert_eq!(state, RepositoryOperationState::None);
        assert!(state.is_clean());
    }

    #[test]
    fn detects_merge() {
        let dir = tempfile::TempDir::new().unwrap();
        let gd = git_dir(dir.path());
        write(&gd.join("MERGE_HEAD"), "abc123\n");
        write(
            &gd.join("MERGE_MSG"),
            "Merge branch 'feature'\n\nBody line\n",
        );
        let state = detect_repository_operation_state(&gd);
        match state {
            RepositoryOperationState::Merge(s) => {
                assert_eq!(s.other_head.as_deref(), Some("abc123"));
                assert_eq!(s.message.as_deref(), Some("Body line"));
                assert!(s.in_progress);
            }
            other => panic!("expected Merge, got {other:?}"),
        }
    }

    #[test]
    fn detects_cherry_pick() {
        let dir = tempfile::TempDir::new().unwrap();
        let gd = git_dir(dir.path());
        write(&gd.join("CHERRY_PICK_HEAD"), "deadbeef\n");
        let state = detect_repository_operation_state(&gd);
        assert_eq!(state.family(), OperationFamily::CherryPick);
        assert!(state.action_available(RecoveryAction::Continue));
        assert!(state.action_available(RecoveryAction::Abort));
        assert!(state.action_available(RecoveryAction::Skip));
    }

    #[test]
    fn detects_revert() {
        let dir = tempfile::TempDir::new().unwrap();
        let gd = git_dir(dir.path());
        write(&gd.join("REVERT_HEAD"), "feedface\n");
        let state = detect_repository_operation_state(&gd);
        assert_eq!(state.family(), OperationFamily::Revert);
        assert!(state.action_available(RecoveryAction::Continue));
        assert!(state.action_available(RecoveryAction::Abort));
        assert!(state.action_available(RecoveryAction::Skip));
    }

    #[test]
    fn detects_rebase_apply() {
        let dir = tempfile::TempDir::new().unwrap();
        let gd = git_dir(dir.path());
        let ra = gd.join("rebase-apply");
        std::fs::create_dir_all(&ra).unwrap();
        write(&ra.join("head-name"), "main\n");
        write(&ra.join("onto-name"), "origin/main\n");
        write(&ra.join("next"), "2\n");
        let state = detect_repository_operation_state(&gd);
        match state {
            RepositoryOperationState::Rebase(s) => {
                assert!(s.apply_mode);
                assert!(!s.interactive);
                assert_eq!(s.upstream.as_deref(), Some("origin/main"));
                assert_eq!(s.current_step, Some(2));
            }
            other => panic!("expected Rebase, got {other:?}"),
        }
    }

    #[test]
    fn detects_sequencer() {
        let dir = tempfile::TempDir::new().unwrap();
        let gd = git_dir(dir.path());
        let sq = gd.join("sequencer");
        std::fs::create_dir_all(&sq).unwrap();
        write(
            &sq.join("todo"),
            "pick abc1234 first commit subject\npick def5678 second commit subject\n",
        );
        let state = detect_repository_operation_state(&gd);
        match state {
            RepositoryOperationState::Sequencer(s) => {
                assert_eq!(s.action.as_deref(), Some("pick"));
                assert_eq!(s.subject.as_deref(), Some("abc1234 first commit subject"));
                assert_eq!(s.total_steps, Some(2));
            }
            other => panic!("expected Sequencer, got {other:?}"),
        }
    }

    #[test]
    fn detects_bisect() {
        let dir = tempfile::TempDir::new().unwrap();
        let gd = git_dir(dir.path());
        write(
            &gd.join("BISECT_LOG"),
            "git bisect start\n# bad: [abc1234] bad\ngit bisect bad abc1234\n# good: [def5678] good\ngit bisect good def5678\n",
        );
        let state = detect_repository_operation_state(&gd);
        match state {
            RepositoryOperationState::Bisect(s) => {
                assert_eq!(s.bad.as_deref(), Some("abc1234"));
                assert_eq!(s.good.as_deref(), Some("def5678"));
            }
            other => panic!("expected Bisect, got {other:?}"),
        }
    }

    #[test]
    fn available_actions_per_family() {
        assert!(!RepositoryOperationState::None.action_available(RecoveryAction::Continue));
        assert!(!RepositoryOperationState::None.action_available(RecoveryAction::Abort));
        assert!(!RepositoryOperationState::None.action_available(RecoveryAction::Skip));
        // Merge: continue, abort
        let s = RepositoryOperationState::Merge(MergeState::default());
        assert!(s.action_available(RecoveryAction::Continue));
        assert!(s.action_available(RecoveryAction::Abort));
        assert!(!s.action_available(RecoveryAction::Skip));
    }
}

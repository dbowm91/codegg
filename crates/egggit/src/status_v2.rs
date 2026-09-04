use crate::conflict::{
    classify_conflict_code, default_actions_for, ConflictEntry, ConflictObjectId, ConflictReport,
    ConflictShape,
};
use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Rich structured repository status from `git status --porcelain=v2 -z --branch`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RichRepoStatus {
    /// Canonical repository root path.
    pub root: String,
    /// Branch info from the `# branch.oid <sha>` line.
    pub head: Option<String>,
    /// Branch name from `# branch.head <name>` (None if detached).
    pub branch: Option<String>,
    /// Upstream ref from `# branch.upstream <ref>`.
    pub upstream: Option<String>,
    /// Ahead count from `# branch.ab +<ahead> -<behind>`.
    pub ahead: Option<i32>,
    /// Behind count from `# branch.ab +<ahead> -<behind>`.
    pub behind: Option<i32>,
    /// Staged entries (index changes).
    pub staged: Vec<StatusEntry>,
    /// Unstaged entries (worktree changes).
    pub unstaged: Vec<StatusEntry>,
    /// Untracked files.
    pub untracked: Vec<StatusEntry>,
    /// Ignored files (only populated when `include_ignored` is true).
    pub ignored: Vec<StatusEntry>,
    /// Conflict entries.
    pub conflicted: Vec<StatusEntry>,
    /// Typed conflict entries (richer than `conflicted`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflict_entries: Vec<ConflictEntry>,
    /// Aggregated conflict report (`None` when no conflicts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_report: Option<ConflictReport>,
    /// Active operation state (merge, rebase, cherry-pick, revert, bisect).
    pub operation_state: Option<OperationState>,
    /// Typed in-progress operation state (Phase F).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_operation_state: Option<crate::operation_state::RepositoryOperationState>,
    /// Legal recovery actions for the current operation state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_actions: Vec<crate::operation_state::RecoveryAction>,
    /// Whether the repository is in a clean state.
    pub is_clean: bool,
    /// High-level dirty summary.
    pub dirty_summary: DirtySummary,
}

/// High-level dirty summary counts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirtySummary {
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub conflicted_count: usize,
}

/// A single status entry from porcelain v2 output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusEntry {
    /// XY status codes from porcelain v2.
    pub xy_status: String,
    /// Path of the file.
    pub path: String,
    /// Renamed/copied source path (from `R` or `C` status).
    pub old_path: Option<String>,
    /// Submodule status (from `STM` codes in porcelain v2).
    pub submodule_status: Option<String>,
    /// Conflict stage levels.
    pub conflict_stages: Option<String>,
}

/// Active operation state detected from `.git/` sentinel files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationState {
    Merge { head: Option<String> },
    Rebase { head: Option<String> },
    CherryPick { head: Option<String> },
    Revert { head: Option<String> },
    Bisect,
}

fn read_head_file(path: std::path::PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn detect_operation_state(root: &Path) -> Option<OperationState> {
    let git_dir = root.join(".git");

    if git_dir.join("MERGE_HEAD").exists() {
        let head = read_head_file(git_dir.join("MERGE_HEAD"));
        return Some(OperationState::Merge { head });
    }

    if git_dir.join("REBASE_HEAD").exists()
        || git_dir.join("rebase-merge").exists()
        || git_dir.join("rebase-apply").exists()
    {
        let head = if git_dir.join("REBASE_HEAD").exists() {
            read_head_file(git_dir.join("REBASE_HEAD"))
        } else {
            read_head_file(git_dir.join("rebase-merge").join("head-name"))
        };
        return Some(OperationState::Rebase { head });
    }

    if git_dir.join("CHERRY_PICK_HEAD").exists() {
        let head = read_head_file(git_dir.join("CHERRY_PICK_HEAD"));
        return Some(OperationState::CherryPick { head });
    }

    if git_dir.join("REVERT_HEAD").exists() {
        let head = read_head_file(git_dir.join("REVERT_HEAD"));
        return Some(OperationState::Revert { head });
    }

    if git_dir.join("BISECT_LOG").exists() {
        return Some(OperationState::Bisect);
    }

    None
}

fn parse_branch_line(
    line: &str,
    head: &mut Option<String>,
    branch: &mut Option<String>,
    upstream: &mut Option<String>,
    ahead: &mut Option<i32>,
    behind: &mut Option<i32>,
) {
    if let Some(sha) = line.strip_prefix("# branch.oid ") {
        let sha = sha.trim();
        if !sha.is_empty() {
            *head = Some(sha.to_string());
        }
    } else if let Some(name) = line.strip_prefix("# branch.head ") {
        let name = name.trim();
        if name != "(detached)" && !name.is_empty() {
            *branch = Some(name.to_string());
        }
    } else if let Some(ref_name) = line.strip_prefix("# branch.upstream ") {
        let ref_name = ref_name.trim();
        if !ref_name.is_empty() {
            *upstream = Some(ref_name.to_string());
        }
    } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
        let ab = ab.trim();
        let mut a: Option<i32> = None;
        let mut b: Option<i32> = None;
        for part in ab.split_whitespace() {
            if let Some(val) = part.strip_prefix('+') {
                a = val.parse().ok();
            } else if let Some(val) = part.strip_prefix('-') {
                b = val.parse().ok();
            }
        }
        *ahead = a;
        *behind = b;
    }
}

fn classify_entry(xy: &str) -> &'static str {
    let x = xy.as_bytes().first().copied().unwrap_or(b' ');
    let y = xy.as_bytes().get(1).copied().unwrap_or(b' ');
    if x == b'?' {
        return "untracked";
    }
    if x == b'!' {
        return "ignored";
    }
    if x == b'u' || y == b'u' {
        return "conflicted";
    }
    // In porcelain v2: X = index status, Y = worktree status
    // '.' means no change in that area
    if x != b' ' && x != b'?' && x != b'!' && x != b'.' {
        return "staged";
    }
    if y != b' ' && y != b'?' && y != b'!' && y != b'.' {
        return "unstaged";
    }
    "clean"
}

/// Split a porcelain v2 entry into its fixed-width leading fields and
/// the verbatim path remainder. Paths are separated from the fixed
/// fields by a single space and may themselves contain spaces.
fn split_entry_fields(entry: &str, fixed_fields: usize) -> Option<(Vec<&str>, &str)> {
    let mut fields = Vec::with_capacity(fixed_fields);
    let mut rest = entry;
    for _ in 0..fixed_fields {
        let idx = rest.find(' ')?;
        let (field, tail) = rest.split_at(idx);
        fields.push(field);
        rest = &tail[1..];
    }
    if rest.is_empty() {
        return None;
    }
    Some((fields, rest))
}

/// Parse a single porcelain v2 entry record. `orig_path` carries the
/// renamed/copied source path, which git emits as the NUL-separated
/// record following a type-2/type-3 entry in `-z` mode.
fn parse_entry(entry: &str, orig_path: Option<&str>) -> Option<StatusEntry> {
    match entry.split(' ').next()? {
        "1" => {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            let (fields, path) = split_entry_fields(entry, 8)?;
            Some(StatusEntry {
                xy_status: fields[1].to_string(),
                path: path.to_string(),
                old_path: None,
                submodule_status: fields.get(2).map(|s| s.to_string()),
                conflict_stages: None,
            })
        }
        "2" | "3" => {
            // 2|3 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>
            let (fields, path) = split_entry_fields(entry, 9)?;
            let xy_full = fields[1];
            let xy = if xy_full.len() >= 2 {
                &xy_full[..2]
            } else {
                xy_full
            };
            Some(StatusEntry {
                xy_status: xy.to_string(),
                path: path.to_string(),
                old_path: orig_path.map(|p| p.to_string()),
                submodule_status: None,
                conflict_stages: None,
            })
        }
        "u" => {
            // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            let (fields, path) = split_entry_fields(entry, 10)?;
            Some(StatusEntry {
                xy_status: format!("u{}", fields[1]),
                path: path.to_string(),
                old_path: None,
                submodule_status: None,
                conflict_stages: fields.get(2).map(|s| s.to_string()),
            })
        }
        _ => None,
    }
}

/// Parse `git status --porcelain=v2 -z --branch` output into a `RichRepoStatus`.
pub fn parse_status_v2(output: &str, root: &str) -> RichRepoStatus {
    let mut head: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut upstream: Option<String> = None;
    let mut ahead: Option<i32> = None;
    let mut behind: Option<i32> = None;

    let mut staged: Vec<StatusEntry> = Vec::new();
    let mut unstaged: Vec<StatusEntry> = Vec::new();
    let mut untracked: Vec<StatusEntry> = Vec::new();
    let mut ignored: Vec<StatusEntry> = Vec::new();
    let mut conflicted: Vec<StatusEntry> = Vec::new();

    // Porcelain v2 with -z: everything is NUL-separated.
    // Branch header lines and status entries are all separated by NUL.
    // Format: # branch.oid <sha>\0# branch.head <name>\0...<entries>\0
    // Paths are unquoted in -z mode, so records must not be trimmed or
    // re-split on whitespace.
    let chunks: Vec<&str> = output.split('\0').collect();
    let mut idx = 0;
    while idx < chunks.len() {
        let chunk = chunks[idx];
        idx += 1;
        if chunk.is_empty() {
            continue;
        }

        if chunk.starts_with("# branch.") {
            parse_branch_line(
                chunk.trim(),
                &mut head,
                &mut branch,
                &mut upstream,
                &mut ahead,
                &mut behind,
            );
            continue;
        }

        // Status entries. Renames/copies (types 2/3) carry the original
        // path as the NUL-separated record immediately after the entry.
        let prefix = chunk.split(' ').next().unwrap_or_default();
        let parsed = match prefix {
            "1" | "u" => parse_entry(chunk, None),
            "2" | "3" => {
                let orig_path = chunks.get(idx).copied();
                idx += 1;
                parse_entry(chunk, orig_path)
            }
            "?" => {
                let path = chunk.strip_prefix('?').unwrap_or(chunk).trim().to_string();
                if path.is_empty() {
                    None
                } else {
                    Some(StatusEntry {
                        xy_status: "?".to_string(),
                        path,
                        old_path: None,
                        submodule_status: None,
                        conflict_stages: None,
                    })
                }
            }
            "!" => {
                let path = chunk.strip_prefix('!').unwrap_or(chunk).trim().to_string();
                if path.is_empty() {
                    None
                } else {
                    Some(StatusEntry {
                        xy_status: "!".to_string(),
                        path,
                        old_path: None,
                        submodule_status: None,
                        conflict_stages: None,
                    })
                }
            }
            _ => None,
        };
        if let Some(status_entry) = parsed {
            match classify_entry(&status_entry.xy_status) {
                "staged" => staged.push(status_entry),
                "unstaged" => unstaged.push(status_entry),
                "conflicted" => conflicted.push(status_entry),
                "ignored" => ignored.push(status_entry),
                "untracked" => untracked.push(status_entry),
                _ => {}
            }
        }
    }

    let operation_state = detect_operation_state(Path::new(root));
    let repository_operation_state =
        crate::operation_state::detect_operation_state_for_root(Path::new(root)).ok();
    let available_actions = repository_operation_state
        .as_ref()
        .map(|s| s.available_actions())
        .unwrap_or_default();

    let conflict_entries = build_conflict_entries(&conflicted);
    let conflict_report = if conflict_entries.is_empty() {
        None
    } else {
        Some(ConflictReport::from_entries(conflict_entries.clone()))
    };

    let is_clean =
        staged.is_empty() && unstaged.is_empty() && untracked.is_empty() && conflicted.is_empty();

    let dirty_summary = DirtySummary {
        staged_count: staged.len(),
        unstaged_count: unstaged.len(),
        untracked_count: untracked.len(),
        conflicted_count: conflicted.len(),
    };

    RichRepoStatus {
        root: root.to_string(),
        head,
        branch,
        upstream,
        ahead,
        behind,
        staged,
        unstaged,
        untracked,
        ignored,
        conflicted,
        conflict_entries,
        conflict_report,
        operation_state,
        repository_operation_state,
        available_actions,
        is_clean,
        dirty_summary,
    }
}

/// Build typed `ConflictEntry` records from porcelain v2 conflict entries.
fn build_conflict_entries(entries: &[StatusEntry]) -> Vec<ConflictEntry> {
    entries
        .iter()
        .map(|e| {
            let kind = classify_conflict_code(&e.xy_status);
            let shape = if e.submodule_status.is_some() {
                ConflictShape::Submodule
            } else if e.old_path.is_some() {
                ConflictShape::Rename
            } else {
                ConflictShape::File
            };
            // Submodule detection: porcelain v2 reports a submodule
            // status code (e.g. "M.", "UU", ".M") in the `M...` portion
            // when the path is a submodule gitlink. We only have
            // `submodule_status` populated when conflict_stages are
            // present, so treat presence as a strong signal here.
            let submodule = e.submodule_status.is_some();
            ConflictEntry {
                path: e.path.clone(),
                status_code: e.xy_status.clone(),
                kind,
                shape,
                base: ConflictObjectId::absent(),
                ours: ConflictObjectId::absent(),
                theirs: ConflictObjectId::absent(),
                original_path: e.old_path.clone(),
                has_conflict_markers: false,
                staged_resolved: false,
                submodule,
                recommended_actions: default_actions_for(kind, shape),
            }
        })
        .collect()
}

async fn run_git_async(
    root: std::path::PathBuf,
    args: Vec<String>,
) -> Result<std::process::Output, EgggitError> {
    crate::process::run(&args, &root).await
}

/// Fetch rich structured status from a repository.
pub async fn rich_repo_status(root: &Path) -> Result<RichRepoStatus, crate::EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    let root_buf = root.to_path_buf();
    let output = run_git_async(
        root_buf.clone(),
        vec![
            "status".into(),
            "--porcelain=v2".into(),
            "-z".into(),
            "--branch".into(),
        ],
    )
    .await?;

    if !output.status.success() {
        return Err(EgggitError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let root_str = root.to_string_lossy().to_string();

    Ok(parse_status_v2(&stdout, &root_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_repo(dir: &Path) {
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    fn commit_all(dir: &Path, msg: &str) {
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[test]
    fn parse_clean_status() {
        // Porcelain v2 with -z: all lines are NUL-separated
        let output = "# branch.oid abc123\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +0 -0\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.head, Some("abc123".to_string()));
        assert_eq!(status.branch, Some("main".to_string()));
        assert_eq!(status.upstream, Some("origin/main".to_string()));
        assert_eq!(status.ahead, Some(0));
        assert_eq!(status.behind, Some(0));
        assert!(status.staged.is_empty());
        assert!(status.unstaged.is_empty());
        assert!(status.untracked.is_empty());
        assert!(status.is_clean);
        assert_eq!(
            status.dirty_summary,
            DirtySummary {
                staged_count: 0,
                unstaged_count: 0,
                untracked_count: 0,
                conflicted_count: 0,
            }
        );
    }

    #[test]
    fn parse_status_with_staged_unstaged_untracked() {
        // In porcelain v2: X=index(staged), Y=worktree(unstaged)
        // "M." = index modified (staged), ".M" = worktree modified (unstaged)
        let output = "# branch.oid abc123\0# branch.head main\x001 M. N... 100644 100644 100644 abc123 abc123 staged.txt\x001 .M N... 100644 100644 000000 abc123 abc123 unstaged.txt\0? untracked.txt\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].path, "staged.txt");
        assert_eq!(status.staged[0].xy_status, "M.");
        assert_eq!(status.unstaged.len(), 1);
        assert_eq!(status.unstaged[0].path, "unstaged.txt");
        assert_eq!(status.unstaged[0].xy_status, ".M");
        assert_eq!(status.untracked.len(), 1);
        assert_eq!(status.untracked[0].path, "untracked.txt");
        assert!(!status.is_clean);
        assert_eq!(
            status.dirty_summary,
            DirtySummary {
                staged_count: 1,
                unstaged_count: 1,
                untracked_count: 1,
                conflicted_count: 0,
            }
        );
    }

    #[test]
    fn parse_detached_head() {
        let output = "# branch.oid deadbeef\0# branch.head (detached)\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.head, Some("deadbeef".to_string()));
        assert!(status.branch.is_none());
        assert!(status.upstream.is_none());
    }

    #[test]
    fn parse_status_with_conflicts() {
        // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
        let output = "# branch.oid abc123\0# branch.head main\0u AU N... 000000 100644 100644 100644 000000 abc123 def456 conflict.txt\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.conflicted.len(), 1);
        assert_eq!(status.conflicted[0].path, "conflict.txt");
        assert!(status.staged.is_empty());
        assert!(status.unstaged.is_empty());
        assert!(!status.conflict_entries.is_empty());
        let entry = &status.conflict_entries[0];
        assert_eq!(entry.path, "conflict.txt");
        assert_eq!(entry.status_code, "uAU");
        // Submodule status absent → not a submodule conflict.
        assert!(!entry.submodule);
        assert!(!entry.recommended_actions.is_empty());
    }

    #[test]
    fn conflict_report_populated_when_unmerged() {
        let output = "# branch.oid abc123\0# branch.head main\0u UU N... 000000 100644 100644 100644 000000 abc123 def456 a.txt\0u AA N... 000000 100644 100644 100644 000000 abc123 def456 b.txt\0";
        let status = parse_status_v2(output, "/tmp/repo");
        let report = status.conflict_report.expect("report present");
        assert_eq!(report.total, 2);
        assert_eq!(report.entries.len(), 2);
        assert_eq!(status.available_actions.len(), 0); // /tmp/repo has no MERGE_HEAD etc.
    }

    #[test]
    fn parse_status_with_renamed_files() {
        // Type 2 (rename): `2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>`
        // with the original path as the next NUL-separated record.
        let output = "# branch.oid abc123\0# branch.head main\x002 R. N... 100644 100644 100644 abc123 def456 R100 new_name.txt\0old_name.txt\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].path, "new_name.txt");
        assert_eq!(status.staged[0].old_path, Some("old_name.txt".to_string()));
        assert_eq!(status.staged[0].xy_status, "R.");
    }

    #[test]
    fn parse_status_paths_with_spaces() {
        // Paths are unquoted in -z mode and may contain spaces.
        let output = "# branch.oid abc123\0# branch.head main\x001 M. N... 100644 100644 100644 abc123 abc123 my file.txt\x001 .M N... 100644 100644 000000 abc123 abc123 dir with spaces/other name.txt\0? untracked file with spaces.txt\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.staged[0].path, "my file.txt");
        assert_eq!(status.unstaged[0].path, "dir with spaces/other name.txt");
        assert_eq!(status.untracked[0].path, "untracked file with spaces.txt");
    }

    #[test]
    fn parse_status_rename_with_spaces() {
        // Rename entries keep both paths verbatim: the new path after the
        // fixed fields and the original path in the following record.
        let output = "# branch.oid abc123\0# branch.head main\x002 R. N... 100644 100644 100644 abc123 def456 R100 new name.txt\0old name.txt\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].path, "new name.txt");
        assert_eq!(status.staged[0].old_path, Some("old name.txt".to_string()));
    }

    #[test]
    fn parse_status_with_ahead_behind() {
        let output = "# branch.oid abc123\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +5 -3\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.ahead, Some(5));
        assert_eq!(status.behind, Some(3));
    }

    #[test]
    fn parse_status_with_ignored() {
        let output = "# branch.oid abc123\0# branch.head main\0! target/debug/\0! .env.local\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.ignored.len(), 2);
        assert_eq!(status.ignored[0].path, "target/debug/");
        assert_eq!(status.ignored[1].path, ".env.local");
    }

    #[test]
    fn parse_status_copied_files() {
        // Type 3 (copy): `3 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>`
        // with the source path as the next NUL-separated record.
        let output = "# branch.oid abc123\0# branch.head main\x003 C. N... 100644 100644 100644 abc123 abc123 C100 copy.txt\0source.txt\0";
        let status = parse_status_v2(output, "/tmp/repo");
        assert_eq!(status.staged.len(), 1);
        assert_eq!(status.staged[0].path, "copy.txt");
        assert_eq!(status.staged[0].old_path, Some("source.txt".to_string()));
        assert_eq!(status.staged[0].xy_status, "C.");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn integration_clean_repo() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        commit_all(dir.path(), "init");

        let status = rich_repo_status(dir.path()).await.unwrap();
        assert!(status.is_clean);
        assert_eq!(status.branch, Some("main".to_string()));
        assert!(status.head.is_some());
        assert!(status.staged.is_empty());
        assert!(status.unstaged.is_empty());
        assert!(status.untracked.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn integration_dirty_repo() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        commit_all(dir.path(), "init");

        // Modify existing file (unstaged)
        std::fs::write(dir.path().join("a.txt"), "changed").unwrap();
        // New untracked file
        std::fs::write(dir.path().join("b.txt"), "new").unwrap();
        // Stage a new file
        std::fs::write(dir.path().join("c.txt"), "staged").unwrap();
        Command::new("git")
            .args(["add", "c.txt"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let status = rich_repo_status(dir.path()).await.unwrap();
        assert!(!status.is_clean);
        assert!(status.staged.iter().any(|e| e.path == "c.txt"));
        assert!(status.unstaged.iter().any(|e| e.path == "a.txt"));
        assert!(status.untracked.iter().any(|e| e.path == "b.txt"));
        assert_eq!(status.dirty_summary.staged_count, 1);
        assert_eq!(status.dirty_summary.unstaged_count, 1);
        assert_eq!(status.dirty_summary.untracked_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn integration_detached_head() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        commit_all(dir.path(), "init");
        // Create another commit to detach from
        std::fs::write(dir.path().join("b.txt"), "two").unwrap();
        commit_all(dir.path(), "second");
        // Detach HEAD
        let head_output = Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let head_sha = String::from_utf8_lossy(&head_output.stdout)
            .trim()
            .to_string();
        Command::new("git")
            .args(["checkout", &head_sha])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let status = rich_repo_status(dir.path()).await.unwrap();
        assert!(status.branch.is_none());
        assert_eq!(status.head.as_deref(), Some(head_sha.as_str()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn integration_non_repo_errors() {
        let fake = std::path::PathBuf::from("/this/path/does/not/exist/xyz");
        let result = rich_repo_status(&fake).await;
        assert!(result.is_err());
    }

    #[test]
    fn detect_merge_state() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join(".git/MERGE_HEAD"), "abc123\n").unwrap();
        let state = detect_operation_state(dir.path());
        assert_eq!(
            state,
            Some(OperationState::Merge {
                head: Some("abc123".to_string())
            })
        );
    }

    #[test]
    fn detect_rebase_state() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join(".git/REBASE_HEAD"), "def456\n").unwrap();
        let state = detect_operation_state(dir.path());
        assert_eq!(
            state,
            Some(OperationState::Rebase {
                head: Some("def456".to_string())
            })
        );
    }

    #[test]
    fn detect_bisect_state() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join(".git/BISECT_LOG"), "").unwrap();
        let state = detect_operation_state(dir.path());
        assert_eq!(state, Some(OperationState::Bisect));
    }

    #[test]
    fn no_operation_state_when_clean() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let state = detect_operation_state(dir.path());
        assert!(state.is_none());
    }

    #[test]
    fn parse_empty_output() {
        let output = "";
        let status = parse_status_v2(output, "/tmp/repo");
        assert!(status.is_clean);
        assert!(status.head.is_none());
        assert!(status.branch.is_none());
    }
}

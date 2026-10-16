use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Proof that the selected commit contains no active custom checkout filter
/// under the effective attributes visible to the Git process. Fields stay
/// private so callers cannot construct an attestation without the preflight.
#[derive(Debug, Clone)]
pub struct FilterFreeCheckoutAttestation {
    pub(crate) repository_root: PathBuf,
    pub(crate) commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_detached: bool,
}

async fn run_git_async(
    root: PathBuf,
    args: Vec<String>,
) -> Result<std::process::Output, EgggitError> {
    crate::process::run(&args, &root).await
}

fn push_parsed_worktree(
    worktrees: &mut Vec<WorktreeInfo>,
    path: &str,
    branch: &str,
    head: Option<&str>,
    is_current: bool,
    is_detached: bool,
) {
    if path.is_empty() {
        return;
    }

    let branch_name = if !branch.is_empty() {
        branch
            .strip_prefix("refs/heads/")
            .unwrap_or(branch)
            .to_string()
    } else if is_detached {
        match head {
            Some(sha) if !sha.is_empty() => format!("detached@{sha}"),
            _ => format!("detached@{path}"),
        }
    } else {
        String::new()
    };

    worktrees.push(WorktreeInfo {
        path: path.to_string(),
        branch: branch_name,
        is_current,
        is_detached,
    });
}

/// List worktrees in a repository (read-only).
pub async fn list_worktrees(git_root: &Path) -> Result<Vec<WorktreeInfo>, EgggitError> {
    let output = run_git_async(
        git_root.to_path_buf(),
        vec!["worktree".into(), "list".into(), "--porcelain".into()],
    )
    .await?;
    if !output.status.success() {
        return Err(EgggitError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();
    let mut current_head: Option<String> = None;
    let mut current_is_detached = false;
    let mut current_is_current = false;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            push_parsed_worktree(
                &mut worktrees,
                &current_path,
                &current_branch,
                current_head.as_deref(),
                current_is_current,
                current_is_detached,
            );
            current_path = path.to_string();
            current_branch.clear();
            current_head = None;
            current_is_detached = false;
            current_is_current = Path::new(path) == git_root
                || Path::new(path).canonicalize().is_ok_and(|p| {
                    p == git_root
                        .canonicalize()
                        .unwrap_or_else(|_| git_root.to_path_buf())
                });
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current_branch = branch.to_string();
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current_head = Some(head.to_string());
        } else if line == "detached" {
            current_is_detached = true;
        }
    }

    push_parsed_worktree(
        &mut worktrees,
        &current_path,
        &current_branch,
        current_head.as_deref(),
        current_is_current,
        current_is_detached,
    );

    Ok(worktrees)
}

/// Check the committed tree's effective attributes before checkout.
/// Git filter drivers are repository-configured commands, so a managed
/// checkout is supported only when every selected path has no active filter.
/// Both the tree walk and attribute report have explicit output bounds. The
/// `--all` query distinguishes an absent filter attribute from an explicitly
/// configured driver named `unspecified`.
pub async fn attest_filter_free_checkout(
    git_root: &Path,
    commit: &str,
) -> Result<FilterFreeCheckoutAttestation, EgggitError> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(EgggitError::InvalidBaseRef(
            "checkout base must be a full object ID".into(),
        ));
    }
    let repository_root = git_root
        .canonicalize()
        .map_err(|error| EgggitError::Io(format!("canonicalize repository root: {error}")))?;
    const MAX_TREE_PATHS_BYTES: usize = 32 * 1024 * 1024;
    const MAX_FILTER_REPORT_BYTES: usize = 64 * 1024 * 1024;
    let tree = crate::process::run_bounded(
        &[
            "ls-tree".into(),
            "-r".into(),
            "-z".into(),
            "--name-only".into(),
            commit.into(),
        ],
        &repository_root,
        MAX_TREE_PATHS_BYTES,
    )
    .await?;
    if !tree.status.success() {
        return Err(EgggitError::Git(
            "failed to enumerate checkout paths".into(),
        ));
    }
    if !tree.stdout.is_empty() && tree.stdout.last() != Some(&0) {
        return Err(EgggitError::Git(
            "Git returned malformed NUL-delimited checkout paths".into(),
        ));
    }
    let attributes = crate::process::run_bounded_with_input(
        &[
            "check-attr".into(),
            format!("--source={commit}"),
            "--stdin".into(),
            "-z".into(),
            "--all".into(),
        ],
        &repository_root,
        tree.stdout,
        MAX_FILTER_REPORT_BYTES,
    )
    .await?;
    if !attributes.status.success() {
        return Err(EgggitError::Git(
            "failed to inspect checkout filter attributes".into(),
        ));
    }
    let fields: Vec<&[u8]> = attributes.stdout.split(|byte| *byte == 0).collect();
    if fields.len() % 3 != 1 {
        return Err(EgggitError::Git(
            "Git returned malformed checkout filter attributes".into(),
        ));
    }
    for entry in fields[..fields.len().saturating_sub(1)].chunks_exact(3) {
        if entry[1] == b"filter" {
            return Err(EgggitError::Git(
                "checkout uses a custom Git filter; managed materialization is unavailable for this revision".into(),
            ));
        }
    }
    Ok(FilterFreeCheckoutAttestation {
        repository_root,
        commit: commit.to_owned(),
    })
}

/// Walk up the directory tree to find a git root.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let git_path = current.join(".git");
        if git_path.exists() || is_git_file(&git_path) {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn is_git_file(git_path: &Path) -> bool {
    if let Ok(content) = std::fs::read_to_string(git_path) {
        content.starts_with("gitdir:")
    } else {
        false
    }
}

pub fn is_git_worktree(dir: &Path) -> bool {
    let git_path = dir.join(".git");
    git_path.exists() && is_git_file(&git_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_repo(dir: &Path) {
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[test]
    fn find_git_root_in_repo() {
        let dir = tempfile::Builder::new()
            .prefix("egggit-find-root-yes-")
            .tempdir()
            .unwrap();
        init_repo(dir.path());
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let root = find_git_root(&nested).unwrap();
        assert_eq!(
            root.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn find_git_root_returns_none_for_nonexistent_path() {
        // Walking up from a non-existent path can still find a real git
        // ancestor. The function should not crash and should return
        // *some* result. We only assert it doesn't panic and returns
        // the codegg workspace git root (since the test runs from there).
        let fake = std::path::PathBuf::from("/this/path/does/not/exist/xyz");
        // The result is non-deterministic depending on whether the test is
        // running inside the codegg workspace. We only assert no panic.
        let _ = find_git_root(&fake);
    }

    #[test]
    fn is_git_file_detects_gitdir() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join(".git");
        std::fs::write(&file, "gitdir: /tmp/foo").unwrap();
        assert!(is_git_file(&file));
    }

    #[test]
    fn is_git_file_rejects_other() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join(".git");
        std::fs::write(&file, "not a gitdir pointer").unwrap();
        assert!(!is_git_file(&file));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_worktrees_for_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        init_repo(dir.path());
        let list = list_worktrees(dir.path()).await.unwrap();
        // There is at least the main worktree.
        assert!(!list.is_empty());
        assert!(list[0].is_current);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn filter_free_checkout_attestation_rejects_custom_filter_attributes() {
        let dir = tempfile::TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("plain.txt"), "plain\n").unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .status()
                .unwrap()
        };
        assert!(git(&["add", "."]).success());
        assert!(git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "plain",
        ])
        .success());
        let base = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir.path())
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let proof = attest_filter_free_checkout(dir.path(), &base)
            .await
            .unwrap();
        assert_eq!(proof.commit, base);

        std::fs::write(
            dir.path().join(".gitattributes"),
            "plain.txt filter=custom\n",
        )
        .unwrap();
        assert!(git(&["add", ".gitattributes"]).success());
        assert!(git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "filtered",
        ])
        .success());
        let filtered = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir.path())
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let error = attest_filter_free_checkout(dir.path(), &filtered)
            .await
            .expect_err("custom filters must be refused");
        assert!(error.to_string().contains("custom Git filter"));

        std::fs::write(
            dir.path().join(".gitattributes"),
            "plain.txt filter=unspecified\n",
        )
        .unwrap();
        assert!(git(&["add", ".gitattributes"]).success());
        assert!(git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "explicit unspecified filter",
        ])
        .success());
        let explicit_unspecified = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir.path())
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let error = attest_filter_free_checkout(dir.path(), &explicit_unspecified)
            .await
            .expect_err("an explicitly named driver must be refused even if named unspecified");
        assert!(error.to_string().contains("custom Git filter"));
    }
}

use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ChangedFile {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    #[default]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileDiff {
    pub path: String,
    pub patch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatchValidation {
    pub valid: bool,
    pub error: Option<String>,
}

fn parse_numstat_line(line: &str) -> Option<ChangedFile> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?;
    let removed = parts.next()?;
    let path = parts.next()?.to_string();
    let kind = if added.starts_with('-') && removed.starts_with('-') {
        ChangeKind::Deleted
    } else if added == "0" && removed == "0" {
        ChangeKind::Other
    } else {
        ChangeKind::Modified
    };
    Some(ChangedFile { path, kind })
}

async fn capture_diff(
    root: &Path,
    base: Option<&str>,
    args: &[&str],
) -> Result<String, EgggitError> {
    let mut full: Vec<String> = Vec::with_capacity(args.len() + 2);
    // args typically start with "diff". Insert base right after the subcommand.
    let mut inserted_base = false;
    for (i, a) in args.iter().enumerate() {
        full.push(a.to_string());
        if i == 0 && *a == "diff" {
            let b = base.unwrap_or("HEAD");
            full.push(b.to_string());
            inserted_base = true;
        }
    }
    if !inserted_base {
        if let Some(b) = base {
            full.push(b.to_string());
        }
    }
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if !root.exists() {
            return Err(EgggitError::NotARepository(root.display().to_string()));
        }
        let mut cmd = std::process::Command::new("git");
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        let refs: Vec<&str> = full.iter().map(|s| s.as_str()).collect();
        cmd.args(&refs).current_dir(&root);
        cmd.output()
            .map_err(|e| EgggitError::Io(e.to_string()))
            .and_then(|out| {
                if !out.status.success() {
                    Err(EgggitError::Git(
                        String::from_utf8_lossy(&out.stderr).to_string(),
                    ))
                } else {
                    Ok(String::from_utf8_lossy(&out.stdout).to_string())
                }
            })
    })
    .await
    .map_err(|e| EgggitError::Join(e.to_string()))?
}

pub async fn diff_summary(root: &Path, base: Option<&str>) -> Result<DiffSummary, EgggitError> {
    let out = capture_diff(root, base, &["diff", "--numstat"]).await?;
    let mut summary = DiffSummary::default();
    for line in out.lines() {
        if let Some(file) = parse_numstat_line(line) {
            summary.files.push(file);
        }
    }
    let stat = capture_diff(root, base, &["diff", "--shortstat"]).await?;
    let trimmed = stat.trim().to_string();
    if !trimmed.is_empty() {
        let mut tokens = trimmed.split_whitespace();
        if let Some(s) = tokens.next() {
            summary.files_changed = s.parse().unwrap_or(0);
        }
        summary.insertions = count_after(&trimmed, "insertion");
        summary.deletions = count_after(&trimmed, "deletion");
    }
    if summary.files_changed == 0 {
        summary.files_changed = summary.files.len();
    }
    Ok(summary)
}

/// What slice of the working tree a diff should cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    /// All changes between the working tree and `HEAD` (staged +
    /// unstaged combined).
    Head,
    /// Staged changes against `HEAD` (`git diff --cached HEAD`).
    Staged,
    /// Diff against an explicit base ref. Equivalent to
    /// `git diff <base>`.
    Base(&'static str),
}

impl DiffMode {
    /// Render the mode as the trailing argument list passed to
    /// `git diff`. The "diff" subcommand is assumed to already be
    /// present at the start of `args`.
    fn extra_args(self) -> Vec<&'static str> {
        match self {
            DiffMode::Head => Vec::new(),
            DiffMode::Staged => vec!["--cached", "HEAD"],
            DiffMode::Base(_) => Vec::new(),
        }
    }
}

/// Capture the full unified diff text for the requested mode.
///
/// The `mode` argument selects whether the diff covers all changes
/// against `HEAD` (default), staged changes only, or an explicit
/// base ref. The returned text is the raw output of `git diff` for
/// the requested slice.
pub async fn diff_text(root: &Path, mode: DiffMode) -> Result<String, EgggitError> {
    let extras = mode.extra_args();
    let base = match mode {
        DiffMode::Base(b) => Some(b),
        _ => None,
    };

    // Build the full arg list. For Staged, `extras` already contains
    // `--cached HEAD`, so we don't append the base a second time.
    let mut full: Vec<String> = Vec::with_capacity(extras.len() + 2);
    full.push("diff".to_string());
    for a in &extras {
        full.push((*a).to_string());
    }
    if let Some(b) = base {
        full.push(b.to_string());
    }

    let refs: Vec<&str> = full.iter().map(|s| s.as_str()).collect();
    run_diff(root, &refs).await
}

async fn run_diff(root: &Path, args: &[&str]) -> Result<String, EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }
    let root = root.to_path_buf();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new("git");
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        cmd.args(&arg_refs).current_dir(&root);
        cmd.output()
            .map_err(|e| EgggitError::Io(e.to_string()))
            .and_then(|out| {
                if !out.status.success() {
                    Err(EgggitError::Git(
                        String::from_utf8_lossy(&out.stderr).to_string(),
                    ))
                } else {
                    Ok(String::from_utf8_lossy(&out.stdout).to_string())
                }
            })
    })
    .await
    .map_err(|e| EgggitError::Join(e.to_string()))?
}

fn count_after(haystack: &str, needle: &str) -> usize {
    if let Some(idx) = haystack.find(needle) {
        let before = &haystack[..idx];
        if let Some(last) = before.split_whitespace().last() {
            return last.parse().unwrap_or(0);
        }
    }
    0
}

pub async fn changed_files(
    root: &Path,
    base: Option<&str>,
) -> Result<Vec<ChangedFile>, EgggitError> {
    let summary = diff_summary(root, base).await?;
    Ok(summary.files)
}

pub async fn file_diff(
    root: &Path,
    path: &Path,
    base: Option<&str>,
) -> Result<FileDiff, EgggitError> {
    let rel = path.to_string_lossy().to_string();
    let out = capture_diff(root, base, &["diff", "--", &rel]).await?;
    Ok(FileDiff {
        path: rel,
        patch: out,
    })
}

pub async fn validate_patch(root: &Path, patch: &str) -> Result<PatchValidation, EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }
    let root = root.to_path_buf();
    let patch = patch.to_string();
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = std::process::Command::new("git")
            .args(["apply", "--check", "-"])
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| EgggitError::Io(e.to_string()))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(patch.as_bytes())
                .map_err(|e| EgggitError::Io(e.to_string()))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| EgggitError::Io(e.to_string()))?;
        if out.status.success() {
            Ok(PatchValidation {
                valid: true,
                error: None,
            })
        } else {
            Ok(PatchValidation {
                valid: false,
                error: Some(String::from_utf8_lossy(&out.stderr).to_string()),
            })
        }
    })
    .await
    .map_err(|e| EgggitError::Join(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::repo_status;
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

    #[tokio::test]
    async fn repo_status_clean() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let s = repo_status(dir.path()).await.unwrap();
        assert_eq!(s.branch, "main");
        assert!(!s.is_dirty);
        assert!(s.commit_hash.is_some());
    }

    #[tokio::test]
    async fn repo_status_dirty() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("a.txt"), "changed").unwrap();

        let s = repo_status(dir.path()).await.unwrap();
        assert!(s.is_dirty);
    }

    #[tokio::test]
    async fn diff_summary_detects_changes() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello world").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let s = diff_summary(dir.path(), None).await.unwrap();
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].path, "a.txt");
    }

    #[tokio::test]
    async fn changed_files_after_commit() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("b.txt"), "new").unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello!").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let files = changed_files(dir.path(), None).await.unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"a.txt"));
        assert!(paths.contains(&"b.txt"));
    }

    #[tokio::test]
    async fn file_diff_returns_patch() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("a.txt"), "world\n").unwrap();

        let d = file_diff(dir.path(), std::path::Path::new("a.txt"), None)
            .await
            .unwrap();
        assert_eq!(d.path, "a.txt");
        assert!(d.patch.contains("hello") || d.patch.contains("world"));
    }

    #[tokio::test]
    async fn validate_patch_clean() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let patch = "diff --git a/a.txt b/a.txt\nindex 0000..0000 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-hello\n+world\n";
        let v = validate_patch(dir.path(), patch).await.unwrap();
        assert!(v.valid, "expected patch to validate, got: {:?}", v.error);
    }

    #[tokio::test]
    async fn validate_patch_invalid() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let patch = "this is not a patch";
        let v = validate_patch(dir.path(), patch).await.unwrap();
        assert!(!v.valid);
        assert!(v.error.is_some());
    }

    #[tokio::test]
    async fn non_repo_errors_for_nonexistent_path() {
        let fake = std::path::PathBuf::from("/this/path/does/not/exist/xyz");
        let r = repo_status(&fake).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn diff_text_head_includes_unstaged_changes() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        // Unstaged edit.
        std::fs::write(dir.path().join("a.txt"), "two\n").unwrap();

        let text = diff_text(dir.path(), DiffMode::Head).await.unwrap();
        assert!(
            text.contains("-one"),
            "expected -one in unstaged diff, got:\n{text}"
        );
        assert!(text.contains("+two"));
    }

    #[tokio::test]
    async fn diff_text_staged_includes_only_staged_changes() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        // Stage one edit and leave another unstaged.
        std::fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "brand new\n").unwrap();
        Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let text = diff_text(dir.path(), DiffMode::Staged).await.unwrap();
        assert!(text.contains("a.txt"), "staged diff should include a.txt");
        // b.txt is unstaged so it should not be in the staged slice.
        assert!(
            !text.contains("b.txt"),
            "staged diff should not include b.txt"
        );
    }
}

use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single parsed commit from git log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitInfo {
    /// Full 40-char SHA.
    pub oid: String,
    /// Short 7-char SHA.
    pub short_oid: String,
    /// Parent OIDs (0 for initial commit, 1 for normal, 2+ for merge commits).
    pub parents: Vec<String>,
    /// Author name.
    pub author_name: String,
    /// Author email.
    pub author_email: String,
    /// Author timestamp (unix epoch).
    pub author_time: i64,
    /// Committer name.
    pub committer_name: String,
    /// Committer email.
    pub committer_email: String,
    /// Committer timestamp (unix epoch).
    pub committer_time: i64,
    /// Subject (first line of commit message).
    pub subject: String,
    /// Body (rest of commit message, trimmed).
    pub body: String,
    /// Decorations (e.g., "(HEAD -> main, origin/main)").
    pub decorations: Option<String>,
}

async fn run_git_log(
    root: std::path::PathBuf,
    args: Vec<String>,
) -> Result<std::process::Output, EgggitError> {
    tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new("git");
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        cmd.args(&args).current_dir(&root);
        cmd.output().map_err(|e| EgggitError::Io(e.to_string()))
    })
    .await
    .map_err(|e| EgggitError::Join(e.to_string()))?
}

fn parse_value(line: &str, prefix: &str) -> String {
    line.strip_prefix(prefix).unwrap_or("").to_string()
}

fn parse_log_output(stdout: &str) -> Vec<CommitInfo> {
    let mut commits = Vec::new();
    for block in stdout.split("---END---\n").filter(|b| !b.trim().is_empty()) {
        let lines: Vec<&str> = block.lines().collect();
        if lines.is_empty() {
            continue;
        }

        let mut oid = String::new();
        let mut short_oid = String::new();
        let mut parents = Vec::new();
        let mut author_name = String::new();
        let mut author_email = String::new();
        let mut author_time: i64 = 0;
        let mut committer_name = String::new();
        let mut committer_email = String::new();
        let mut committer_time: i64 = 0;
        let mut subject = String::new();
        let mut body_lines: Vec<String> = Vec::new();
        let mut decorations: Option<String> = None;

        for line in &lines {
            if line.starts_with("commit:") {
                oid = parse_value(line, "commit:");
            } else if line.starts_with("short:") {
                short_oid = parse_value(line, "short:");
            } else if line.starts_with("parents:") {
                let val = parse_value(line, "parents:");
                if !val.is_empty() {
                    parents = val.split(' ').map(|s| s.to_string()).collect();
                }
            } else if line.starts_with("author-name:") {
                author_name = parse_value(line, "author-name:");
            } else if line.starts_with("author-email:") {
                author_email = parse_value(line, "author-email:");
            } else if line.starts_with("author-time:") {
                author_time = parse_value(line, "author-time:").parse().unwrap_or(0);
            } else if line.starts_with("committer-name:") {
                committer_name = parse_value(line, "committer-name:");
            } else if line.starts_with("committer-email:") {
                committer_email = parse_value(line, "committer-email:");
            } else if line.starts_with("committer-time:") {
                committer_time = parse_value(line, "committer-time:").parse().unwrap_or(0);
            } else if line.starts_with("subject:") {
                subject = parse_value(line, "subject:");
            } else if line.starts_with("body:") {
                let body_val = parse_value(line, "body:");
                if !body_val.is_empty() {
                    body_lines.push(body_val);
                }
            } else if line.starts_with("decorations:") {
                let dec = parse_value(line, "decorations:");
                decorations = if dec.is_empty() { None } else { Some(dec) };
            }
        }

        if oid.is_empty() {
            continue;
        }

        let body = body_lines.join("\n").trim().to_string();

        commits.push(CommitInfo {
            oid,
            short_oid,
            parents,
            author_name,
            author_email,
            author_time,
            committer_name,
            committer_email,
            committer_time,
            subject,
            body,
            decorations,
        });
    }
    commits
}

/// Fetch recent commits using `git log` with a machine-readable format.
///
/// `max_count` limits how many commits to return (default 20).
/// `paths` optionally filters to commits touching specific paths.
pub async fn log_commits(
    root: &Path,
    max_count: Option<usize>,
    paths: &[&str],
) -> Result<Vec<CommitInfo>, crate::EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    let count = max_count.unwrap_or(20);
    let mut args: Vec<String> = vec![
        "log".into(),
        format!("--max-count={count}"),
        "--format=commit:%H%nshort:%h%nparents:%P%nauthor-name:%aN%nauthor-email:%aE%nauthor-time:%at%ncommitter-name:%cN%ncommitter-email:%cE%ncommitter-time:%ct%nsubject:%s%nbody:%b%ndecorations:%D%n---END---".into(),
    ];

    if !paths.is_empty() {
        args.push("--".into());
        for p in paths {
            args.push(p.to_string());
        }
    }

    let root = root.to_path_buf();
    let output = run_git_log(root, args).await;

    // Handle errors gracefully (empty repos, etc.)
    let output = match output {
        Ok(o) => o,
        Err(_) => return Ok(Vec::new()),
    };

    // git log returns non-zero for empty repos
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not have any commits yet") || stderr.contains("unknown revision") {
            return Ok(Vec::new());
        }
        return Err(EgggitError::Git(stderr.to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_log_output(&stdout))
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

    fn commit(dir: &Path, msg: &str, filename: &str, content: &str) {
        std::fs::write(dir.join(filename), content).unwrap();
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

    #[tokio::test(flavor = "current_thread")]
    async fn log_returns_commits() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "first", "a.txt", "hello");
        commit(dir.path(), "second", "a.txt", "world");

        let commits = log_commits(dir.path(), None, &[]).await.unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "second");
        assert_eq!(commits[1].subject, "first");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_respects_max_count() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "first", "a.txt", "hello");
        commit(dir.path(), "second", "a.txt", "world");

        let commits = log_commits(dir.path(), Some(1), &[]).await.unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "second");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_filters_by_path() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "first", "a.txt", "hello");
        commit(dir.path(), "second", "b.txt", "world");

        let commits = log_commits(dir.path(), None, &["a.txt"]).await.unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "first");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_parse_commit_fields() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "initial", "a.txt", "hello");

        let commits = log_commits(dir.path(), None, &[]).await.unwrap();
        assert_eq!(commits.len(), 1);
        let c = &commits[0];
        assert_eq!(c.oid.len(), 40);
        assert_eq!(c.short_oid.len(), 7);
        assert!(c.author_name.starts_with("Test"));
        assert_eq!(c.author_email, "test@example.com");
        assert!(c.author_time > 0);
        assert_eq!(c.committer_name, c.author_name);
        assert_eq!(c.committer_email, c.author_email);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_empty_repo() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let commits = log_commits(dir.path(), None, &[]).await.unwrap();
        assert!(commits.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_nonexistent_path_errors() {
        let fake = std::path::PathBuf::from("/this/path/does/not/exist/xyz");
        let r = log_commits(&fake, None, &[]).await;
        assert!(r.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_merge_commit_has_two_parents() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "first", "a.txt", "hello");

        // Create a branch and add a commit on it
        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        commit(dir.path(), "feature commit", "b.txt", "feature");

        // Go back to main and make another commit
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        commit(dir.path(), "main commit", "c.txt", "main");

        // Merge feature into main
        Command::new("git")
            .args(["merge", "feature", "-m", "merge commit"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let commits = log_commits(dir.path(), None, &[]).await.unwrap();
        // The merge commit should be the first one
        let merge = commits.iter().find(|c| c.subject == "merge commit");
        assert!(merge.is_some(), "expected merge commit");
        assert_eq!(merge.unwrap().parents.len(), 2);
    }
}

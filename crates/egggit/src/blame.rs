use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single blame entry for a line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlameEntry {
    /// Commit OID that last modified this line.
    pub commit: String,
    /// Short commit SHA.
    pub short_commit: String,
    /// Line number in the file.
    pub lineno: u32,
    /// Original line number in the source commit.
    pub orig_lineno: Option<u32>,
    /// Author of the commit.
    pub author: String,
    /// Author timestamp.
    pub author_time: i64,
    /// The actual line content.
    pub content: String,
}

/// Full blame result for a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlameResult {
    /// Path of the blamed file.
    pub path: String,
    /// All blame entries, one per line.
    pub entries: Vec<BlameEntry>,
}

async fn run_git_blame(
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

fn parse_porcelain(stdout: &str) -> Vec<BlameEntry> {
    let mut entries = Vec::new();
    let mut current_commit = String::new();
    let mut current_short = String::new();
    let mut current_author = String::new();
    let mut current_author_time: i64 = 0;
    let mut current_orig_lineno: Option<u32> = None;

    for line in stdout.lines() {
        // Header line: <40-sha> <orig_lineno> <lineno> [<num_lines>]
        // e.g. "0000000000000000000000000000000000000000 1 1 2"
        if line.len() >= 41 && line.as_bytes()[40] == b' ' {
            let parts: Vec<&str> = line.splitn(4, ' ').collect();
            if parts.len() >= 3 {
                current_commit = parts[0].to_string();
                current_short = current_commit.chars().take(7).collect();
                current_orig_lineno = parts[1].parse().ok();
                // Reset per-line metadata for new hunk
                current_author.clear();
                current_author_time = 0;
            }
            continue;
        }

        // Metadata lines inside a hunk
        if let Some(val) = line.strip_prefix("author ") {
            current_author = val.to_string();
        } else if let Some(val) = line.strip_prefix("author-time ") {
            current_author_time = val.parse().unwrap_or(0);
        } else if line == "filename"
            || line == "summary"
            || line.starts_with("filename ")
            || line.starts_with("summary ")
            || line.starts_with("previous ")
        {
            // skip metadata lines we don't need
        }

        // Content line starts with a tab
        if let Some(content) = line.strip_prefix('\t') {
            let lineno = entries.len() as u32 + 1;
            entries.push(BlameEntry {
                commit: current_commit.clone(),
                short_commit: current_short.clone(),
                lineno,
                orig_lineno: current_orig_lineno.take(),
                author: current_author.clone(),
                author_time: current_author_time,
                content: content.to_string(),
            });
        }
    }

    entries
}

/// Get blame information for a file.
pub async fn blame_file(root: &Path, path: &str) -> Result<BlameResult, crate::EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    let args = vec!["blame".into(), "--porcelain".into(), path.to_string()];

    let root = root.to_path_buf();
    let output = run_git_blame(root, args).await?;

    if !output.status.success() {
        return Err(EgggitError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries = parse_porcelain(&stdout);

    Ok(BlameResult {
        path: path.to_string(),
        entries,
    })
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
    async fn blame_returns_entries() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "initial", "a.txt", "line1\nline2\nline3\n");

        let result = blame_file(dir.path(), "a.txt").await.unwrap();
        assert_eq!(result.path, "a.txt");
        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.entries[0].content, "line1");
        assert_eq!(result.entries[1].content, "line2");
        assert_eq!(result.entries[2].content, "line3");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blame_entries_have_commit_info() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "initial", "a.txt", "hello\n");

        let result = blame_file(dir.path(), "a.txt").await.unwrap();
        assert_eq!(result.entries.len(), 1);
        let e = &result.entries[0];
        assert_eq!(e.commit.len(), 40);
        assert_eq!(e.short_commit.len(), 7);
        assert_eq!(e.author, "Test");
        assert!(e.author_time > 0);
        assert_eq!(e.lineno, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blame_after_modification() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "first", "a.txt", "old1\nold2\n");
        commit(dir.path(), "second", "a.txt", "new1\nold2\n");

        let result = blame_file(dir.path(), "a.txt").await.unwrap();
        assert_eq!(result.entries.len(), 2);
        // Line 1 was changed in the second commit
        assert_eq!(result.entries[0].content, "new1");
        // Line 2 is from the first commit
        assert_eq!(result.entries[1].content, "old2");
        // They should have different commits
        assert_ne!(result.entries[0].commit, result.entries[1].commit);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blame_nonexistent_file_errors() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "initial", "a.txt", "hello\n");

        let r = blame_file(dir.path(), "nope.txt").await;
        assert!(r.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blame_non_repo_errors() {
        let fake = std::path::PathBuf::from("/this/path/does/not/exist/xyz");
        let r = blame_file(&fake, "a.txt").await;
        assert!(r.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blame_multiline_file() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let content = "first\nsecond\nthird\nfourth\nfifth\n";
        commit(dir.path(), "initial", "a.txt", content);

        let result = blame_file(dir.path(), "a.txt").await.unwrap();
        assert_eq!(result.entries.len(), 5);
        for (i, e) in result.entries.iter().enumerate() {
            assert_eq!(e.lineno, i as u32 + 1);
        }
        assert_eq!(result.entries[4].content, "fifth");
    }
}

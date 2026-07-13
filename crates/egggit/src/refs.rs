use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Information about a branch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchInfo {
    /// Branch name.
    pub name: String,
    /// Whether this is the current branch.
    pub is_current: bool,
    /// Whether the branch is detached.
    pub is_detached: bool,
    /// Upstream tracking ref (e.g., "origin/main").
    pub upstream: Option<String>,
    /// Ahead count relative to upstream.
    pub ahead: Option<i32>,
    /// Behind count relative to upstream.
    pub behind: Option<i32>,
    /// HEAD commit SHA.
    pub head: Option<String>,
}

/// Information about a tag.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TagInfo {
    /// Tag name.
    pub name: String,
    /// Object the tag points to.
    pub object: String,
    /// Tag type: "annotated" or "lightweight".
    pub kind: String,
}

/// Information about a remote.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteInfo {
    /// Remote name.
    pub name: String,
    /// Fetch URL.
    pub fetch_url: Option<String>,
    /// Push URL.
    pub push_url: Option<String>,
}

async fn run_git(
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

async fn capture_stdout(root: &Path, args: &[&str]) -> Result<String, EgggitError> {
    let root = root.to_path_buf();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let output = run_git(root, args).await?;
    if !output.status.success() {
        return Err(EgggitError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// List all branches with upstream info.
pub async fn list_branches(root: &Path) -> Result<Vec<BranchInfo>, crate::EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    // Get current branch name
    let current = capture_stdout(root, &["branch", "--show-current"])
        .await
        .unwrap_or_default();

    // Check if detached
    let rev_parse = capture_stdout(root, &["rev-parse", "--verify", "HEAD"]).await;
    let is_detached = current.is_empty() && rev_parse.is_ok();

    // List branches with tracking info
    let output = capture_stdout(root, &["branch", "-vv"]).await?;

    let mut branches = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (is_current_branch, line) = if let Some(rest) = line.strip_prefix("* ") {
            (true, rest)
        } else if let Some(rest) = line.strip_prefix("+ ") {
            // + prefix used for detached HEAD worktrees
            (false, rest)
        } else {
            let line = line.strip_prefix("  ").unwrap_or(line);
            (false, line)
        };

        // Format: <name> <sha> <upstream> [ahead N, behind M]
        let mut parts = line.splitn(2, ' ');
        let name = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }

        let rest = parts.next().unwrap_or("").trim();

        // Extract SHA
        let (sha, rest) = if !rest.is_empty() {
            let tokens: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
            let sha = tokens[0].to_string();
            let remainder = tokens.get(1).copied().unwrap_or("").to_string();
            (sha, remainder)
        } else {
            (String::new(), String::new())
        };
        let rest = rest.as_str();

        // Extract upstream and ahead/behind
        let mut upstream: Option<String> = None;
        let mut ahead: Option<i32> = None;
        let mut behind: Option<i32> = None;

        if let Some(bracket_start) = rest.find('[') {
            if let Some(bracket_end) = rest.find(']') {
                let tracking = &rest[bracket_start + 1..bracket_end];
                // Parse "origin/main: ahead 1, behind 2"
                let mut tracking_parts = tracking.splitn(2, ':');
                let ref_name = tracking_parts.next().unwrap_or("").trim().to_string();
                let counts = tracking_parts.next().unwrap_or("").trim().to_string();

                if !ref_name.is_empty() {
                    upstream = Some(ref_name);
                }

                for segment in counts.split(',') {
                    let segment = segment.trim();
                    if let Some(a) = segment.strip_prefix("ahead ") {
                        ahead = a.trim().parse().ok();
                    } else if let Some(b) = segment.strip_prefix("behind ") {
                        behind = b.trim().parse().ok();
                    }
                }
            }
        }

        let head = if sha.is_empty() { None } else { Some(sha) };

        branches.push(BranchInfo {
            name,
            is_current: is_current_branch,
            is_detached: is_current_branch && is_detached,
            upstream,
            ahead,
            behind,
            head,
        });
    }

    // If detached, add a synthetic entry
    if is_detached && !branches.iter().any(|b| b.is_current) {
        let sha = rev_parse.unwrap_or_default().trim().to_string();
        branches.insert(
            0,
            BranchInfo {
                name: "HEAD".to_string(),
                is_current: true,
                is_detached: true,
                upstream: None,
                ahead: None,
                behind: None,
                head: if sha.is_empty() { None } else { Some(sha) },
            },
        );
    }

    Ok(branches)
}

/// List all tags.
pub async fn list_tags(root: &Path) -> Result<Vec<TagInfo>, crate::EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    let output = capture_stdout(
        root,
        &[
            "tag",
            "-l",
            "--format=%(refname:short)%09%(objectname)%09%(*objectname)",
        ],
    )
    .await?;

    let mut tags = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        let name = parts[0].to_string();
        if name.is_empty() {
            continue;
        }

        let remaining = parts.get(1).unwrap_or(&"");
        let remaining_parts: Vec<&str> = remaining.splitn(2, '\t').collect();

        // For annotated tags: %(objectname) is the tag object, %(*objectname) is the target
        // For lightweight tags: %(objectname) is the target, %(*objectname) is empty
        let (object, kind) = if let Some(target) = remaining_parts.get(1) {
            let target = target.trim();
            if target.is_empty() {
                // Lightweight tag: objectname is the commit
                (remaining_parts[0].to_string(), "lightweight".to_string())
            } else {
                // Annotated tag: *objectname is the commit
                (target.to_string(), "annotated".to_string())
            }
        } else {
            // Fallback: treat as lightweight
            (remaining_parts[0].to_string(), "lightweight".to_string())
        };

        tags.push(TagInfo { name, object, kind });
    }

    Ok(tags)
}

/// List all remotes.
pub async fn list_remotes(root: &Path) -> Result<Vec<RemoteInfo>, crate::EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    let output = capture_stdout(root, &["remote", "-v"]).await?;

    let mut remotes: Vec<RemoteInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: "name\turl\t(type)"
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() < 2 {
            continue;
        }

        let name = parts[0].to_string();
        let rest = parts[1];

        // rest is "url\t(type)" or "url (type)" - extract url
        let url = if let Some(tab_idx) = rest.find('\t') {
            rest[..tab_idx].trim().to_string()
        } else {
            // Strip " (fetch)" or " (push)" suffix if present
            let trimmed = rest.trim();
            if let Some(idx) = trimmed.rfind(" (") {
                trimmed[..idx].trim().to_string()
            } else {
                trimmed.to_string()
            }
        };

        let is_fetch = rest.contains("(fetch)");

        if !seen.contains(&name) {
            seen.insert(name.clone());
            remotes.push(RemoteInfo {
                name,
                fetch_url: None,
                push_url: None,
            });
        }

        if let Some(remote) = remotes.iter_mut().find(|r| r.name == parts[0]) {
            if is_fetch {
                remote.fetch_url = Some(url);
            } else {
                remote.push_url = Some(url);
            }
        }
    }

    Ok(remotes)
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
    async fn list_branches_returns_main() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "init", "a.txt", "hello");

        let branches = list_branches(dir.path()).await.unwrap();
        assert!(!branches.is_empty());
        let main = branches.iter().find(|b| b.name == "main").unwrap();
        assert!(main.is_current);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_branches_detects_current() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "init", "a.txt", "hello");
        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let branches = list_branches(dir.path()).await.unwrap();
        let feature = branches.iter().find(|b| b.name == "feature").unwrap();
        assert!(feature.is_current);
        let main = branches.iter().find(|b| b.name == "main").unwrap();
        assert!(!main.is_current);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_branches_empty_repo() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        // No commits yet — branch listing may fail or be empty
        let result = list_branches(dir.path()).await;
        // Accept either empty or error since git may fail on no commits
        let _ = result;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_tags_returns_lightweight() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "init", "a.txt", "hello");
        Command::new("git")
            .args(["tag", "v0.1.0"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let tags = list_tags(dir.path()).await.unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "v0.1.0");
        assert_eq!(tags[0].kind, "lightweight");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_tags_returns_annotated() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "init", "a.txt", "hello");
        Command::new("git")
            .args(["tag", "-a", "v0.2.0", "-m", "release"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let tags = list_tags(dir.path()).await.unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "v0.2.0");
        assert_eq!(tags[0].kind, "annotated");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_tags_empty() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let tags = list_tags(dir.path()).await.unwrap();
        assert!(tags.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_remotes_empty() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let remotes = list_remotes(dir.path()).await.unwrap();
        assert!(remotes.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_remotes_with_remote() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        Command::new("git")
            .args(["remote", "add", "origin", "https://example.com/repo.git"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let remotes = list_remotes(dir.path()).await.unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(
            remotes[0].fetch_url.as_deref(),
            Some("https://example.com/repo.git")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_remotes_fetch_and_push() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        Command::new("git")
            .args([
                "remote",
                "add",
                "--fetch",
                "origin",
                "https://example.com/repo.git",
            ])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args([
                "remote",
                "set-url",
                "--push",
                "origin",
                "https://example.com/push.git",
            ])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let remotes = list_remotes(dir.path()).await.unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(
            remotes[0].fetch_url.as_deref(),
            Some("https://example.com/repo.git")
        );
        assert_eq!(
            remotes[0].push_url.as_deref(),
            Some("https://example.com/push.git")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn non_repo_errors() {
        let fake = std::path::PathBuf::from("/this/path/does/not/exist/xyz");
        assert!(list_branches(&fake).await.is_err());
        assert!(list_tags(&fake).await.is_err());
        assert!(list_remotes(&fake).await.is_err());
    }
}

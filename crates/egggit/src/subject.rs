//! Bounded, deterministic source-subject capture for execution provenance.
//!
//! The persisted result contains only HEAD and a digest. Paths and content are
//! transient inputs to the digest and are never returned to callers.

use crate::process;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

const MAX_PATHS: usize = 10_000;
const MAX_PATH_BYTES: usize = 4096;
const MAX_HASHED_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitSourceSubject {
    pub revision: String,
    pub dirty_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectCaptureError {
    NotGit,
    UnsafePath,
    BoundsExceeded,
    Failed(String),
}

impl std::fmt::Display for SubjectCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SubjectCaptureError {}

async fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, SubjectCaptureError> {
    let args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let out = process::run_bounded(&args, root, MAX_HASHED_BYTES)
        .await
        .map_err(|e| {
            if matches!(e, crate::EgggitError::OutputTooLarge) {
                SubjectCaptureError::BoundsExceeded
            } else {
                SubjectCaptureError::Failed(e.to_string())
            }
        })?;
    if !out.status.success() {
        if !root.join(".git").exists() {
            return Err(SubjectCaptureError::NotGit);
        }
        let error = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
        return Err(if error.contains("not a git repository") {
            SubjectCaptureError::NotGit
        } else {
            SubjectCaptureError::Failed("git subject capture failed".into())
        });
    }
    Ok(out.stdout)
}

/// Capture exact HEAD and a bounded digest of all dirty source state. Any
/// non-Unicode/unsafe path, submodule state, or bound overflow fails closed.
pub async fn capture_git_source_subject(
    root: &Path,
) -> Result<GitSourceSubject, SubjectCaptureError> {
    capture_inner(root, 0).await
}

async fn capture_inner(
    root: &Path,
    submodule_depth: usize,
) -> Result<GitSourceSubject, SubjectCaptureError> {
    const MAX_SUBMODULE_DEPTH: usize = 4;
    if submodule_depth > MAX_SUBMODULE_DEPTH {
        return Err(SubjectCaptureError::BoundsExceeded);
    }
    let head = git(root, &["rev-parse", "--verify", "HEAD"]).await?;
    let revision = String::from_utf8(head)
        .map_err(|_| SubjectCaptureError::Failed("invalid git object id".into()))?
        .trim()
        .to_owned();
    if revision.is_empty() || revision.len() > 64 {
        return Err(SubjectCaptureError::Failed("invalid git object id".into()));
    }
    // Include staged and unstaged tracked changes (including gitlink changes).
    let diff = git(
        root,
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--submodule=short",
            "HEAD",
            "--",
        ],
    )
    .await?;
    if diff.len() > MAX_HASHED_BYTES {
        return Err(SubjectCaptureError::BoundsExceeded);
    }
    // NUL-delimited porcelain preserves spaces and newlines in paths.
    let status = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
    )
    .await?;
    if status.is_empty() {
        return Ok(GitSourceSubject {
            revision,
            dirty_digest: None,
        });
    }
    let mut records = Vec::<(Vec<u8>, Vec<u8>)>::new();
    let mut parts = status.split(|b| *b == 0);
    while let Some(record) = parts.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 4 {
            return Err(SubjectCaptureError::Failed("malformed git status".into()));
        }
        let flags = record[..2].to_vec();
        let path = &record[3..];
        if path.is_empty()
            || path.len() > MAX_PATH_BYTES
            || !path.is_ascii() && std::str::from_utf8(path).is_err()
        {
            return Err(SubjectCaptureError::UnsafePath);
        }
        // Rename/copy porcelain records contain a second NUL-delimited path.
        if flags.contains(&b'R') || flags.contains(&b'C') {
            let _ = parts
                .next()
                .ok_or(SubjectCaptureError::Failed("missing rename source".into()))?;
        }
        if records.len() >= MAX_PATHS {
            return Err(SubjectCaptureError::BoundsExceeded);
        }
        records.push((path.to_vec(), flags));
    }
    records.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut hasher = Sha256::new();
    hasher.update(b"codegg-git-source-subject-v1\0");
    hasher.update((diff.len() as u64).to_be_bytes());
    hasher.update(&diff);
    let mut budget = diff.len();
    for (path, flags) in records {
        let path_str = std::str::from_utf8(&path).map_err(|_| SubjectCaptureError::UnsafePath)?;
        let relative = PathBuf::from(path_str);
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(SubjectCaptureError::UnsafePath);
        }
        let absolute = root.join(relative);
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(&path);
        hasher.update(&flags);
        match std::fs::symlink_metadata(&absolute) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target =
                    std::fs::read_link(&absolute).map_err(|_| SubjectCaptureError::UnsafePath)?;
                let target = target.to_str().ok_or(SubjectCaptureError::UnsafePath)?;
                hasher.update(b"symlink\0");
                hasher.update(target.as_bytes());
            }
            Ok(meta) if meta.is_file() => {
                if budget.saturating_add(meta.len() as usize) > MAX_HASHED_BYTES {
                    return Err(SubjectCaptureError::BoundsExceeded);
                }
                let bytes = std::fs::read(&absolute)
                    .map_err(|e| SubjectCaptureError::Failed(e.to_string()))?;
                budget = budget.saturating_add(bytes.len());
                if budget > MAX_HASHED_BYTES {
                    return Err(SubjectCaptureError::BoundsExceeded);
                }
                hasher.update(b"file\0");
                hasher.update((bytes.len() as u64).to_be_bytes());
                hasher.update(&bytes);
            }
            Ok(meta) if meta.is_dir() => {
                if absolute.join(".git").exists() {
                    if submodule_depth == MAX_SUBMODULE_DEPTH {
                        return Err(SubjectCaptureError::BoundsExceeded);
                    }
                    let nested = Box::pin(capture_inner(&absolute, submodule_depth + 1)).await?;
                    hasher.update(b"submodule\0");
                    hasher.update(nested.revision.as_bytes());
                    if let Some(digest) = nested.dirty_digest {
                        hasher.update(digest.as_bytes());
                    }
                } else {
                    hasher.update(b"directory\0");
                }
            }
            Ok(_) => {
                hasher.update(b"special\0");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                hasher.update(b"deleted\0");
            }
            Err(e) => return Err(SubjectCaptureError::Failed(e.to_string())),
        }
    }
    Ok(GitSourceSubject {
        revision,
        dirty_digest: Some(format!("{:x}", hasher.finalize())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(root: &Path, args: &[&str]) {
        let result = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fn repository() -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.name", "Fixture"]);
        git(
            dir.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        std::fs::write(dir.path().join("tracked.txt"), "base").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "base"]);
        dir
    }

    #[tokio::test]
    async fn clean_and_dirty_subjects_are_deterministic() {
        let dir = repository();
        let clean = capture_git_source_subject(dir.path()).await.unwrap();
        assert!(clean.dirty_digest.is_none());
        std::fs::write(dir.path().join("untracked.txt"), "contents").unwrap();
        let first = capture_git_source_subject(dir.path()).await.unwrap();
        let second = capture_git_source_subject(dir.path()).await.unwrap();
        assert_eq!(first, second);
        assert!(first.dirty_digest.is_some());
    }

    #[tokio::test]
    async fn staged_unstaged_and_deleted_paths_change_subject() {
        let dir = repository();
        std::fs::write(dir.path().join("tracked.txt"), "staged").unwrap();
        git(dir.path(), &["add", "tracked.txt"]);
        let staged = capture_git_source_subject(dir.path()).await.unwrap();
        std::fs::write(dir.path().join("tracked.txt"), "staged plus unstaged").unwrap();
        let unstaged = capture_git_source_subject(dir.path()).await.unwrap();
        assert_ne!(staged.dirty_digest, unstaged.dirty_digest);
        std::fs::remove_file(dir.path().join("tracked.txt")).unwrap();
        let deleted = capture_git_source_subject(dir.path()).await.unwrap();
        assert_ne!(unstaged.dirty_digest, deleted.dirty_digest);
    }

    #[tokio::test]
    async fn nested_submodule_dirty_state_is_included() {
        let submodule = repository();
        let parent = repository();
        let sub_path = submodule.path().to_str().unwrap();
        git(
            parent.path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                sub_path,
                "nested",
            ],
        );
        git(parent.path(), &["commit", "-am", "add submodule"]);
        std::fs::write(parent.path().join("nested/tracked.txt"), "submodule dirty").unwrap();
        let first = capture_git_source_subject(parent.path()).await.unwrap();
        std::fs::write(
            parent.path().join("nested/tracked.txt"),
            "different dirty state",
        )
        .unwrap();
        let second = capture_git_source_subject(parent.path()).await.unwrap();
        assert_ne!(first.dirty_digest, second.dirty_digest);
    }
}

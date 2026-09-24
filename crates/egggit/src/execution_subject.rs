//! Bounded capture of the Git subject observed at an execution boundary.
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{process, EgggitError};

const MAX_PATHS: usize = 4096;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PATH_LEN: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedExecutionSubject {
    pub revision: String,
    pub dirty: bool,
    pub dirty_digest: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SubjectCaptureError {
    #[error("workspace is not a Git repository")]
    NotGit,
    #[error("subject capture exceeded a safety bound")]
    BoundsExceeded,
    #[error("subject capture encountered an unsafe path")]
    UnsafePath,
    #[error("subject capture failed: {0}")]
    Failed(String),
}

/// Capture HEAD and a deterministic digest of staged, unstaged and untracked state.
/// `root` must be the scheduler-owned canonical workspace root.
pub async fn capture_execution_subject(
    root: &Path,
) -> Result<CapturedExecutionSubject, SubjectCaptureError> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || capture(&root))
        .await
        .map_err(|e| SubjectCaptureError::Failed(e.to_string()))?
}

fn capture(root: &Path) -> Result<CapturedExecutionSubject, SubjectCaptureError> {
    let head = process::run_sync(
        &[
            "rev-parse".into(),
            "--verify".into(),
            "HEAD^{commit}".into(),
        ],
        root,
    )
    .map_err(map_git)?;
    if !head.status.success() {
        return Err(SubjectCaptureError::NotGit);
    }
    let revision = String::from_utf8(head.stdout)
        .map_err(|_| SubjectCaptureError::UnsafePath)?
        .trim()
        .to_owned();
    if revision.is_empty()
        || revision.len() > 128
        || !revision.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(SubjectCaptureError::Failed("invalid HEAD object id".into()));
    }
    let status = process::run_sync(
        &[
            "status".into(),
            "--porcelain=v1".into(),
            "-z".into(),
            "--untracked-files=all".into(),
            "--ignore-submodules=none".into(),
        ],
        root,
    )
    .map_err(map_git)?;
    if !status.status.success() {
        return Err(SubjectCaptureError::Failed("git status failed".into()));
    }
    if status.stdout.is_empty() {
        return Ok(CapturedExecutionSubject {
            revision,
            dirty: false,
            dirty_digest: None,
        });
    }
    let mut entries = Vec::<(String, String)>::new();
    let mut parts = status.stdout.split(|b| *b == 0).filter(|p| !p.is_empty());
    while let Some(record) = parts.next() {
        if record.len() < 4 {
            return Err(SubjectCaptureError::UnsafePath);
        }
        let code = std::str::from_utf8(&record[..2])
            .map_err(|_| SubjectCaptureError::UnsafePath)?
            .to_owned();
        let name =
            std::str::from_utf8(&record[3..]).map_err(|_| SubjectCaptureError::UnsafePath)?;
        if name.len() > MAX_PATH_LEN || name.starts_with('/') || name.split('/').any(|c| c == "..")
        {
            return Err(SubjectCaptureError::UnsafePath);
        }
        entries.push((name.to_owned(), code.clone()));
        if entries.len() > MAX_PATHS {
            return Err(SubjectCaptureError::BoundsExceeded);
        }
        // Rename/copy porcelain has a second NUL-delimited source path.
        if code.contains('R') || code.contains('C') {
            let old = parts.next().ok_or(SubjectCaptureError::UnsafePath)?;
            let old = std::str::from_utf8(old).map_err(|_| SubjectCaptureError::UnsafePath)?;
            if old.len() > MAX_PATH_LEN {
                return Err(SubjectCaptureError::UnsafePath);
            }
            entries.push((old.to_owned(), "rename-source".into()));
        }
    }
    entries.sort();
    let mut digest = Sha256::new();
    // Raw no-abbrev diffs carry the index and worktree blob object ids, so two
    // different staged blobs cannot collapse to the same digest just because
    // the worktree bytes happen to match. Paths/counts were bounded above.
    for args in [
        vec!["diff", "--cached", "--raw", "--no-abbrev", "-z"],
        vec!["diff", "--raw", "--no-abbrev", "-z"],
    ] {
        let diff = process::run_sync(
            &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            root,
        )
        .map_err(map_git)?;
        if !diff.status.success() {
            return Err(SubjectCaptureError::Failed("git diff failed".into()));
        }
        if diff.stdout.len() > 4 * 1024 * 1024 {
            return Err(SubjectCaptureError::BoundsExceeded);
        }
        digest.update((diff.stdout.len() as u64).to_be_bytes());
        digest.update(&diff.stdout);
    }
    let mut total = 0u64;
    for (name, code) in entries {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((code.len() as u64).to_be_bytes());
        digest.update(code.as_bytes());
        let path = root.join(PathBuf::from(&name));
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::read_link(&path)
                    .map_err(|e| SubjectCaptureError::Failed(e.to_string()))?;
                let target = target.to_str().ok_or(SubjectCaptureError::UnsafePath)?;
                digest.update(b"symlink\0");
                digest.update(target.as_bytes());
            }
            Ok(meta) if meta.is_file() => {
                total = total.saturating_add(meta.len());
                if total > MAX_BYTES {
                    return Err(SubjectCaptureError::BoundsExceeded);
                }
                let bytes =
                    std::fs::read(&path).map_err(|e| SubjectCaptureError::Failed(e.to_string()))?;
                digest.update(b"file\0");
                digest.update((bytes.len() as u64).to_be_bytes());
                digest.update(Sha256::digest(bytes));
            }
            Ok(meta) if meta.is_dir() => {
                digest.update(b"directory\0");
            }
            Ok(_) => {
                digest.update(b"other\0");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                digest.update(b"deleted\0");
            }
            Err(e) => return Err(SubjectCaptureError::Failed(e.to_string())),
        }
    }
    Ok(CapturedExecutionSubject {
        revision,
        dirty: true,
        dirty_digest: Some(format!("{:x}", digest.finalize())),
    })
}

fn map_git(error: EgggitError) -> SubjectCaptureError {
    SubjectCaptureError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn clean_and_dirty_capture_is_deterministic() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        git(root, &["init", "-q"]);
        std::fs::write(root.join("file"), "one").unwrap();
        git(root, &["add", "file"]);
        git(root, &["commit", "-qm", "fixture"]);
        let clean = capture(root).unwrap();
        assert!(!clean.dirty);
        std::fs::write(root.join("file"), "two").unwrap();
        let first = capture(root).unwrap();
        let second = capture(root).unwrap();
        assert!(first.dirty);
        assert_eq!(first, second);
        assert_ne!(first.dirty_digest, clean.dirty_digest);
    }
}

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("path contains NUL byte")]
    NullByte,
    #[error("absolute path not allowed in repository-relative context: {0}")]
    AbsolutePath(String),
    #[error("path escapes repository root: {0}")]
    PathEscape(String),
    #[error("path is empty")]
    Empty,
    #[error("path is not valid UTF-8")]
    NotUtf8,
}

/// Canonical repository root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoRoot(PathBuf);

impl RepoRoot {
    /// Create a new RepoRoot. The path is canonicalized on creation.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, PathError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(PathError::Empty);
        }
        let canonical = if path.exists() {
            path.canonicalize()
                .map_err(|e| PathError::PathEscape(format!("{}: {e}", path.display())))?
        } else {
            path
        };
        Ok(Self(canonical))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn as_str(&self) -> Option<&str> {
        self.0.to_str()
    }
}

/// Repository-relative literal path. Rejects NUL bytes, absolute paths,
/// parent traversal, and paths resolving outside the repository root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoPath(String);

impl RepoPath {
    /// Validate and create a repository-relative path.
    pub fn new(repo_root: &RepoRoot, path: &str) -> Result<Self, PathError> {
        if path.is_empty() {
            return Err(PathError::Empty);
        }
        if path.contains('\0') {
            return Err(PathError::NullByte);
        }
        if Path::new(path).is_absolute() {
            return Err(PathError::AbsolutePath(path.to_owned()));
        }
        let normalized = normalize_path(path);
        if normalized.starts_with("..") || normalized.contains("/../") {
            return Err(PathError::PathEscape(path.to_owned()));
        }
        let resolved = repo_root.as_path().join(&normalized);
        let canonical = if resolved.exists() {
            resolved
                .canonicalize()
                .map_err(|e| PathError::PathEscape(format!("{path}: {e}")))?
        } else {
            resolved
        };
        if !canonical.starts_with(repo_root.as_path()) {
            return Err(PathError::PathEscape(path.to_owned()));
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Raw advanced pathspec — used when literal path validation isn't possible
/// (e.g., glob patterns, regex pathspecs).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pathspec(String);

impl Pathspec {
    pub fn new(spec: &str) -> Result<Self, PathError> {
        if spec.is_empty() {
            return Err(PathError::Empty);
        }
        if spec.contains('\0') {
            return Err(PathError::NullByte);
        }
        Ok(Self(spec.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Simple path normalization: collapse `./` prefixes.
/// Trailing slashes are preserved — they carry semantic meaning in git (directory vs file).
fn normalize_path(path: &str) -> String {
    let p = path.trim_start_matches("./");
    if p.is_empty() {
        ".".to_owned()
    } else {
        p.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_new_exists() {
        let root = RepoRoot::new("/tmp").unwrap();
        assert!(root.as_path().exists());
        assert!(root.as_str().is_some());
    }

    #[test]
    fn repo_root_new_empty() {
        assert!(matches!(RepoRoot::new(""), Err(PathError::Empty)));
    }

    #[test]
    fn repo_path_rejects_absolute() {
        let root = RepoRoot::new("/tmp").unwrap();
        assert!(matches!(
            RepoPath::new(&root, "/etc/passwd"),
            Err(PathError::AbsolutePath(_))
        ));
    }

    #[test]
    fn repo_path_rejects_null_byte() {
        let root = RepoRoot::new("/tmp").unwrap();
        assert!(matches!(
            RepoPath::new(&root, "foo\0bar"),
            Err(PathError::NullByte)
        ));
    }

    #[test]
    fn repo_path_rejects_parent_traversal() {
        let root = RepoRoot::new("/tmp").unwrap();
        assert!(matches!(
            RepoPath::new(&root, "../etc/passwd"),
            Err(PathError::PathEscape(_))
        ));
    }

    #[test]
    fn repo_path_rejects_empty() {
        let root = RepoRoot::new("/tmp").unwrap();
        assert!(matches!(RepoPath::new(&root, ""), Err(PathError::Empty)));
    }

    #[test]
    fn repo_path_accepts_simple() {
        let root = RepoRoot::new("/tmp").unwrap();
        let rp = RepoPath::new(&root, "src/main.rs").unwrap();
        assert_eq!(rp.as_str(), "src/main.rs");
    }

    #[test]
    fn repo_path_normalizes_dot_prefix() {
        let root = RepoRoot::new("/tmp").unwrap();
        let rp = RepoPath::new(&root, "./src/main.rs").unwrap();
        assert_eq!(rp.as_str(), "src/main.rs");
    }

    #[test]
    fn pathspec_accepts_glob() {
        let ps = Pathspec::new("src/**/*.rs").unwrap();
        assert_eq!(ps.as_str(), "src/**/*.rs");
    }

    #[test]
    fn pathspec_rejects_empty() {
        assert!(matches!(Pathspec::new(""), Err(PathError::Empty)));
    }

    #[test]
    fn pathspec_rejects_null() {
        assert!(matches!(Pathspec::new("a\0b"), Err(PathError::NullByte)));
    }
}

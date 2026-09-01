//! Durable, workspace-local source storage for Tool Programs.
//!
//! A submitted program carries an immutable reference rather than its source
//! body. The scheduler executor resolves that reference from the workspace
//! lease and verifies the SHA-256 digest before parsing or executing it.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

/// Maximum source body accepted by the Tool Program submission path.
pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;

/// Reference persisted in a Tool Program job payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolProgramSourceRef {
    pub digest: String,
    pub length: u64,
    pub relative_path: String,
}

#[derive(Debug, Error)]
pub enum ToolProgramSourceError {
    #[error("source exceeds maximum size of {MAX_SOURCE_BYTES} bytes")]
    Oversized,
    #[error("source digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("source reference is invalid: {0}")]
    InvalidReference(String),
    #[error("source not found: {0}")]
    NotFound(String),
    #[error("source storage I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Workspace-local content-addressed source store.
pub struct ToolProgramSourceStore {
    base_dir: PathBuf,
}

impl ToolProgramSourceStore {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            base_dir: workspace_root.join(".codegg").join("tool_program_sources"),
        }
    }

    /// Persist source atomically and return its immutable reference.
    pub fn persist(&self, source: &str) -> Result<ToolProgramSourceRef, ToolProgramSourceError> {
        let bytes = source.as_bytes();
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(ToolProgramSourceError::Oversized);
        }

        let digest = digest(source);
        let relative_path = format!("{digest}.py");
        let target = self.base_dir.join(&relative_path);

        if let Ok(existing) = std::fs::read(&target) {
            let actual = digest_bytes(&existing);
            if actual != digest {
                return Err(ToolProgramSourceError::DigestMismatch {
                    expected: digest,
                    actual,
                });
            }
            return Ok(ToolProgramSourceRef {
                digest,
                length: bytes.len() as u64,
                relative_path,
            });
        }

        std::fs::create_dir_all(&self.base_dir)?;
        let temp = self
            .base_dir
            .join(format!(".{digest}.{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temp, bytes)?;
        if let Err(error) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            // Another identical submit may have won the race. Verify the
            // existing winner before accepting it.
            if let Ok(existing) = std::fs::read(&target) {
                if digest_bytes(&existing) == digest {
                    return Ok(ToolProgramSourceRef {
                        digest,
                        length: bytes.len() as u64,
                        relative_path,
                    });
                }
            }
            return Err(error.into());
        }

        Ok(ToolProgramSourceRef {
            digest,
            length: bytes.len() as u64,
            relative_path,
        })
    }

    /// Retrieve and verify a source reference.
    pub fn retrieve(
        &self,
        reference: &ToolProgramSourceRef,
    ) -> Result<String, ToolProgramSourceError> {
        if reference.relative_path != format!("{}.py", reference.digest)
            || reference.relative_path.contains("..")
            || reference.relative_path.starts_with('/')
        {
            return Err(ToolProgramSourceError::InvalidReference(
                reference.relative_path.clone(),
            ));
        }
        let path = self.base_dir.join(&reference.relative_path);
        let mut source_file = open_source_file(&path, &reference.relative_path)?;
        let metadata = source_file
            .metadata()
            .map_err(|_| ToolProgramSourceError::NotFound(reference.digest.clone()))?;
        if !metadata.is_file() {
            return Err(ToolProgramSourceError::InvalidReference(
                reference.relative_path.clone(),
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        std::io::Read::read_to_end(&mut source_file, &mut bytes)
            .map_err(|_| ToolProgramSourceError::NotFound(reference.digest.clone()))?;
        if bytes.len() as u64 != reference.length {
            return Err(ToolProgramSourceError::DigestMismatch {
                expected: reference.digest.clone(),
                actual: digest_bytes(&bytes),
            });
        }
        let actual = digest_bytes(&bytes);
        if actual != reference.digest {
            return Err(ToolProgramSourceError::DigestMismatch {
                expected: reference.digest.clone(),
                actual,
            });
        }
        String::from_utf8(bytes)
            .map_err(|_| ToolProgramSourceError::InvalidReference(reference.digest.clone()))
    }
}

#[cfg(unix)]
fn open_source_file(
    path: &Path,
    relative_path: &str,
) -> Result<std::fs::File, ToolProgramSourceError> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ToolProgramSourceError::NotFound(relative_path.to_string())
            } else if error.raw_os_error() == Some(libc::ELOOP) {
                ToolProgramSourceError::InvalidReference(relative_path.to_string())
            } else {
                ToolProgramSourceError::Io(error)
            }
        })
}

#[cfg(not(unix))]
fn open_source_file(
    path: &Path,
    relative_path: &str,
) -> Result<std::fs::File, ToolProgramSourceError> {
    std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ToolProgramSourceError::NotFound(relative_path.to_string())
        } else {
            ToolProgramSourceError::Io(error)
        }
    })
}

pub fn digest(source: &str) -> String {
    digest_bytes(source.as_bytes())
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_and_retrieve_verifies_content() {
        let temp = tempfile::tempdir().unwrap();
        let store = ToolProgramSourceStore::new(temp.path());
        let reference = store.persist("emit({\"ok\": true})\n").unwrap();
        assert_eq!(
            store.retrieve(&reference).unwrap(),
            "emit({\"ok\": true})\n"
        );
    }

    #[test]
    fn tampering_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let store = ToolProgramSourceStore::new(temp.path());
        let reference = store.persist("emit(1)\n").unwrap();
        std::fs::write(
            temp.path()
                .join(".codegg/tool_program_sources")
                .join(&reference.relative_path),
            "emit(2)\n",
        )
        .unwrap();
        assert!(matches!(
            store.retrieve(&reference),
            Err(ToolProgramSourceError::DigestMismatch { .. })
        ));
    }

    #[test]
    fn traversal_reference_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let store = ToolProgramSourceStore::new(temp.path());
        let reference = ToolProgramSourceRef {
            digest: "abc".into(),
            length: 0,
            relative_path: "../abc.py".into(),
        };
        assert!(matches!(
            store.retrieve(&reference),
            Err(ToolProgramSourceError::InvalidReference(_))
        ));
    }
}

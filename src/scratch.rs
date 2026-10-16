//! Daemon-owned scratch resources for model execution.
//!
//! Session directory names are derived from a digest of the canonical session
//! identity. Callers never provide a filesystem path, and allocation never
//! searches existing directories to infer ownership.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

const SCRATCH_VENDOR_NAME: &str = "codegg";
const SCRATCH_ROOT_NAME: &str = "scratch";
const OWNER_RECORD_NAME: &str = ".codegg-owner";

#[derive(Debug, Error)]
pub enum ScratchError {
    #[error("session identity is missing")]
    MissingSession,
    #[error("could not determine the current user's cache directory")]
    CacheUnavailable,
    #[error("failed to provision managed scratch: {0}")]
    Io(#[from] std::io::Error),
    #[error("managed scratch path is not a real directory: {0}")]
    InvalidPath(PathBuf),
    #[error("managed scratch path is owned by a different user: {0}")]
    WrongOwner(PathBuf),
    #[error("could not determine the current user's home-directory owner")]
    OwnerUnavailable,
}

/// Resolve and securely create the persistent scratch root for one session.
/// The identity is supplied by the host execution context, never by tool
/// arguments. Repeated calls, including calls after restart, converge on the
/// same owner-specific directory.
pub fn session_scratch_dir(session_id: &str) -> Result<PathBuf, ScratchError> {
    if session_id.is_empty() {
        return Err(ScratchError::MissingSession);
    }
    // Keep disposable artifacts outside `Library/Application Support/codegg`
    // on macOS, which the Required sandbox correctly denies as a credential
    // root. Cache storage remains per-user and stable across normal daemon and
    // process restarts; the artifacts are explicitly disposable.
    let cache_root = dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)?;
    std::fs::create_dir_all(&cache_root)?;
    let vendor_root = cache_root.join(SCRATCH_VENDOR_NAME);
    let scratch_root = vendor_root.join(SCRATCH_ROOT_NAME);
    let sessions_root = scratch_root.join("sessions");
    #[cfg(unix)]
    let expected_uid = {
        use std::os::unix::fs::MetadataExt;
        let home = dirs::home_dir().ok_or(ScratchError::OwnerUnavailable)?;
        std::fs::metadata(home)
            .map_err(|_| ScratchError::OwnerUnavailable)?
            .uid()
    };
    #[cfg(not(unix))]
    let expected_uid = ();
    ensure_private_directory(&vendor_root, expected_uid)?;
    ensure_private_directory(&scratch_root, expected_uid)?;
    ensure_private_directory(&sessions_root, expected_uid)?;

    let digest = Sha256::digest(session_id.as_bytes());
    let name = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let session_root = sessions_root.join(name);
    ensure_private_directory(&session_root, expected_uid)?;
    ensure_owner_record(&session_root, &digest)?;
    Ok(std::fs::canonicalize(session_root)?)
}

fn ensure_owner_record(session_root: &Path, digest: &[u8]) -> Result<(), ScratchError> {
    use std::io::{Read, Write};

    let record_path = session_root.join(OWNER_RECORD_NAME);
    let mut record = b"codegg-session-scratch-v1\n".to_vec();
    for byte in digest {
        record.extend_from_slice(format!("{byte:02x}").as_bytes());
    }
    record.push(b'\n');

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&record_path) {
        Ok(mut file) => {
            file.write_all(&record)?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let mut matches = false;
            let mut observed = Vec::new();
            for attempt in 0..20 {
                let metadata = std::fs::symlink_metadata(&record_path)?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(ScratchError::InvalidPath(record_path));
                }
                observed.clear();
                std::fs::File::open(&record_path)?.read_to_end(&mut observed)?;
                if observed == record {
                    matches = true;
                    break;
                }
                if attempt < 19 {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            if !matches {
                // A daemon may stop after create_new but before the complete
                // owner record reaches disk. Recover only an exact prefix of
                // the host-computed record; malformed or foreign records stay
                // fail-closed. The enclosing directory is already owner-only.
                if !record.starts_with(&observed) || observed.len() >= record.len() {
                    return Err(ScratchError::InvalidPath(record_path));
                }
                std::fs::remove_file(&record_path)?;
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&record_path)?;
                file.write_all(&record)?;
                file.sync_all()?;
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn ensure_private_directory(
    path: &Path,
    expected_uid: impl OwnerCheck,
) -> Result<(), ScratchError> {
    if let Some(parent) = path.parent() {
        parent
            .canonicalize_parent_and_self()
            .map_err(|_| ScratchError::InvalidPath(parent.to_path_buf()))?;
    }
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    let canonical = std::fs::canonicalize(path)?;
    if canonical != path.canonicalize_parent_and_self()? {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != expected_uid.uid() {
            return Err(ScratchError::WrongOwner(path.to_path_buf()));
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
trait OwnerCheck {
    fn uid(self) -> u32;
}

#[cfg(unix)]
impl OwnerCheck for u32 {
    fn uid(self) -> u32 {
        self
    }
}

#[cfg(not(unix))]
trait OwnerCheck {}

#[cfg(not(unix))]
impl OwnerCheck for () {}

trait CanonicalPathCheck {
    fn canonicalize_parent_and_self(&self) -> Result<PathBuf, std::io::Error>;
}

impl CanonicalPathCheck for Path {
    fn canonicalize_parent_and_self(&self) -> Result<PathBuf, std::io::Error> {
        // Reject symlinked path components too, not only a symlink at the
        // leaf. `canonicalize` alone would silently follow them.
        let mut current = PathBuf::new();
        for component in self.components() {
            current.push(component);
            if current.exists() {
                let metadata = std::fs::symlink_metadata(&current)?;
                if metadata.file_type().is_symlink() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "scratch path contains a symlink",
                    ));
                }
            }
        }
        std::fs::canonicalize(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_paths_are_stable_and_separate() {
        let first = session_scratch_dir("scratch-test-session-a").unwrap();
        let same = session_scratch_dir("scratch-test-session-a").unwrap();
        let other = session_scratch_dir("scratch-test-session-b").unwrap();
        assert_eq!(first, same);
        assert_ne!(first, other);
        assert!(first.is_dir());
        assert!(other.is_dir());
        assert!(!first.to_string_lossy().contains("scratch-test-session-a"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        std::fs::remove_dir_all(first).unwrap();
        std::fs::remove_dir_all(other).unwrap();
    }

    #[test]
    fn concurrent_allocations_converge_on_one_owned_directory() {
        let session_id = format!("scratch-concurrent-{}", uuid::Uuid::new_v4());
        let workers = (0..8)
            .map(|_| {
                let session_id = session_id.clone();
                std::thread::spawn(move || session_scratch_dir(&session_id).unwrap())
            })
            .collect::<Vec<_>>();
        let paths = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert!(paths.iter().all(|path| path == &paths[0]));
        let owner_record = std::fs::read_to_string(paths[0].join(OWNER_RECORD_NAME)).unwrap();
        assert!(owner_record.starts_with("codegg-session-scratch-v1\n"));
        std::fs::remove_dir_all(&paths[0]).unwrap();
    }

    #[test]
    fn owner_record_recovers_from_an_interrupted_prefix() {
        let root = tempfile::tempdir().unwrap();
        let digest = Sha256::digest(b"interrupted-owner-record");
        let record_path = root.path().join(OWNER_RECORD_NAME);
        std::fs::write(&record_path, b"codegg-session-scratch-v1\n").unwrap();

        ensure_owner_record(root.path(), &digest).unwrap();

        let actual = std::fs::read(&record_path).unwrap();
        assert!(actual.starts_with(b"codegg-session-scratch-v1\n"));
        assert_eq!(actual.len(), b"codegg-session-scratch-v1\n".len() + 65);
    }

    #[test]
    fn empty_identity_is_refused() {
        assert!(matches!(
            session_scratch_dir(""),
            Err(ScratchError::MissingSession)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn allocation_refuses_a_preexisting_symlink() {
        let parent = tempfile::tempdir().unwrap();
        let target = parent.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = parent.path().join("linked-root");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            ensure_private_directory(&link, 0),
            Err(ScratchError::InvalidPath(_))
        ));
    }

    #[tokio::test]
    async fn structured_file_tools_are_scoped_to_the_host_session() {
        use crate::tool::Tool;

        let owner = format!("scratch-file-owner-{}", uuid::Uuid::new_v4());
        let other = format!("scratch-file-other-{}", uuid::Uuid::new_v4());
        let owner_root = session_scratch_dir(&owner).unwrap();
        let other_root = session_scratch_dir(&other).unwrap();
        let target = owner_root.join("artifact.txt");
        let mut owner_context =
            crate::tool::ToolExecutionContext::with_backend(crate::tool::ToolBackendKind::Native);
        owner_context.session_id = Some(owner.clone());
        let write = crate::tool::write::WriteTool::new();
        write
            .execute_structured(
                serde_json::json!({ "path": target.clone(), "content": "private artifact" }),
                Some(owner_context.clone()),
            )
            .await
            .unwrap();

        let read = crate::tool::read::ReadTool::new();
        let output = read
            .execute_structured(
                serde_json::json!({ "path": target.clone() }),
                Some(owner_context.clone()),
            )
            .await
            .unwrap();
        assert!(output.output.contains("private artifact"));

        assert!(write
            .execute_structured(
                serde_json::json!({
                    "path": owner_root.parent().unwrap().join("parent-write.txt"),
                    "content": "must not be writable"
                }),
                Some(owner_context.clone()),
            )
            .await
            .is_err());

        #[cfg(unix)]
        {
            let outside = tempfile::NamedTempFile::new().unwrap();
            let escape = owner_root.join("escape-link");
            std::os::unix::fs::symlink(outside.path(), &escape).unwrap();
            assert!(read
                .execute_structured(
                    serde_json::json!({ "path": escape }),
                    Some(owner_context.clone()),
                )
                .await
                .is_err());
        }

        let mut other_context =
            crate::tool::ToolExecutionContext::with_backend(crate::tool::ToolBackendKind::Native);
        other_context.session_id = Some(other.clone());
        assert!(read
            .execute_structured(serde_json::json!({ "path": target }), Some(other_context),)
            .await
            .is_err());
        std::fs::remove_dir_all(owner_root).unwrap();
        std::fs::remove_dir_all(other_root).unwrap();
    }
}

//! Daemon-owned scratch resources for model execution.
//!
//! Session directory names are derived from a digest of the canonical session
//! identity. Callers never provide a filesystem path, and allocation never
//! searches existing directories to infer ownership.

use std::path::{Path, PathBuf};
#[cfg(any(unix, windows))]
use std::sync::{Mutex, OnceLock};

#[cfg(any(unix, windows))]
static SHARED_RECLAIM_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentScratchScope {
    pub session_id: String,
    pub run_id: codegg_core::identity::AgentRunId,
    pub private_root: PathBuf,
    pub shared: Vec<AuthorizedSharedScratchRoot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedSharedScratchRoot {
    pub scratch_id: codegg_core::identity::ScratchId,
    pub alias: String,
    pub path: PathBuf,
}

impl AgentScratchScope {
    pub fn from_authorized_grants(
        session_id: String,
        run_id: codegg_core::identity::AgentRunId,
        grants: &[codegg_core::agent_run::SharedScratchGrant],
    ) -> Result<Self, ScratchError> {
        let private_root = agent_run_scratch_dir(&session_id, &run_id)?;
        let shared = grants
            .iter()
            .map(|grant| {
                Ok(AuthorizedSharedScratchRoot {
                    scratch_id: grant.scratch_id.clone(),
                    alias: grant.alias.clone(),
                    path: shared_scratch_dir(&session_id, &grant.scratch_id)?,
                })
            })
            .collect::<Result<Vec<_>, ScratchError>>()?;
        Ok(Self {
            session_id,
            run_id,
            private_root,
            shared,
        })
    }

    pub fn prompt_block(&self) -> String {
        let mut text = format!(
            "This delegated run has private scratch at {}. Temporary files belong there. ",
            self.private_root.display()
        );
        if self.shared.is_empty() {
            text.push_str("It has no shared scratch grants.");
        } else {
            text.push_str(
                "Explicit shared scratch grants (write only when your task requires it):",
            );
            for shared in &self.shared {
                let env_key = shared.alias.to_ascii_uppercase().replace('-', "_");
                text.push_str(&format!(
                    "\n- {} via CODEGG_SHARED_SCRATCH_{}: {}",
                    shared.alias,
                    env_key,
                    shared.path.display()
                ));
            }
        }
        text
    }
}

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
    #[error("shared scratch reclamation is unavailable on this platform")]
    CleanupUnavailable,
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
    let cache_root = dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)?;
    std::fs::create_dir_all(&cache_root)?;
    ensure_private_parent(&cache_root, expected_uid)?;
    let vendor_root = cache_root.join(SCRATCH_VENDOR_NAME);
    let scratch_root = vendor_root.join(SCRATCH_ROOT_NAME);
    let sessions_root = scratch_root.join("sessions");
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

/// Resolve the private scratch root for one durable delegated agent run.
/// The typed run identity is host-owned; callers never provide a path.
pub fn agent_run_scratch_dir(
    session_id: &str,
    run_id: &codegg_core::identity::AgentRunId,
) -> Result<PathBuf, ScratchError> {
    let session_root = session_scratch_dir(session_id)?;
    let runs_root = session_root.join("runs");
    let owner = directory_owner(&session_root)?;
    ensure_private_directory(&runs_root, owner)?;
    scoped_resource_dir(&runs_root, "agent-run", run_id.as_str(), owner)
}

/// Resolve a shared resource locator. Authorization is deliberately checked
/// by AgentRunStore before this host-only filesystem helper is called.
pub fn shared_scratch_dir(
    session_id: &str,
    scratch_id: &codegg_core::identity::ScratchId,
) -> Result<PathBuf, ScratchError> {
    let session_root = session_scratch_dir(session_id)?;
    let shared_root = session_root.join("shared");
    let owner = directory_owner(&session_root)?;
    ensure_private_directory(&shared_root, owner)?;
    scoped_resource_dir(&shared_root, "shared-scratch", scratch_id.as_str(), owner)
}

/// Remove a shared cache directory after the durable store proves it has been
/// revoked and has no admitted attempt leases. Unknown paths are never read
/// from model input; the leaf is reconstructed from the typed resource ID.
#[cfg(any(unix, windows))]
pub(crate) fn reclaim_shared_scratch_dir(
    session_id: &str,
    scratch_id: &codegg_core::identity::ScratchId,
) -> Result<bool, ScratchError> {
    let _guard = SHARED_RECLAIM_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let session_root = session_scratch_dir(session_id)?;
    let shared_root = session_root.join("shared");
    let owner = directory_owner(&session_root)?;
    ensure_private_directory(&shared_root, owner)?;
    reclaim_owned_resource_dir(
        &shared_root,
        &resource_digest("shared-scratch", scratch_id.as_str()),
        owner,
    )
}

/// Remove an agent run's private scratch after its durable run has reached a
/// terminal state. The run ID is converted to the same host-owned locator as
/// allocation; callers never pass a filesystem path.
#[cfg(any(unix, windows))]
pub(crate) fn reclaim_agent_run_scratch_dir(
    session_id: &str,
    run_id: &codegg_core::identity::AgentRunId,
) -> Result<bool, ScratchError> {
    let _guard = SHARED_RECLAIM_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(session_root) = existing_session_scratch_dir(session_id)? else {
        return Ok(false);
    };
    let runs_root = session_root.join("runs");
    if !runs_root.exists() {
        return Ok(false);
    }
    let owner = directory_owner(&session_root)?;
    ensure_private_directory(&runs_root, owner)?;
    reclaim_owned_resource_dir(
        &runs_root,
        &resource_digest("agent-run", run_id.as_str()),
        owner,
    )
}

/// Remove all scratch under a deleted session after durable jobs and runs are
/// terminal. Session transcripts and child Git results live in their own
/// stores and are not affected.
#[cfg(any(unix, windows))]
pub(crate) fn reclaim_deleted_session_scratch_dir(session_id: &str) -> Result<bool, ScratchError> {
    let _guard = SHARED_RECLAIM_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(session_root) = existing_session_scratch_dir(session_id)? else {
        return Ok(false);
    };
    let sessions_root = session_root
        .parent()
        .ok_or_else(|| ScratchError::InvalidPath(session_root.clone()))?;
    let owner = directory_owner(sessions_root)?;
    reclaim_owned_resource_dir(sessions_root, &Sha256::digest(session_id.as_bytes()), owner)
}

fn existing_session_scratch_dir(session_id: &str) -> Result<Option<PathBuf>, ScratchError> {
    if session_id.is_empty() {
        return Err(ScratchError::MissingSession);
    }
    let digest = Sha256::digest(session_id.as_bytes());
    let name = hex_digest(&digest);
    let cache_root = dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)?;
    let path = cache_root
        .join(SCRATCH_VENDOR_NAME)
        .join(SCRATCH_ROOT_NAME)
        .join("sessions")
        .join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => session_scratch_dir(session_id).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn reclaim_agent_run_scratch_dir(
    _session_id: &str,
    _run_id: &codegg_core::identity::AgentRunId,
) -> Result<bool, ScratchError> {
    Err(ScratchError::CleanupUnavailable)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn reclaim_deleted_session_scratch_dir(_session_id: &str) -> Result<bool, ScratchError> {
    Err(ScratchError::CleanupUnavailable)
}

#[cfg(any(unix, windows))]
fn reclaim_owned_resource_dir(
    parent: &Path,
    digest: &[u8],
    owner: impl OwnerCheck,
) -> Result<bool, ScratchError> {
    let name = hex_digest(digest);
    let path = parent.join(&name);
    let canonical_parent = std::fs::canonicalize(parent)?;
    let recovered = recover_reclaim_tombstones(parent, digest, owner)?;
    let tombstone = parent.join(format!(".reclaim-{name}-{}", uuid::Uuid::new_v4()));
    match std::fs::rename(&path, &tombstone) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(recovered),
        Err(error) => return Err(error.into()),
    }
    validate_and_remove_tombstone(&tombstone, &canonical_parent, digest, owner)?;
    Ok(true)
}

/// Other platforms stay fail-closed until their filesystem metadata and
/// recursive removal semantics are qualified.
#[cfg(not(any(unix, windows)))]
pub(crate) fn reclaim_shared_scratch_dir(
    _session_id: &str,
    _scratch_id: &codegg_core::identity::ScratchId,
) -> Result<bool, ScratchError> {
    Err(ScratchError::CleanupUnavailable)
}

#[cfg(any(unix, windows))]
fn recover_reclaim_tombstones(
    shared_root: &Path,
    digest: &[u8],
    owner: impl OwnerCheck,
) -> Result<bool, ScratchError> {
    let prefix = format!(".reclaim-{}-", hex_digest(digest));
    let tombstones = std::fs::read_dir(shared_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect::<Vec<_>>();
    let canonical_shared_root = std::fs::canonicalize(shared_root)?;
    let mut recovered = false;
    for previous in tombstones {
        // Claim an orphaned tombstone under a fresh opaque name. A concurrent
        // retry can claim it first; either claimant removes only this revoked
        // resource generation, never a directory recreated at its live name.
        let claimed = shared_root.join(format!("{prefix}{}", uuid::Uuid::new_v4()));
        match std::fs::rename(&previous, &claimed) {
            Ok(()) => {
                validate_and_remove_tombstone(&claimed, &canonical_shared_root, digest, owner)?;
                recovered = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(recovered)
}

#[cfg(any(unix, windows))]
fn validate_and_remove_tombstone(
    tombstone: &Path,
    canonical_shared_root: &Path,
    digest: &[u8],
    owner: impl OwnerCheck,
) -> Result<(), ScratchError> {
    let metadata = std::fs::symlink_metadata(tombstone)?;
    if is_symlink_or_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(ScratchError::InvalidPath(tombstone.to_path_buf()));
    }
    let expected_tombstone = canonical_shared_root.join(
        tombstone
            .file_name()
            .ok_or_else(|| ScratchError::InvalidPath(tombstone.to_path_buf()))?,
    );
    if std::fs::canonicalize(tombstone)? != expected_tombstone {
        return Err(ScratchError::InvalidPath(tombstone.to_path_buf()));
    }
    let owner_record = tombstone.join(OWNER_RECORD_NAME);
    let owner_metadata = std::fs::symlink_metadata(&owner_record)?;
    if is_symlink_or_reparse_point(&owner_metadata) || !owner_metadata.is_file() {
        return Err(ScratchError::InvalidPath(owner_record));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if owner_metadata.uid() != owner.uid() || owner_metadata.mode() & 0o077 != 0 {
            return Err(ScratchError::WrongOwner(owner_record));
        }
    }
    let record = std::fs::read(&owner_record)?;
    let mut expected_record = b"codegg-session-scratch-v1\n".to_vec();
    expected_record.extend_from_slice(hex_digest(digest).as_bytes());
    expected_record.push(b'\n');
    if record != expected_record {
        return Err(ScratchError::InvalidPath(tombstone.to_path_buf()));
    }
    validate_reclaim_tree(tombstone)?;
    std::fs::remove_dir_all(tombstone)?;
    Ok(())
}

#[cfg(any(unix, windows))]
fn validate_reclaim_tree(path: &Path) -> Result<(), ScratchError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if is_symlink_or_reparse_point(&metadata) {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            validate_reclaim_tree(&entry?.path())?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(ScratchError::InvalidPath(path.to_path_buf()));
        }
    }
    // A hard link is a file name, not a recursive edge. Removing the name
    // under the claimed scratch tombstone only decrements its link count; it
    // does not modify or remove the outside file. Windows does not expose a
    // stable link-count accessor at this crate's MSRV, so quarantine reparse
    // points and let the platform's handle-based remove_dir_all unlink names.
    Ok(())
}

#[cfg(unix)]
fn is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn resource_digest(scope: &str, id: &str) -> Vec<u8> {
    Sha256::digest(format!("{scope}:{id}").as_bytes()).to_vec()
}

fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn directory_owner(path: &Path) -> Result<impl OwnerCheck, ScratchError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(std::fs::metadata(path)
            .map_err(|_| ScratchError::InvalidPath(path.to_path_buf()))?
            .uid())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn scoped_resource_dir(
    root: &Path,
    scope: &str,
    id: &str,
    owner: impl OwnerCheck,
) -> Result<PathBuf, ScratchError> {
    let digest = resource_digest(scope, id);
    let digest_hex = hex_digest(&digest);
    let path = root.join(digest_hex);
    ensure_private_directory(&path, owner)?;
    ensure_owner_record(&path, &digest)?;
    Ok(std::fs::canonicalize(path)?)
}

fn ensure_private_parent(path: &Path, expected_uid: impl OwnerCheck) -> Result<(), ScratchError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| ScratchError::InvalidPath(path.to_path_buf()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    path.canonicalize_parent_and_self()
        .map_err(|_| ScratchError::InvalidPath(path.to_path_buf()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != expected_uid.uid() || metadata.mode() & 0o022 != 0 {
            return Err(ScratchError::WrongOwner(path.to_path_buf()));
        }
    }
    Ok(())
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
trait OwnerCheck: Copy {
    fn uid(self) -> u32;
}

#[cfg(unix)]
impl OwnerCheck for u32 {
    fn uid(self) -> u32 {
        self
    }
}

#[cfg(not(unix))]
trait OwnerCheck: Copy {}

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
    fn delegated_private_and_shared_roots_are_distinct_and_stable() {
        let session_id = format!("scratch-run-session-{}", uuid::Uuid::new_v4());
        let first_run = codegg_core::identity::AgentRunId::new();
        let second_run = codegg_core::identity::AgentRunId::new();
        let shared = codegg_core::identity::ScratchId::new();
        let first = agent_run_scratch_dir(&session_id, &first_run).unwrap();
        let repeated = agent_run_scratch_dir(&session_id, &first_run).unwrap();
        let second = agent_run_scratch_dir(&session_id, &second_run).unwrap();
        let shared_root = shared_scratch_dir(&session_id, &shared).unwrap();
        assert_eq!(first, repeated);
        assert_ne!(first, second);
        assert_ne!(first, shared_root);
        assert_ne!(second, shared_root);
        #[cfg(any(unix, windows))]
        {
            assert!(reclaim_shared_scratch_dir(&session_id, &shared).unwrap());
            assert!(!shared_root.exists());
            assert!(!reclaim_shared_scratch_dir(&session_id, &shared).unwrap());
        }
        #[cfg(not(any(unix, windows)))]
        {
            assert!(matches!(
                reclaim_shared_scratch_dir(&session_id, &shared),
                Err(ScratchError::CleanupUnavailable)
            ));
            assert!(shared_root.is_dir());
            std::fs::remove_dir_all(&shared_root).unwrap();
        }
        std::fs::remove_dir_all(first).unwrap();
        std::fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn terminal_run_cleanup_removes_only_its_private_root() {
        let session_id = format!("scratch-run-cleanup-{}", uuid::Uuid::new_v4());
        let run_id = codegg_core::identity::AgentRunId::new();
        let other_run = codegg_core::identity::AgentRunId::new();
        let run_root = agent_run_scratch_dir(&session_id, &run_id).unwrap();
        let other_root = agent_run_scratch_dir(&session_id, &other_run).unwrap();
        std::fs::write(run_root.join("temporary.txt"), "disposable").unwrap();
        std::fs::write(other_root.join("keep.txt"), "still live").unwrap();

        assert!(reclaim_agent_run_scratch_dir(&session_id, &run_id).unwrap());
        assert!(!run_root.exists());
        assert_eq!(
            std::fs::read_to_string(other_root.join("keep.txt")).unwrap(),
            "still live"
        );
        assert!(!reclaim_agent_run_scratch_dir(&session_id, &run_id).unwrap());

        std::fs::remove_dir_all(other_root).unwrap();
        let session_root = session_scratch_dir(&session_id).unwrap();
        std::fs::remove_dir_all(session_root).unwrap();
    }

    #[test]
    fn deleted_session_cleanup_reclaims_nested_private_and_shared_roots() {
        let session_id = format!("scratch-session-cleanup-{}", uuid::Uuid::new_v4());
        let run_id = codegg_core::identity::AgentRunId::new();
        let scratch_id = codegg_core::identity::ScratchId::new();
        let run_root = agent_run_scratch_dir(&session_id, &run_id).unwrap();
        let shared_root = shared_scratch_dir(&session_id, &scratch_id).unwrap();
        std::fs::write(run_root.join("run.tmp"), "temporary").unwrap();
        std::fs::write(shared_root.join("shared.tmp"), "temporary").unwrap();

        assert!(reclaim_deleted_session_scratch_dir(&session_id).unwrap());
        assert!(!run_root.exists());
        assert!(!shared_root.exists());
        assert!(!reclaim_deleted_session_scratch_dir(&session_id).unwrap());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn revoked_cleanup_recovers_an_orphaned_tombstone_after_restart() {
        let session_id = format!("scratch-orphaned-reclaim-{}", uuid::Uuid::new_v4());
        let scratch_id = codegg_core::identity::ScratchId::new();
        let root = shared_scratch_dir(&session_id, &scratch_id).unwrap();
        std::fs::write(root.join("artifact.txt"), "disposable").unwrap();
        let digest = resource_digest("shared-scratch", scratch_id.as_str());
        let tombstone = root
            .parent()
            .unwrap()
            .join(format!(".reclaim-{}-interrupted", hex_digest(&digest)));
        std::fs::rename(&root, &tombstone).unwrap();

        assert!(reclaim_shared_scratch_dir(&session_id, &scratch_id).unwrap());
        assert!(!tombstone.exists());
        assert!(!root.exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn concurrent_revocation_cleanup_reclaims_one_resource_once() {
        let session_id = format!("scratch-concurrent-reclaim-{}", uuid::Uuid::new_v4());
        let scratch_id = codegg_core::identity::ScratchId::new();
        let root = shared_scratch_dir(&session_id, &scratch_id).unwrap();
        std::fs::write(root.join("artifact.txt"), "disposable").unwrap();
        let workers = (0..2)
            .map(|_| {
                let session_id = session_id.clone();
                let scratch_id = scratch_id.clone();
                std::thread::spawn(move || reclaim_shared_scratch_dir(&session_id, &scratch_id))
            })
            .collect::<Vec<_>>();
        let reclaimed = workers
            .into_iter()
            .map(|worker| worker.join().unwrap().unwrap())
            .filter(|reclaimed| *reclaimed)
            .count();

        assert_eq!(reclaimed, 1);
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn revoked_shared_cleanup_quarantines_symlinks_and_hardlinks() {
        let session_id = format!("scratch-cleanup-session-{}", uuid::Uuid::new_v4());
        let scratch_id = codegg_core::identity::ScratchId::new();
        let external = tempfile::tempdir().unwrap();
        let external_file = external.path().join("external.txt");
        std::fs::write(&external_file, "outside content").unwrap();
        let root = shared_scratch_dir(&session_id, &scratch_id).unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::os::unix::fs::symlink(&external_file, root.join("nested/link")).unwrap();

        assert!(reclaim_shared_scratch_dir(&session_id, &scratch_id).is_err());
        assert_eq!(
            std::fs::read_to_string(&external_file).unwrap(),
            "outside content"
        );

        let shared_parent = root.parent().unwrap();
        let tombstone = std::fs::read_dir(shared_parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".reclaim-"))
            })
            .expect("unsafe resource is quarantined under an opaque tombstone");
        assert!(
            !root.exists(),
            "the managed name is free for a new generation"
        );
        std::fs::remove_dir_all(tombstone).unwrap();

        let hardlink_id = codegg_core::identity::ScratchId::new();
        let hardlink_root = shared_scratch_dir(&session_id, &hardlink_id).unwrap();
        std::fs::hard_link(&external_file, hardlink_root.join("external-link")).unwrap();
        assert!(reclaim_shared_scratch_dir(&session_id, &hardlink_id).is_err());
        assert_eq!(
            std::fs::read_to_string(&external_file).unwrap(),
            "outside content"
        );
        let hardlink_tombstone = std::fs::read_dir(shared_parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".reclaim-") && path.exists())
            })
            .expect("hardlinked resource is quarantined");
        std::fs::remove_dir_all(hardlink_tombstone).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn revoked_shared_cleanup_quarantines_reparse_points_and_unlinks_hardlinks_safely() {
        let session_id = format!("scratch-windows-cleanup-session-{}", uuid::Uuid::new_v4());
        let external = tempfile::tempdir().unwrap();
        let external_file = external.path().join("external.txt");
        let external_dir = external.path().join("external-dir");
        std::fs::create_dir(&external_dir).unwrap();
        let external_nested = external_dir.join("untouched.txt");
        std::fs::write(&external_file, "outside file content").unwrap();
        std::fs::write(&external_nested, "outside directory content").unwrap();

        let reparse_id = codegg_core::identity::ScratchId::new();
        let reparse_root = shared_scratch_dir(&session_id, &reparse_id).unwrap();
        let reparse_path = reparse_root.join("external-reparse-point");
        // Windows CI runs with an elevated disposable token, so the standard
        // library can create a native directory-symlink reparse point without
        // passing paths through cmd.exe's separate command-line parser.
        std::os::windows::fs::symlink_dir(&external_dir, &reparse_path)
            .expect("create native directory reparse point");
        assert!(reclaim_shared_scratch_dir(&session_id, &reparse_id).is_err());
        assert_eq!(
            std::fs::read_to_string(&external_nested).unwrap(),
            "outside directory content"
        );
        let shared_parent = reparse_root.parent().unwrap();
        let tombstone = std::fs::read_dir(shared_parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".reclaim-") && path.exists())
            })
            .expect("unsafe reparse-point tree remains quarantined");
        std::fs::remove_dir_all(tombstone).unwrap();

        let hardlink_id = codegg_core::identity::ScratchId::new();
        let hardlink_root = shared_scratch_dir(&session_id, &hardlink_id).unwrap();
        std::fs::hard_link(&external_file, hardlink_root.join("external-link")).unwrap();
        assert!(reclaim_shared_scratch_dir(&session_id, &hardlink_id).unwrap());
        assert!(!hardlink_root.exists());
        assert_eq!(
            std::fs::read_to_string(&external_file).unwrap(),
            "outside file content"
        );
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

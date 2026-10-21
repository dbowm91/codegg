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
    let expected_uid = current_user_owner()?;
    let cache_root = dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)?;
    std::fs::create_dir_all(&cache_root)?;
    ensure_private_parent(&cache_root, expected_uid)?;
    let vendor_root = cache_root.join(SCRATCH_VENDOR_NAME);
    let scratch_root = vendor_root.join(SCRATCH_ROOT_NAME);
    let sessions_root = scratch_root.join("sessions");
    ensure_private_directory(&vendor_root, expected_uid)?;
    ensure_private_directory(&scratch_root, expected_uid)?;
    ensure_private_directory(&sessions_root, expected_uid)?;

    let digest = session_digest_bytes(session_id);
    let session_root = sessions_root.join(hex_digest(&digest));
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
    reclaim_owned_resource_dir(sessions_root, &session_digest_bytes(session_id), owner)
}

fn existing_session_scratch_dir(session_id: &str) -> Result<Option<PathBuf>, ScratchError> {
    if session_id.is_empty() {
        return Err(ScratchError::MissingSession);
    }
    let name = session_digest(session_id);
    let path = scratch_sessions_root()?.join(name);
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
    if let Some(_refusal) = reclaim_entry_refusal(&metadata) {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            validate_reclaim_tree(&entry?.path())?;
        }
        return Ok(());
    }
    // A hard link is a file name, not a recursive edge. Removing the name
    // under the claimed scratch tombstone only decrements its link count; it
    // does not modify or remove the outside file. Windows does not expose a
    // stable link-count accessor at this crate's MSRV, so quarantine reparse
    // points and let the platform's handle-based remove_dir_all unlink names.
    Ok(())
}

/// The single session-identity digest computation.
///
/// The session tree name is `hex(sha256(session_id))`. This is the only
/// place that hashes a session identity: allocation, reclamation, and the
/// usage census all route through it, so the census can never report a
/// digest the allocator would not produce. The mapping is deliberately
/// one-way — the census reports the digest and never inverts it.
fn session_digest_bytes(session_id: &str) -> Vec<u8> {
    Sha256::digest(session_id.as_bytes()).to_vec()
}

/// The opaque session digest that names a scratch tree.
pub fn session_digest(session_id: &str) -> String {
    hex_digest(&session_digest_bytes(session_id))
}

/// The current user's owner identity, shared by allocation and the census.
///
/// The census reuses this rather than asking the filesystem a second
/// question: a tree the current user does not own is refused, not walked.
#[cfg(unix)]
fn current_user_owner() -> Result<u32, ScratchError> {
    use std::os::unix::fs::MetadataExt;
    let home = dirs::home_dir().ok_or(ScratchError::OwnerUnavailable)?;
    std::fs::metadata(home)
        .map_err(|_| ScratchError::OwnerUnavailable)
        .map(|metadata| metadata.uid())
}

#[cfg(not(unix))]
fn current_user_owner() -> Result<(), ScratchError> {
    Ok(())
}

/// The sessions root, resolved without creating it.
///
/// Allocation creates this tree; the census only reads it. Resolving it
/// through one helper keeps the two paths on the same location without the
/// census acquiring a create side effect.
fn scratch_sessions_root() -> Result<PathBuf, ScratchError> {
    let cache_root = dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)?;
    Ok(cache_root
        .join(SCRATCH_VENDOR_NAME)
        .join(SCRATCH_ROOT_NAME)
        .join("sessions"))
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

#[cfg(not(any(unix, windows)))]
fn is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Why the reclaim validator would refuse an entry.
///
/// This rule is **shared** with [`validate_reclaim_tree`], so the usage
/// census can never report a tree as reclaimable that the reclaim path
/// would refuse. A report that disagrees with the behaviour it describes is
/// worse than no report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimRefusal {
    /// The entry is a symlink, or — on Windows — a reparse point.
    Link,
    /// The entry is neither a regular file nor a directory.
    NotRegular,
    /// The regular file carries more than one hard link (Unix only).
    MultiplyLinked,
    /// The entry's metadata could not be read.
    ///
    /// The validator surfaces this as an I/O error rather than a path
    /// refusal, but the outcome is identical: the tree cannot be reclaimed.
    Unreadable,
}

impl ReclaimRefusal {
    /// Stable wire name for the refusal reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::NotRegular => "not_regular",
            Self::MultiplyLinked => "multiply_linked",
            Self::Unreadable => "unreadable",
        }
    }
}

/// The reclaim-safety verdict for one entry, using the validator's own rules.
///
/// `None` means the entry is safe to reclaim as it stands.
fn reclaim_entry_refusal(metadata: &std::fs::Metadata) -> Option<ReclaimRefusal> {
    if is_symlink_or_reparse_point(metadata) {
        return Some(ReclaimRefusal::Link);
    }
    if metadata.is_dir() {
        return None;
    }
    if !metadata.is_file() {
        return Some(ReclaimRefusal::NotRegular);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Some(ReclaimRefusal::MultiplyLinked);
        }
    }
    None
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

// ── M006: scratch usage census ─────────────────────────────────────────────
//
// The census measures the per-user scratch tree and changes nothing.
//
// It never creates, renames, unlinks, chmods, truncates, or opens a file for
// writing, and it never creates a tombstone. Only `symlink_metadata` is
// used, so a link is reported and never traversed, and a file is measured by
// its metadata and never opened — opening a FIFO would block, and opening a
// device node could have side effects the caller never asked for.
//
// The census is a point-in-time observation of a live tree. A model may be
// writing into its scratch while the walk runs, so a size that moves between
// two reads is normal, not an error.

/// Bounded walk budget for one census refresh.
///
/// Every dimension is bounded because the census walks a live tree that a
/// model may be writing to. A bound that is reached is reported as
/// `truncated`; a partial total is never presented as a complete one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScratchCensusBudget {
    /// Maximum number of filesystem entries visited across the whole walk.
    pub max_entries: usize,
    /// Maximum directory depth below the sessions root.
    pub max_depth: usize,
    /// Maximum number of refusal paths reported per tree.
    pub max_paths_per_tree: usize,
    /// Maximum number of trees reported.
    pub max_trees: usize,
}

impl Default for ScratchCensusBudget {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_depth: 16,
            max_paths_per_tree: 32,
            max_trees: 2_000,
        }
    }
}

/// One tree's measured usage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScratchTreeUsage {
    /// The opaque tree digest — the directory name. Never a session id: the
    /// mapping is one-way by design and the census does not invert it.
    pub digest: String,
    /// Total bytes of the regular files observed.
    pub total_bytes: u64,
    /// Number of regular files observed.
    pub file_count: u64,
    /// Number of directories observed.
    pub dir_count: u64,
    /// Number of symlinks (or Windows reparse points) observed.
    pub link_count: u64,
    /// Number of multiply-linked regular files observed.
    pub multiply_linked_count: u64,
    /// Number of entries that are neither a regular file nor a directory.
    pub non_regular_count: u64,
    /// Number of entries whose metadata could not be read.
    pub unreadable_count: u64,
    /// Number of entries that vanished between listing and inspection.
    pub vanished_entries: u64,
    /// Oldest modification time observed, in epoch milliseconds.
    pub oldest_modified_ms: Option<i64>,
    /// Newest modification time observed, in epoch milliseconds.
    pub newest_modified_ms: Option<i64>,
    /// Whether the reclaim validator would accept this tree as observed.
    ///
    /// `None` when the verdict could not be established — the tree vanished
    /// mid-walk, or the walk was truncated before the tree was fully read.
    /// A partial walk never yields `Some(true)`.
    pub reclaimable: Option<bool>,
    /// The rules that refuse this tree, in discovery order, deduplicated.
    pub refusal_rules: Vec<ReclaimRefusal>,
    /// Bounded relative paths (inside the tree) that triggered a refusal.
    ///
    /// These are the only file names the census reports: they identify the
    /// entry the operator has to remove, and they never leave the tree.
    pub refusal_paths: Vec<String>,
    /// `true` when the tree itself vanished during the walk.
    pub vanished: bool,
    /// `true` when a budget stopped the walk before the tree was fully read.
    pub truncated: bool,
}

/// The measured state of the per-user scratch tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScratchUsageReport {
    /// One entry per managed session tree that was inspected.
    pub trees: Vec<ScratchTreeUsage>,
    /// Number of trees reported.
    pub tree_count: usize,
    /// Total bytes across every reported tree.
    pub total_bytes: u64,
    /// Oldest modification time observed, in epoch milliseconds.
    pub oldest_modified_ms: Option<i64>,
    /// Newest modification time observed, in epoch milliseconds.
    pub newest_modified_ms: Option<i64>,
    /// `true` when a budget stopped the walk.
    pub truncated: bool,
    /// Reclaim tombstones found under the sessions root.
    ///
    /// A tombstone is a tree the reclaim path quarantined because the
    /// validator refused it. Counting them keeps a stuck reclamation
    /// visible instead of silently absent from the report.
    pub quarantined_tree_count: usize,
    /// Entries under the sessions root that are not managed trees.
    pub unrecognized_entry_count: usize,
    /// The budget that was in force for this walk.
    pub budget: ScratchCensusBudget,
    /// When the walk completed, in epoch milliseconds.
    pub generated_at_ms: i64,
    /// Non-fatal problems observed during the walk.
    pub anomalies: Vec<String>,
}

/// Mutable state of one census walk.
struct CensusWalk {
    budget: ScratchCensusBudget,
    entries_visited: usize,
    trees_reported: usize,
    truncated: bool,
}

impl CensusWalk {
    /// Record a refusal against one tree, keeping the path list bounded.
    fn record_refusal(&self, usage: &mut ScratchTreeUsage, rule: ReclaimRefusal, relative: &str) {
        if !usage.refusal_rules.contains(&rule) {
            usage.refusal_rules.push(rule);
        }
        if usage.refusal_paths.len() < self.budget.max_paths_per_tree {
            usage.refusal_paths.push(bound_relative_path(relative));
        }
    }
}

/// Census the per-user scratch tree. Read-only.
///
/// A missing scratch root is an empty report, not an error: a fresh install
/// has nothing to measure.
pub fn census_scratch_usage(budget: ScratchCensusBudget) -> ScratchUsageReport {
    match scratch_sessions_root() {
        Ok(root) => census_scratch_usage_at(&root, budget),
        Err(error) => {
            let mut report = empty_census_report(budget);
            report
                .anomalies
                .push(format!("scratch root is unavailable: {error}"));
            report
        }
    }
}

/// An empty report carrying the budget and the measurement time.
fn empty_census_report(budget: ScratchCensusBudget) -> ScratchUsageReport {
    ScratchUsageReport {
        budget,
        generated_at_ms: chrono::Utc::now().timestamp_millis(),
        ..ScratchUsageReport::default()
    }
}

/// Census the scratch tree under an explicit sessions root.
///
/// The root is a parameter so the census can be measured against a fixture
/// tree rather than the developer's own scratch. The daemon's request path
/// never reaches this with a client-chosen root: the daemon always
/// measures the single scratch tree it owns.
pub fn census_scratch_usage_at(
    sessions_root: &Path,
    budget: ScratchCensusBudget,
) -> ScratchUsageReport {
    let mut report = empty_census_report(budget);

    let metadata = match std::fs::symlink_metadata(sessions_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A fresh install has no scratch tree yet. Nothing to measure.
            return report;
        }
        Err(error) => {
            report
                .anomalies
                .push(format!("scratch root could not be read: {error}"));
            return report;
        }
    };
    if is_symlink_or_reparse_point(&metadata) || !metadata.is_dir() {
        report
            .anomalies
            .push("scratch root is not a real directory".to_string());
        return report;
    }

    // The census measures the tree the daemon owns. A root owned by another
    // user is refused rather than walked.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let expected = match current_user_owner() {
            Ok(owner) => owner,
            Err(error) => {
                report
                    .anomalies
                    .push(format!("current user owner is unavailable: {error}"));
                return report;
            }
        };
        if metadata.uid() != expected {
            report
                .anomalies
                .push("scratch root is owned by a different user".to_string());
            return report;
        }
    }

    let entries = match std::fs::read_dir(sessions_root) {
        Ok(entries) => entries,
        Err(error) => {
            report
                .anomalies
                .push(format!("scratch root could not be listed: {error}"));
            return report;
        }
    };

    let mut managed: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report
                    .anomalies
                    .push(format!("scratch entry could not be read: {error}"));
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".reclaim-") {
            // A quarantined tree: the reclaim path moved it aside because
            // the validator refused it.
            report.quarantined_tree_count += 1;
            continue;
        }
        if !is_session_digest_name(&name) {
            report.unrecognized_entry_count += 1;
            continue;
        }
        managed.push((name, entry.path()));
    }
    // Deterministic order so two runs over the same tree agree.
    managed.sort();

    let mut walk = CensusWalk {
        budget,
        entries_visited: 0,
        trees_reported: 0,
        truncated: false,
    };
    for (digest, path) in managed {
        if walk.trees_reported >= walk.budget.max_trees {
            walk.truncated = true;
            break;
        }
        walk.trees_reported += 1;
        let usage = census_tree(&path, digest, &mut walk);
        report.total_bytes = report.total_bytes.saturating_add(usage.total_bytes);
        if let Some(oldest) = usage.oldest_modified_ms {
            report.oldest_modified_ms = Some(match report.oldest_modified_ms {
                Some(current) => current.min(oldest),
                None => oldest,
            });
        }
        if let Some(newest) = usage.newest_modified_ms {
            report.newest_modified_ms = Some(match report.newest_modified_ms {
                Some(current) => current.max(newest),
                None => newest,
            });
        }
        report.trees.push(usage);
    }

    report.tree_count = report.trees.len();
    report.truncated = walk.truncated;
    report
}

/// Inspect one managed tree and report its usage.
fn census_tree(root: &Path, digest: String, walk: &mut CensusWalk) -> ScratchTreeUsage {
    let mut usage = ScratchTreeUsage {
        digest,
        ..ScratchTreeUsage::default()
    };

    let root_metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A concurrent reclamation removed the tree mid-walk. That is a
            // normal outcome of a read-only walk over a live tree.
            usage.vanished = true;
            return usage;
        }
        Err(_) => {
            usage.unreadable_count += 1;
            walk.record_refusal(&mut usage, ReclaimRefusal::Unreadable, "");
            usage.reclaimable = Some(false);
            return usage;
        }
    };
    if let Some(rule) = reclaim_entry_refusal(&root_metadata) {
        match rule {
            ReclaimRefusal::Link => usage.link_count += 1,
            ReclaimRefusal::NotRegular => usage.non_regular_count += 1,
            ReclaimRefusal::MultiplyLinked => usage.multiply_linked_count += 1,
            ReclaimRefusal::Unreadable => {}
        }
        walk.record_refusal(&mut usage, rule, "");
        usage.reclaimable = Some(false);
        return usage;
    }
    if !root_metadata.is_dir() {
        usage.non_regular_count += 1;
        walk.record_refusal(&mut usage, ReclaimRefusal::NotRegular, "");
        usage.reclaimable = Some(false);
        return usage;
    }

    // Explicit stack: the walk is bounded in depth, so recursion would buy
    // nothing and a hostile tree should not be able to grow the call stack.
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((path, relative, depth)) = stack.pop() {
        if walk.entries_visited >= walk.budget.max_entries {
            walk.truncated = true;
            usage.truncated = true;
            break;
        }
        walk.entries_visited += 1;

        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Removed between the listing and the inspection.
                usage.vanished_entries += 1;
                continue;
            }
            Err(_) => {
                usage.unreadable_count += 1;
                walk.record_refusal(&mut usage, ReclaimRefusal::Unreadable, &relative);
                continue;
            }
        };

        if let Some(rule) = reclaim_entry_refusal(&metadata) {
            match rule {
                ReclaimRefusal::Link => usage.link_count += 1,
                ReclaimRefusal::NotRegular => usage.non_regular_count += 1,
                ReclaimRefusal::MultiplyLinked => usage.multiply_linked_count += 1,
                ReclaimRefusal::Unreadable => {}
            }
            walk.record_refusal(&mut usage, rule, &relative);
            // Never descend into a link or a non-regular entry: the link may
            // point outside the tree, and a FIFO must not be opened.
            continue;
        }

        observe_modification_time(&mut usage, &metadata);
        if metadata.is_dir() {
            usage.dir_count += 1;
            if depth >= walk.budget.max_depth {
                walk.truncated = true;
                usage.truncated = true;
                continue;
            }
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => {
                                let name = entry.file_name().to_string_lossy().into_owned();
                                let child_relative = if relative.is_empty() {
                                    name
                                } else {
                                    format!("{relative}/{name}")
                                };
                                stack.push((entry.path(), child_relative, depth + 1));
                            }
                            Err(_) => {
                                usage.unreadable_count += 1;
                                walk.record_refusal(
                                    &mut usage,
                                    ReclaimRefusal::Unreadable,
                                    &relative,
                                );
                            }
                        }
                    }
                }
                Err(_) => {
                    // A directory without read permission: the validator would
                    // fail on it too, so the tree is not reclaimable.
                    usage.unreadable_count += 1;
                    walk.record_refusal(&mut usage, ReclaimRefusal::Unreadable, &relative);
                }
            }
            continue;
        }

        usage.file_count += 1;
        usage.total_bytes = usage.total_bytes.saturating_add(metadata.len());
    }

    usage.reclaimable = if usage.truncated {
        // A partial walk cannot claim the tree is reclaimable.
        None
    } else {
        Some(usage.refusal_rules.is_empty())
    };
    usage
}

/// Fold one entry's modification time into the tree's age range.
fn observe_modification_time(usage: &mut ScratchTreeUsage, metadata: &std::fs::Metadata) {
    let Ok(modified) = metadata.modified() else {
        return;
    };
    let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return;
    };
    let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
    usage.oldest_modified_ms = Some(match usage.oldest_modified_ms {
        Some(current) => current.min(millis),
        None => millis,
    });
    usage.newest_modified_ms = Some(match usage.newest_modified_ms {
        Some(current) => current.max(millis),
        None => millis,
    });
}

/// `true` when `name` is a managed session tree name: a 64-character
/// lowercase hex digest.
fn is_session_digest_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Bound a relative path's length before it enters a report.
///
/// The path stays relative to its own tree: an absolute host path never
/// enters a report, and the leaf is preserved so the operator can still
/// find the entry the reclaim validator refused.
fn bound_relative_path(relative: &str) -> String {
    const MAX_RELATIVE_PATH_CHARS: usize = 200;
    if relative.chars().count() <= MAX_RELATIVE_PATH_CHARS {
        return relative.to_string();
    }
    let leaf = relative.rsplit('/').next().unwrap_or(relative);
    let parent = relative.rsplit('/').nth(1).unwrap_or("");
    let mut bounded = String::from("…/");
    if !parent.is_empty() {
        bounded.push_str(&truncate_chars(parent, 48));
        bounded.push('/');
    }
    bounded.push_str(&truncate_chars(leaf, 120));
    bounded
}

/// Truncate `text` to at most `max` characters.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
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

    // ── M006: the scratch usage census ──────────────────────────────────

    /// A digest-shaped directory name, so the census treats it as a managed
    /// session tree.
    fn fixture_tree_name() -> String {
        "a".repeat(64)
    }

    /// Build a sessions root holding one managed tree.
    fn fixture_sessions_root(tree_name: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let tree = root.path().join(tree_name);
        std::fs::create_dir_all(&tree).unwrap();
        root
    }

    #[test]
    fn census_reports_counts_sizes_and_a_reclaim_verdict() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        std::fs::write(tree.join("a.txt"), b"hello").unwrap();
        std::fs::write(tree.join("b.txt"), b"worldly").unwrap();
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested/c.txt"), b"1234567890").unwrap();

        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());

        assert_eq!(report.tree_count, 1, "one managed tree: {report:?}");
        assert_eq!(report.trees.len(), 1);
        let usage = &report.trees[0];
        assert_eq!(usage.digest, fixture_tree_name());
        assert_eq!(usage.file_count, 3);
        // The tree root plus `nested`.
        assert_eq!(usage.dir_count, 2);
        assert_eq!(usage.total_bytes, 5 + 7 + 10);
        assert_eq!(report.total_bytes, 22);
        assert_eq!(usage.link_count, 0);
        assert_eq!(usage.multiply_linked_count, 0);
        assert_eq!(usage.non_regular_count, 0);
        assert_eq!(usage.unreadable_count, 0);
        assert!(usage.refusal_rules.is_empty());
        assert_eq!(usage.reclaimable, Some(true));
        assert!(!report.truncated);
        assert!(report.anomalies.is_empty(), "{:?}", report.anomalies);
        assert!(usage.oldest_modified_ms.is_some());
        assert!(usage.newest_modified_ms.is_some());
    }

    #[test]
    fn census_reports_an_empty_tree_as_reclaimable() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());
        assert_eq!(report.tree_count, 1);
        assert_eq!(report.trees[0].file_count, 0);
        assert_eq!(report.trees[0].reclaimable, Some(true));
    }

    #[cfg(unix)]
    #[test]
    fn census_reports_a_multiply_linked_file_as_not_reclaimable() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        std::fs::write(tree.join("linked.txt"), b"content").unwrap();
        // A second name for the same inode: the reclaim validator refuses
        // it, so the census must too.
        std::fs::hard_link(tree.join("linked.txt"), tree.join("alias.txt")).unwrap();

        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());

        let usage = &report.trees[0];
        assert_eq!(usage.multiply_linked_count, 2);
        assert_eq!(
            usage.reclaimable,
            Some(false),
            "a multiply-linked tree is not reclaimable: {usage:?}"
        );
        assert!(usage
            .refusal_rules
            .contains(&ReclaimRefusal::MultiplyLinked));
        assert!(
            usage
                .refusal_paths
                .iter()
                .any(|path| path.contains("linked.txt")),
            "the offending path is reported: {:?}",
            usage.refusal_paths
        );
    }

    #[cfg(unix)]
    #[test]
    fn census_reports_a_symlink_as_not_reclaimable() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), tree.join("escape")).unwrap();

        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());

        let usage = &report.trees[0];
        assert_eq!(usage.link_count, 1);
        assert_eq!(usage.reclaimable, Some(false));
        assert!(usage.refusal_rules.contains(&ReclaimRefusal::Link));
        // The link target is outside the tree and must never be followed or
        // reported.
        assert!(
            !usage
                .refusal_paths
                .iter()
                .any(|path| path.contains("secret.txt")),
            "the link target is never reported: {:?}",
            usage.refusal_paths
        );
        assert_eq!(usage.file_count, 0, "the link is not descended into");
    }

    #[cfg(unix)]
    #[test]
    fn census_reports_a_socket_as_not_reclaimable_without_opening_it() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        // A socket file is a non-regular entry: opening it would connect,
        // and a FIFO would block forever. The census must report it and
        // skip it.
        //
        // The socket is bound at a short path and then moved into the
        // tree, because a Unix socket path is limited to `SUN_LEN` and the
        // tree's 64-character digest name does not leave room for one.
        let socket_dir = tempfile::tempdir().unwrap();
        let socket_path = socket_dir.path().join("s");
        let _socket = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        std::fs::rename(&socket_path, tree.join("sock")).unwrap();

        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());

        let usage = &report.trees[0];
        assert_eq!(usage.non_regular_count, 1);
        assert_eq!(usage.reclaimable, Some(false));
        assert!(usage.refusal_rules.contains(&ReclaimRefusal::NotRegular));
    }

    #[test]
    fn census_reports_a_truncated_walk_when_the_budget_is_exhausted() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        for index in 0..8 {
            std::fs::write(tree.join(format!("file-{index}.txt")), b"x").unwrap();
        }

        let budget = ScratchCensusBudget {
            max_entries: 2,
            ..ScratchCensusBudget::default()
        };
        let report = census_scratch_usage_at(root.path(), budget);

        assert!(report.truncated, "the walk reports its own truncation");
        let usage = &report.trees[0];
        assert!(usage.truncated);
        assert_eq!(
            usage.reclaimable, None,
            "a partial walk never claims a tree is reclaimable"
        );
    }

    #[test]
    fn census_reports_a_vanished_tree_without_failing_the_walk() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let missing = root.path().join("b".repeat(64));
        let mut walk = CensusWalk {
            budget: ScratchCensusBudget::default(),
            entries_visited: 0,
            trees_reported: 0,
            truncated: false,
        };

        let usage = census_tree(&missing, "b".repeat(64), &mut walk);

        assert!(usage.vanished, "a vanished tree is reported as vanished");
        assert_eq!(usage.reclaimable, None);
        assert_eq!(usage.file_count, 0);
        assert!(!usage.truncated);
    }

    #[test]
    fn census_counts_quarantined_tombstones_and_ignores_foreign_entries() {
        let root = fixture_sessions_root(&fixture_tree_name());
        std::fs::create_dir(root.path().join(".reclaim-abc-123")).unwrap();
        std::fs::create_dir(root.path().join("not-a-digest")).unwrap();
        std::fs::write(root.path().join("a-file"), b"x").unwrap();

        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());

        assert_eq!(report.quarantined_tree_count, 1);
        assert_eq!(report.unrecognized_entry_count, 2);
        assert_eq!(report.tree_count, 1, "only digest-shaped names are trees");
    }

    #[test]
    fn census_leaves_the_tree_untouched() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        std::fs::write(tree.join("a.txt"), b"hello").unwrap();
        std::fs::create_dir(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested/b.txt"), b"world").unwrap();

        let before = snapshot_tree(root.path());
        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());
        let after = snapshot_tree(root.path());

        assert_eq!(before, after, "the census mutates nothing");
        assert_eq!(report.tree_count, 1);
    }

    #[test]
    fn census_report_carries_no_absolute_path_no_content_and_no_session_id() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        let secret = "SUPER-SECRET-CONTENT";
        std::fs::write(tree.join("secret.txt"), secret).unwrap();

        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());
        let rendered = format!("{report:?}");

        assert!(
            !rendered.contains(secret),
            "file content never enters the report: {rendered}"
        );
        assert!(
            !rendered.contains(&root.path().to_string_lossy().to_string()),
            "an absolute host path never enters the report: {rendered}"
        );
        assert!(
            !rendered.contains("secret.txt"),
            "a file name only appears when it refused the tree: {rendered}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn census_reports_a_refusing_path_but_no_other_file_names() {
        let root = fixture_sessions_root(&fixture_tree_name());
        let tree = root.path().join(fixture_tree_name());
        std::fs::write(tree.join("ordinary.txt"), b"fine").unwrap();
        std::os::unix::fs::symlink("/tmp", tree.join("offender")).unwrap();

        let report = census_scratch_usage_at(root.path(), ScratchCensusBudget::default());
        let usage = &report.trees[0];

        assert_eq!(usage.refusal_paths.len(), 1);
        assert!(!usage
            .refusal_paths
            .iter()
            .any(|path| path.contains("ordinary.txt")));
    }

    #[test]
    fn the_session_digest_is_the_name_the_allocator_produces() {
        let session_id = format!("scratch-digest-{}", uuid::Uuid::new_v4());
        let allocated = session_scratch_dir(&session_id).unwrap();
        let digest = session_digest(&session_id);

        assert_eq!(allocated.file_name().unwrap().to_string_lossy(), digest);
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        std::fs::remove_dir_all(allocated).unwrap();
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn the_census_refusal_rule_agrees_with_the_reclaim_validator() {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain.txt");
        std::fs::write(&plain, b"x").unwrap();
        let link = root.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&plain, &link).unwrap();

        for path in [&plain, &link] {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            let refusal = reclaim_entry_refusal(&metadata);
            let validator_refused = validate_reclaim_tree(path).is_err();
            assert_eq!(
                refusal.is_some(),
                validator_refused,
                "the shared rule and the validator agree on {}: {refusal:?}",
                path.display()
            );
        }
    }

    /// Snapshot a tree's shape and contents for a before/after comparison.
    fn snapshot_tree(root: &Path) -> Vec<(String, u64, Vec<u8>)> {
        let mut entries = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                if metadata.is_dir() {
                    entries.push((format!("{name}/"), 0, Vec::new()));
                    stack.push(entry.path());
                } else {
                    let content = std::fs::read(entry.path()).unwrap_or_default();
                    entries.push((name, metadata.len(), content));
                }
            }
        }
        entries.sort();
        entries
    }
}

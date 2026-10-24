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
/// The name of the owner record inside a scratch tree.
///
/// The record is the daemon's proof that it owns the tree. It is part of the
/// on-disk contract, so the name is public.
pub const OWNER_RECORD_NAME: &str = ".codegg-owner";

#[derive(Debug, Error)]
pub enum ScratchError {
    #[error("session identity is missing")]
    MissingSession,
    #[error("could not determine the current user's cache directory")]
    CacheUnavailable,
    /// An I/O failure while **allocating** a scratch tree.
    ///
    /// The blanket `From<io::Error>` is deliberately absent: every I/O
    /// failure in this module is mapped **explicitly**, so a reclaim failure
    /// can never be reported as a provisioning failure. The compiler
    /// enforces it — a `?` on an `io::Result` does not compile here.
    #[error("failed to provision managed scratch: {0}")]
    Io(#[source] std::io::Error),
    /// An I/O failure while **reclaiming** a scratch tree.
    ///
    /// A reclaim failure is never reported as a provisioning failure:
    /// nothing was provisioned, and an operator reading the message must be
    /// able to tell which of the two operations failed.
    #[error("failed to reclaim managed scratch: {0}")]
    ReclaimIo(#[source] std::io::Error),
    #[error("managed scratch path is not a real directory: {0}")]
    InvalidPath(PathBuf),
    #[error("managed scratch path is owned by a different user: {0}")]
    WrongOwner(PathBuf),
    #[error("managed scratch tree is not reclaimable ({reason}): {path}")]
    Unreclaimable {
        reason: ReclaimRefusal,
        path: PathBuf,
    },
    #[error("could not determine the current user's home-directory owner")]
    OwnerUnavailable,
    #[error("shared scratch reclamation is unavailable on this platform")]
    CleanupUnavailable,
}

/// Resolve the cache root the daemon's scratch tree lives under.
fn scratch_cache_root() -> Result<PathBuf, ScratchError> {
    #[cfg(test)]
    if let Some(root) = test_scratch_root() {
        return Ok(root);
    }
    dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)
}

/// The scratch root a test redirects the tree to, if any.
///
/// The override is **thread-local**: cargo runs each test on its own thread,
/// so two tests never see each other's root, and the tree a test plants is
/// never the developer's real one. This matters more than it looks: the
/// retention sweep *reclaims* trees, so a test that ran against the real
/// root would delete the developer's own scratch.
#[cfg(test)]
mod test_scratch_root_override {
    use std::path::PathBuf;

    thread_local! {
        static TEST_SCRATCH_ROOT: std::cell::RefCell<Option<PathBuf>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(super) fn root() -> Option<PathBuf> {
        TEST_SCRATCH_ROOT.with(|root| root.borrow().clone())
    }

    pub(super) fn install(root: PathBuf) -> Option<PathBuf> {
        TEST_SCRATCH_ROOT.with(|slot| slot.borrow_mut().replace(root))
    }

    pub(super) fn restore(previous: Option<PathBuf>) {
        TEST_SCRATCH_ROOT.with(|slot| {
            *slot.borrow_mut() = previous;
        });
    }
}

#[cfg(test)]
fn test_scratch_root() -> Option<PathBuf> {
    test_scratch_root_override::root()
}

/// A test-only redirect of the scratch tree.
///
/// Dropping the guard restores the previous root.
#[cfg(test)]
pub(crate) struct TestScratchRoot {
    root: PathBuf,
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl TestScratchRoot {
    /// Point the scratch tree at `root` for the current thread.
    pub(crate) fn install(root: PathBuf) -> Self {
        std::fs::create_dir_all(&root).expect("create the test scratch root");
        // The scratch subsystem refuses a group- or other-writable parent, so
        // the test root must be private whatever the ambient umask is. A bare
        // `create_dir_all` inherits it — under umask `0002` it creates `0775`,
        // and the code under test then rejects its own test root with
        // `WrongOwner`. The product path pins `0700` for the same reason
        // (see `ensure_private_parent`); this is the test-side equivalent.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("make the test scratch root private");
        }
        let previous = test_scratch_root_override::install(root.clone());
        Self { root, previous }
    }

    /// The redirected root.
    ///
    /// A thread that the test spawns does not inherit the redirect — it is
    /// thread-local — so a test that allocates from a worker thread installs
    /// the guard there with this path.
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
impl Drop for TestScratchRoot {
    fn drop(&mut self) {
        let previous = self.previous.take();
        test_scratch_root_override::restore(previous);
    }
}

/// A private scratch root for one test.
///
/// The scratch tests allocate real trees, and the reclaim tests **delete**
/// them. A test that ran against the developer's real cache root would write
/// into it and remove the developer's own scratch, so every test that
/// touches the scratch tree installs one of these first.
///
/// The redirect is thread-local, so concurrent tests never see each other's
/// tree. A thread the test spawns does not inherit it: the guard is
/// installed there with [`TestScratchRoot::path`].
#[cfg(test)]
pub(crate) fn isolated_scratch_root(label: &str) -> TestScratchRoot {
    let base = std::fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|_| std::env::temp_dir().to_path_buf());
    TestScratchRoot::install(base.join(format!(
        "cg-scratch-{label}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    )))
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
    let expected_uid = current_owner_uid()?;
    let cache_root = scratch_cache_root()?;
    ensure_private_cache_root(&cache_root, expected_uid)?;
    let vendor_root = cache_root.join(SCRATCH_VENDOR_NAME);
    let scratch_root = vendor_root.join(SCRATCH_ROOT_NAME);
    let sessions_root = scratch_root.join("sessions");
    ensure_private_directory(&vendor_root, expected_uid)?;
    ensure_private_directory(&scratch_root, expected_uid)?;
    ensure_private_directory(&sessions_root, expected_uid)?;

    let digest = session_scratch_digest(session_id);
    let name = hex_digest(&digest);
    let session_root = sessions_root.join(name);
    ensure_private_directory(&session_root, expected_uid)?;
    ensure_owner_record(&session_root, &digest)?;
    std::fs::canonicalize(session_root).map_err(ScratchError::Io)
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
    let session_root = session_scratch_dir(session_id).map_err(reclaim_failure)?;
    let shared_root = session_root.join("shared");
    let owner = directory_owner(&session_root)?;
    ensure_private_directory(&shared_root, owner).map_err(reclaim_failure)?;
    reclaim_owned_resource_dir(
        &shared_root,
        &resource_digest("shared-scratch", scratch_id.as_str()),
        owner,
        ReclaimStrictness::Automatic,
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
        ReclaimStrictness::Automatic,
    )
}

/// Remove all scratch under one session's tree.
///
/// The **decision** to reclaim belongs to the caller — the deleted marker,
/// the idle age, the lease — and this is the filesystem half of it. Session
/// transcripts and child Git results live in their own stores and are not
/// affected.
#[cfg(any(unix, windows))]
pub(crate) fn reclaim_session_scratch_dir(session_id: &str) -> Result<bool, ScratchError> {
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
    reclaim_owned_resource_dir(
        sessions_root,
        &session_scratch_digest(session_id),
        owner,
        ReclaimStrictness::Automatic,
    )
}

/// Remove one session's scratch tree, addressed by its opaque digest.
///
/// The digest is the only handle a sweep or an operator has: the tree's name
/// is `hex(sha256(session_id))`, and the durable side stores the digest, not
/// the session id — so the mapping only runs forward.
#[cfg(any(unix, windows))]
pub(crate) fn reclaim_session_scratch_by_digest(digest_hex: &str) -> Result<bool, ScratchError> {
    let digest = decode_scratch_digest(digest_hex)?;
    let _guard = SHARED_RECLAIM_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let sessions_root = sessions_root_path()?;
    let owner = directory_owner(&sessions_root)?;
    reclaim_owned_resource_dir(&sessions_root, &digest, owner, ReclaimStrictness::Automatic)
}

/// Dispose of one session's scratch tree, addressed by its opaque digest, on
/// the operator's explicit instruction.
///
/// This is the disposition for a tree the automatic path refuses. It relaxes
/// the **entry-content** refusals only: a planted symlink or a non-regular
/// entry is removed by name. The ownership check, the containment check, and
/// the owner-record check are unchanged, so this can only ever remove a tree
/// the daemon itself created and still owns.
#[cfg(any(unix, windows))]
pub(crate) fn reclaim_session_scratch_for_operator(digest_hex: &str) -> Result<bool, ScratchError> {
    let digest = decode_scratch_digest(digest_hex)?;
    let _guard = SHARED_RECLAIM_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let sessions_root = sessions_root_path()?;
    let owner = directory_owner(&sessions_root)?;
    reclaim_owned_resource_dir(&sessions_root, &digest, owner, ReclaimStrictness::Operator)
}

/// Resolve the sessions root without creating anything.
#[cfg(any(unix, windows))]
fn sessions_root_path() -> Result<PathBuf, ScratchError> {
    let cache_root = scratch_cache_root()?;
    Ok(cache_root
        .join(SCRATCH_VENDOR_NAME)
        .join(SCRATCH_ROOT_NAME)
        .join("sessions"))
}

fn existing_session_scratch_dir(session_id: &str) -> Result<Option<PathBuf>, ScratchError> {
    if session_id.is_empty() {
        return Err(ScratchError::MissingSession);
    }
    let name = hex_digest(&session_scratch_digest(session_id));
    let cache_root = scratch_cache_root()?;
    let path = cache_root
        .join(SCRATCH_VENDOR_NAME)
        .join(SCRATCH_ROOT_NAME)
        .join("sessions")
        .join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => session_scratch_dir(session_id)
            .map(Some)
            .map_err(reclaim_failure),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ScratchError::ReclaimIo(error)),
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
pub(crate) fn reclaim_session_scratch_dir(_session_id: &str) -> Result<bool, ScratchError> {
    Err(ScratchError::CleanupUnavailable)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn reclaim_session_scratch_by_digest(_digest_hex: &str) -> Result<bool, ScratchError> {
    Err(ScratchError::CleanupUnavailable)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn reclaim_session_scratch_for_operator(
    _digest_hex: &str,
) -> Result<bool, ScratchError> {
    Err(ScratchError::CleanupUnavailable)
}

#[cfg(any(unix, windows))]
fn reclaim_owned_resource_dir(
    parent: &Path,
    digest: &[u8],
    owner: impl OwnerCheck,
    strictness: ReclaimStrictness,
) -> Result<bool, ScratchError> {
    let name = hex_digest(digest);
    let path = parent.join(&name);
    let canonical_parent = std::fs::canonicalize(parent).map_err(reclaim_io)?;
    let recovered = recover_reclaim_tombstones(parent, digest, owner, strictness)?;
    let tombstone = parent.join(format!(".reclaim-{name}-{}", uuid::Uuid::new_v4()));
    match std::fs::rename(&path, &tombstone) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(recovered),
        Err(error) => return Err(reclaim_io(error)),
    }
    validate_and_remove_tombstone(&tombstone, &canonical_parent, digest, owner, strictness)?;
    Ok(true)
}

/// Map an I/O failure in the reclaim path onto the reclaim-named variant.
///
/// The reclaim path never reports an I/O failure as a provisioning failure:
/// nothing was provisioned, and an operator reading the message must be able
/// to tell the two apart.
#[cfg(any(unix, windows))]
fn reclaim_io(error: std::io::Error) -> ScratchError {
    ScratchError::ReclaimIo(error)
}

/// Map an error that escapes a reclaim operation onto the reclaim naming.
///
/// A provisioning failure that happens *inside* a reclaim is still a reclaim
/// failure to the caller: the operation that failed is the reclaim, and the
/// operator has to be able to tell that from a failure to allocate.
#[cfg(any(unix, windows))]
fn reclaim_failure(error: ScratchError) -> ScratchError {
    match error {
        ScratchError::Io(error) => ScratchError::ReclaimIo(error),
        other => other,
    }
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

/// Recover the tombstones a previous reclaim pass left behind.
///
/// A tombstone is a **generation** of one resource: its name already carries
/// the digest and a uuid, and it is not the resource's live name. The
/// tombstone is therefore validated **in place** — a pass that ends in a
/// refusal must leave the sessions root's set of tombstone names unchanged,
/// so the next pass reaches the same tree instead of a renamed copy of it.
///
/// The claim that protects a tombstone is its name itself. Two reclaimers
/// that reach the same tombstone both try to remove it; the loser sees the
/// tree gone and skips it, which is a race rather than a failure.
#[cfg(any(unix, windows))]
fn recover_reclaim_tombstones(
    shared_root: &Path,
    digest: &[u8],
    owner: impl OwnerCheck,
    strictness: ReclaimStrictness,
) -> Result<bool, ScratchError> {
    let prefix = format!(".reclaim-{}-", hex_digest(digest));
    let tombstones = std::fs::read_dir(shared_root)
        .map_err(reclaim_io)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect::<Vec<_>>();
    let canonical_shared_root = std::fs::canonicalize(shared_root).map_err(reclaim_io)?;
    let mut recovered = false;
    for tombstone in tombstones {
        match validate_and_remove_tombstone(
            &tombstone,
            &canonical_shared_root,
            digest,
            owner,
            strictness,
        ) {
            Ok(()) => recovered = true,
            // A concurrent reclaimer removed the tombstone first. There is
            // nothing left to reclaim, and the race is not a failure.
            Err(ScratchError::ReclaimIo(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            }
            // A refusal is the validator's verdict, not a failure: the tree
            // stays where it is, and the operator disposition owns it.
            Err(error) => return Err(error),
        }
    }
    Ok(recovered)
}

/// Validate one claimed tree and remove it.
///
/// The four checks that authorise a removal are the containment check, the
/// real-directory check, the ownership check, and the owner-record check.
/// A tree that holds **content** must present its owner record: a record
/// that is missing, malformed, or owned by another user is refused, and
/// nothing is removed.
///
/// The one exception is a tree that holds **no entries at all**: there is
/// nothing left to dispose of, and the removal happens inside the owner-only
/// sessions root. That is the shape a daemon killed mid-`remove_dir_all`
/// leaves behind, and it is what makes the reclaim path resumable.
#[cfg(any(unix, windows))]
fn validate_and_remove_tombstone(
    tombstone: &Path,
    canonical_shared_root: &Path,
    digest: &[u8],
    owner: impl OwnerCheck,
    strictness: ReclaimStrictness,
) -> Result<(), ScratchError> {
    let metadata = std::fs::symlink_metadata(tombstone).map_err(reclaim_io)?;
    if is_symlink_or_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(ScratchError::InvalidPath(tombstone.to_path_buf()));
    }
    let expected_tombstone = canonical_shared_root.join(
        tombstone
            .file_name()
            .ok_or_else(|| ScratchError::InvalidPath(tombstone.to_path_buf()))?,
    );
    if std::fs::canonicalize(tombstone).map_err(reclaim_io)? != expected_tombstone {
        return Err(ScratchError::InvalidPath(tombstone.to_path_buf()));
    }
    let owner_record = tombstone.join(OWNER_RECORD_NAME);
    // The record's metadata is read before the record's path is moved into
    // an error, so the borrow of the path does not outlive the read.
    let owner_metadata = std::fs::symlink_metadata(&owner_record);
    let owner_metadata = match owner_metadata {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The owner record is gone. The tree may still be removed when it
            // holds nothing at all; a tree that holds content cannot be
            // proven owned, so it is refused — by the automatic path and by
            // the operator path alike.
            let empty = tree_is_empty(tombstone, owner)?;
            match reclaim_verdict(OwnerRecordState::Missing, None, u64::from(!empty)) {
                ReclaimVerdict::Acceptable => None,
                ReclaimVerdict::Refused(reason) => {
                    return Err(ScratchError::Unreclaimable {
                        reason,
                        path: tombstone.to_path_buf(),
                    })
                }
                ReclaimVerdict::Invalid => {
                    return Err(ScratchError::InvalidPath(tombstone.to_path_buf()))
                }
            }
        }
        Err(error) => return Err(reclaim_io(error)),
    };
    if let Some(owner_metadata) = owner_metadata {
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
        let record = std::fs::read(&owner_record).map_err(reclaim_io)?;
        let mut expected_record = b"codegg-session-scratch-v1\n".to_vec();
        expected_record.extend_from_slice(hex_digest(digest).as_bytes());
        expected_record.push(b'\n');
        if record != expected_record {
            return Err(ScratchError::InvalidPath(tombstone.to_path_buf()));
        }
    }
    validate_reclaim_tree(tombstone, strictness)?;
    std::fs::remove_dir_all(tombstone).map_err(reclaim_io)?;
    Ok(())
}

/// Whether a tree holds no entries at all.
///
/// This is the one predicate that lets the reclaim path remove a tree whose
/// owner record is missing: a tree with nothing in it is already reclaimed,
/// and removing it removes nothing.
///
/// The predicate is a **single read** of the tree's own entry list. It does
/// not recurse, it does not follow a link, and it does not open a file. It
/// also confirms that the tree is a real directory owned by the sessions
/// root's owner, and that its mode is owner-only. A read that fails is
/// treated as "not empty": the predicate fails closed.
#[cfg(any(unix, windows))]
fn tree_is_empty(tree: &Path, owner: impl OwnerCheck) -> Result<bool, ScratchError> {
    let metadata = std::fs::symlink_metadata(tree).map_err(reclaim_io)?;
    if is_symlink_or_reparse_point(&metadata) || !metadata.is_dir() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != owner.uid() || metadata.mode() & 0o077 != 0 {
            return Ok(false);
        }
    }
    let mut entries = match std::fs::read_dir(tree) {
        Ok(entries) => entries,
        // A directory whose entry list cannot be read is **not** empty: the
        // predicate fails closed, and nothing is removed.
        Err(_) => return Ok(false),
    };
    match entries.next() {
        None => Ok(true),
        Some(Ok(_)) => Ok(false),
        // An entry that could not be read is still an entry. The predicate
        // fails closed.
        Some(Err(_)) => Ok(false),
    }
}

#[cfg(any(unix, windows))]
fn validate_reclaim_tree(path: &Path, strictness: ReclaimStrictness) -> Result<(), ScratchError> {
    let metadata = std::fs::symlink_metadata(path).map_err(reclaim_io)?;
    if let Some(reason) = classify_reclaim_entry(&metadata) {
        if strictness == ReclaimStrictness::Automatic {
            return Err(ScratchError::Unreclaimable {
                reason,
                path: path.to_path_buf(),
            });
        }
        // The operator path removes the entry by name. The link's target is
        // not touched: `remove_dir_all` does not follow symbolic links.
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).map_err(reclaim_io)? {
            validate_reclaim_tree(&entry.map_err(reclaim_io)?.path(), strictness)?;
        }
    }
    Ok(())
}

/// How a tree's owner record stands.
///
/// The census and the reclaim validator both ask this question, so a report
/// cannot claim a tree is reclaimable when the validator would refuse it.
#[cfg(any(unix, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerRecordState {
    /// The record is present and is a regular file.
    Present,
    /// The record is absent.
    Missing,
    /// The record is present but is not a usable owner record: it is a
    /// symbolic link, or it is not a regular file.
    Unusable,
}

/// Read a tree's owner record state.
///
/// The census never opens a file for reading, so it cannot inspect the
/// record's *content*; it can only establish whether the record is there
/// and what kind of entry it is.
#[cfg(any(unix, windows))]
fn owner_record_state(tree: &Path) -> OwnerRecordState {
    match std::fs::symlink_metadata(tree.join(OWNER_RECORD_NAME)) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                OwnerRecordState::Unusable
            } else {
                OwnerRecordState::Present
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => OwnerRecordState::Missing,
        // A record that cannot be read is not a usable record.
        Err(_) => OwnerRecordState::Unusable,
    }
}

/// The reclaim validator's verdict for one tree.
#[cfg(any(unix, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReclaimVerdict {
    /// The validator would remove the tree.
    Acceptable,
    /// The validator would refuse the tree, for this reason.
    Refused(ReclaimRefusal),
    /// The validator would refuse the tree with a typed error that is not a
    /// refusal rule — an owner record that is present but unusable.
    Invalid,
}

/// The reclaim validator's verdict, derived from a tree's owner record and
/// its entries.
///
/// The validator and the census both derive their answer from this, so the
/// report cannot claim a tree is reclaimable when the validator would
/// refuse it.
///
/// `entry_count` is the number of entries the tree holds. A tree whose
/// owner record is missing may only be removed when that count is zero:
/// there is nothing left to dispose of. A tree that holds content cannot be
/// proven owned, so it is refused.
#[cfg(any(unix, windows))]
fn reclaim_verdict(
    owner_record: OwnerRecordState,
    entry_refusal: Option<ReclaimRefusal>,
    entry_count: u64,
) -> ReclaimVerdict {
    match owner_record {
        // The record is missing. An empty tree is already reclaimed; a tree
        // that holds content cannot be proven owned.
        OwnerRecordState::Missing => {
            if entry_count == 0 {
                match entry_refusal {
                    Some(reason) => ReclaimVerdict::Refused(reason),
                    None => ReclaimVerdict::Acceptable,
                }
            } else {
                ReclaimVerdict::Refused(ReclaimRefusal::OwnershipUnverifiable)
            }
        }
        // The record is present: the tree's contents decide.
        OwnerRecordState::Present => match entry_refusal {
            Some(reason) => ReclaimVerdict::Refused(reason),
            None => ReclaimVerdict::Acceptable,
        },
        // The record is present but unusable. The validator refuses it with a
        // typed error rather than a refusal rule, so the census reports the
        // tree as not reclaimable and names no refusal.
        OwnerRecordState::Unusable => ReclaimVerdict::Invalid,
    }
}

/// Why the automatic reclaim path refuses a tree.
///
/// The census reports the validator's own verdict, so a report cannot claim
/// a tree is reclaimable when the validator would refuse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimRefusal {
    /// A symbolic link, or a Windows reparse point.
    Symlink,
    /// An entry that is neither a regular file nor a directory.
    NonRegular,
    /// The tree's owner record is missing and the tree holds content, so the
    /// daemon cannot prove that it owns the tree.
    ///
    /// A tree that holds **no** entries at all is already reclaimed: there is
    /// nothing left to dispose of, and the removal happens inside the
    /// owner-only sessions root. A tree that holds content has no such
    /// proof, so it is refused and nothing is removed.
    OwnershipUnverifiable,
}

impl ReclaimRefusal {
    /// A stable, bounded label for the operator report.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Symlink => "symbolic_link",
            Self::NonRegular => "non_regular_entry",
            Self::OwnershipUnverifiable => "ownership_unverifiable",
        }
    }
}

impl std::fmt::Display for ReclaimRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// How strict the reclaim path is about a tree's **entries**.
///
/// The ownership check, the containment check, and the owner-record check are
/// the same under both settings. Only the entry-content rule differs.
#[cfg(any(unix, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReclaimStrictness {
    /// Refuse a tree whose entries include a symlink or a non-regular file.
    ///
    /// The automatic path. A refused tree stays parked under its tombstone,
    /// which keeps the residual visible instead of silently deleting bytes the
    /// daemon cannot vouch for.
    Automatic,
    /// Remove such entries by name.
    ///
    /// The operator path, used only on an explicit instruction to dispose of
    /// a named tree. `std::fs::remove_dir_all` does not follow symbolic
    /// links on either platform, so removing a link removes the name and
    /// leaves whatever it points at untouched.
    Operator,
}

/// The one rule that decides whether a single entry may be removed.
///
/// [`validate_reclaim_tree`] and the census both consult this, so the report
/// cannot drift from the behaviour it describes.
///
/// A hard link is deliberately **not** a refusal. A hard link is a file name,
/// not a recursive edge: removing the name under the claimed scratch
/// tombstone only decrements its link count, and it does not modify or
/// remove the file that the name points at. Windows already behaved this way,
/// because it exposes no stable link-count accessor at this crate's MSRV;
/// the Unix refusal was residue of a fix applied to one platform only.
#[cfg(any(unix, windows))]
fn classify_reclaim_entry(metadata: &std::fs::Metadata) -> Option<ReclaimRefusal> {
    if is_symlink_or_reparse_point(metadata) {
        return Some(ReclaimRefusal::Symlink);
    }
    if !metadata.is_dir() && !metadata.is_file() {
        return Some(ReclaimRefusal::NonRegular);
    }
    None
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

/// The one digest computation for a session's scratch tree.
///
/// The tree name is `hex(sha256(session_id))`. The name is one-way by
/// design — the owner record stores the digest, not the session id — so the
/// durable-to-disk mapping is only computable in the forward direction:
/// enumerate durable session ids and hash each one. Every caller hashes
/// here; a second, divergent copy is how that mapping silently breaks.
pub fn session_scratch_digest(session_id: &str) -> Vec<u8> {
    Sha256::digest(session_id.as_bytes()).to_vec()
}

/// The hex form of [`session_scratch_digest`] — the scratch tree's name.
pub fn session_scratch_digest_hex(session_id: &str) -> String {
    hex_digest(&session_scratch_digest(session_id))
}

/// Decode a tree's hex digest back into its bytes.
///
/// The digest is one-way by design, so this does **not** recover a session
/// id; it only turns a tree's own name into the bytes the owner record and
/// the tombstone name are built from.
pub fn decode_scratch_digest(digest_hex: &str) -> Result<Vec<u8>, ScratchError> {
    if digest_hex.len() != 64 || !digest_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ScratchError::InvalidPath(PathBuf::from(digest_hex)));
    }
    let mut bytes = Vec::with_capacity(32);
    for pair in digest_hex.as_bytes().chunks(2) {
        let text = std::str::from_utf8(pair)
            .map_err(|_| ScratchError::InvalidPath(PathBuf::from(digest_hex)))?;
        bytes.push(
            u8::from_str_radix(text, 16)
                .map_err(|_| ScratchError::InvalidPath(PathBuf::from(digest_hex)))?,
        );
    }
    Ok(bytes)
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
    std::fs::canonicalize(path).map_err(ScratchError::Io)
}

/// The uid every directory the scratch allocator owns must carry.
///
/// The home directory's owner is the authority: a scratch tree that another
/// user owns is never usable, so the allocator refuses it instead of guessing.
#[cfg(unix)]
fn current_owner_uid() -> Result<u32, ScratchError> {
    use std::os::unix::fs::MetadataExt;
    let home = dirs::home_dir().ok_or(ScratchError::OwnerUnavailable)?;
    std::fs::metadata(home)
        .map_err(|_| ScratchError::OwnerUnavailable)
        .map(|metadata| metadata.uid())
}

#[cfg(not(unix))]
fn current_owner_uid() -> Result<(), ScratchError> {
    Ok(())
}

/// Resolve the per-user cache root the scratch allocator depends on.
///
/// A cache root the product creates is created *private*. The process umask
/// must never be able to produce a group- or world-writable root that the
/// ownership check would then refuse: the allocator would otherwise reject
/// the very directory it had just created, and whether the product could run
/// at all would depend on the operator's umask instead of on the product. A
/// cache root that already exists is only validated — the product never
/// re-permissions a directory it does not own, so a cache root another user
/// can write to is refused rather than silently tightened.
fn ensure_private_cache_root(
    path: &Path,
    expected_uid: impl OwnerCheck,
) -> Result<(), ScratchError> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        // A pre-existing path — including a dangling symlink — is validated
        // below, which produces the typed refusal instead of a raw I/O error.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(ScratchError::Io(error)),
    }
    ensure_private_parent(path, expected_uid)
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
            file.write_all(&record).map_err(ScratchError::Io)?;
            file.sync_all().map_err(ScratchError::Io)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let mut matches = false;
            let mut observed = Vec::new();
            for attempt in 0..20 {
                let metadata = std::fs::symlink_metadata(&record_path).map_err(ScratchError::Io)?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(ScratchError::InvalidPath(record_path));
                }
                observed.clear();
                std::fs::File::open(&record_path)
                    .map_err(ScratchError::Io)?
                    .read_to_end(&mut observed)
                    .map_err(ScratchError::Io)?;
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
                std::fs::remove_file(&record_path).map_err(ScratchError::Io)?;
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&record_path).map_err(ScratchError::Io)?;
                file.write_all(&record).map_err(ScratchError::Io)?;
                file.sync_all().map_err(ScratchError::Io)?;
            }
        }
        Err(error) => return Err(ScratchError::Io(error)),
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
        Err(error) => return Err(ScratchError::Io(error)),
    }
    let metadata = std::fs::symlink_metadata(path).map_err(ScratchError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    let canonical = std::fs::canonicalize(path).map_err(ScratchError::Io)?;
    if canonical
        != path
            .canonicalize_parent_and_self()
            .map_err(ScratchError::Io)?
    {
        return Err(ScratchError::InvalidPath(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != expected_uid.uid() {
            return Err(ScratchError::WrongOwner(path.to_path_buf()));
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(ScratchError::Io)?;
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

// ---------------------------------------------------------------------------
// Measurement: the read-only scratch census (M006).
//
// The census is a measurement, never a lifecycle event. It creates nothing,
// renames nothing, unlinks nothing, opens no file for reading, and never
// follows a link: only `symlink_metadata` and directory listing are used.
// ---------------------------------------------------------------------------

/// Bounds for one census walk.
///
/// The census is a measurement, not a traversal contract: it stops at a
/// bound and says so, rather than presenting a partial total as complete.
#[derive(Debug, Clone, Copy)]
pub struct ScratchCensusBudget {
    /// The maximum number of trees the walk reports.
    pub max_trees: usize,
    /// The maximum number of entries visited inside one tree.
    pub max_entries_per_tree: usize,
    /// The maximum directory depth the walk descends into.
    pub max_depth: usize,
}

impl Default for ScratchCensusBudget {
    fn default() -> Self {
        Self {
            max_trees: 512,
            max_entries_per_tree: 4096,
            max_depth: 32,
        }
    }
}

/// One tree's measured usage.
///
/// Counts and sizes only: no path, no file name, no content, and no session
/// identity — the digest is one-way by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchTreeUsage {
    /// The opaque, one-way digest that names the tree.
    pub digest: String,
    /// True when the tree is parked under a reclaim tombstone, which means
    /// the automatic path already refused it at least once.
    pub quarantined: bool,
    /// The total size of the regular files in the tree.
    pub total_bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
    /// Symbolic links (or Windows reparse points). Never traversed.
    pub symlink_count: u64,
    /// Entries that are neither a regular file nor a directory.
    pub non_regular_count: u64,
    /// Regular files with more than one link. Always zero on Windows, which
    /// exposes no stable link-count accessor at this crate's MSRV.
    pub multiply_linked_count: u64,
    /// The oldest modification time observed, in milliseconds since the epoch.
    pub oldest_modified_ms: Option<i64>,
    /// The newest modification time observed, in milliseconds since the epoch.
    pub newest_modified_ms: Option<i64>,
    /// The reclaim validator's own verdict for this tree.
    ///
    /// `None` when the verdict could not be established: the walk was
    /// truncated before the tree was fully read, or the tree vanished while
    /// it was being read. A partial walk never yields `Some(true)` — a tree
    /// the census only half-read is not a tree the validator has accepted.
    pub reclaimable: Option<bool>,
    /// Which rule refused the tree, when it did.
    pub refusal: Option<ReclaimRefusal>,
    /// True when the daemon's owner record is present and is a regular file.
    pub owner_record_present: bool,
    /// True when the walk hit a bound inside this tree.
    pub truncated: bool,
    /// True when the tree vanished while the walk was reading it.
    pub vanished: bool,
}

/// A whole-root measurement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScratchUsageReport {
    /// One entry per tree the walk reached.
    pub trees: Vec<ScratchTreeUsage>,
    /// The number of trees the walk reached.
    pub tree_count: usize,
    /// The number of trees the walk did not reach because of `max_trees`.
    pub unreported_trees: usize,
    /// The total size of every tree the walk reached.
    pub total_bytes: u64,
    /// True when any bound was hit. A truncated report is never presented as
    /// a complete measurement.
    pub truncated: bool,
}

/// Measure the scratch tree the daemon owns.
///
/// Read-only by construction. A missing scratch root is an empty report, not
/// an error: a fresh install has nothing to report.
#[cfg(any(unix, windows))]
pub fn scratch_usage_census(
    budget: &ScratchCensusBudget,
) -> Result<ScratchUsageReport, ScratchError> {
    let Some(sessions_root) = existing_sessions_root()? else {
        return Ok(ScratchUsageReport::default());
    };
    Ok(census_sessions_root(&sessions_root, budget))
}

/// The census is unavailable on this platform.
///
/// The reclaim path is Unix- and Windows-only, so a verdict this platform
/// cannot compute is not reported as "reclaimable" — it is reported as
/// unavailable, which is the fail-closed answer.
#[cfg(not(any(unix, windows)))]
pub fn scratch_usage_census(
    _budget: &ScratchCensusBudget,
) -> Result<ScratchUsageReport, ScratchError> {
    Err(ScratchError::CleanupUnavailable)
}

/// Resolve the sessions root without creating anything.
///
/// The census must never materialize a directory it did not find, so this
/// resolves the root in place and refuses a symlinked component.
#[cfg(any(unix, windows))]
fn existing_sessions_root() -> Result<Option<PathBuf>, ScratchError> {
    let cache_root = scratch_cache_root()?;
    let path = cache_root
        .join(SCRATCH_VENDOR_NAME)
        .join(SCRATCH_ROOT_NAME)
        .join("sessions");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Ok(None);
            }
            Ok(Some(
                path.canonicalize_parent_and_self()
                    .map_err(ScratchError::Io)?,
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ScratchError::Io(error)),
    }
}

/// A tree's name, resolved to the digest it carries.
#[cfg(any(unix, windows))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeName {
    digest: String,
    quarantined: bool,
}

/// Parse a directory name under the sessions root.
///
/// A managed tree is named by its digest. A tree the reclaim path refused is
/// parked under a tombstone name that still carries the digest, so the
/// residual bytes stay visible to the operator.
#[cfg(any(unix, windows))]
fn parse_tree_name(name: &str) -> Option<TreeName> {
    if let Some(rest) = name.strip_prefix(".reclaim-") {
        let digest = rest.split('-').next()?;
        if is_digest_hex(digest) {
            return Some(TreeName {
                digest: digest.to_string(),
                quarantined: true,
            });
        }
        return None;
    }
    if is_digest_hex(name) {
        return Some(TreeName {
            digest: name.to_string(),
            quarantined: false,
        });
    }
    None
}

/// True when the value is exactly one lowercase hex sha256 digest.
#[cfg(any(unix, windows))]
fn is_digest_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Walk one sessions root and measure every tree under it.
#[cfg(any(unix, windows))]
fn census_sessions_root(sessions_root: &Path, budget: &ScratchCensusBudget) -> ScratchUsageReport {
    let mut report = ScratchUsageReport::default();
    for tree in enumerate_trees(sessions_root, true) {
        if report.trees.len() >= budget.max_trees {
            report.unreported_trees += 1;
            report.truncated = true;
            continue;
        }
        let usage = census_tree(&tree, budget);
        report.total_bytes = report.total_bytes.saturating_add(usage.total_bytes);
        report.truncated |= usage.truncated;
        report.trees.push(usage);
    }
    report.tree_count = report.trees.len();
    report
}

/// Measure one tree.
#[cfg(any(unix, windows))]
fn census_tree(tree: &ScratchTreeRef, budget: &ScratchCensusBudget) -> ScratchTreeUsage {
    let path = &tree.path;
    let mut usage = ScratchTreeUsage {
        digest: tree.digest.clone(),
        quarantined: tree.quarantined,
        total_bytes: 0,
        file_count: 0,
        directory_count: 0,
        symlink_count: 0,
        non_regular_count: 0,
        multiply_linked_count: 0,
        oldest_modified_ms: None,
        newest_modified_ms: None,
        reclaimable: None,
        refusal: None,
        owner_record_present: false,
        truncated: false,
        vanished: false,
    };

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            usage.vanished = true;
            return usage;
        }
    };
    if let Some(reason) = classify_reclaim_entry(&metadata) {
        usage.refusal = Some(reason);
        usage.reclaimable = Some(false);
        return usage;
    }
    if !metadata.is_dir() {
        usage.vanished = true;
        return usage;
    }
    note_modified(&mut usage, &metadata);
    let owner_record = owner_record_state(path);
    usage.owner_record_present = owner_record == OwnerRecordState::Present;

    let mut stack: Vec<(PathBuf, usize)> = vec![(path.to_path_buf(), 0)];
    let mut visited = 0usize;
    while let Some((directory, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                // A directory the daemon cannot read (no permission, or a
                // hostile tree) degrades to an incomplete measurement of this
                // tree rather than to a hang or a crash. The report says so
                // instead of presenting a partial count as complete.
                usage.truncated = true;
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let entry_path = entry.path();
            let Ok(entry_metadata) = std::fs::symlink_metadata(&entry_path) else {
                continue;
            };
            visited += 1;
            if visited > budget.max_entries_per_tree {
                usage.truncated = true;
                break;
            }
            note_modified(&mut usage, &entry_metadata);
            // The verdict comes from the validator's own rule, so the report
            // cannot claim a tree is reclaimable when the validator would
            // refuse it.
            if let Some(reason) = classify_reclaim_entry(&entry_metadata) {
                usage.refusal.get_or_insert(reason);
            }
            // What is in the tree, counted independently of the verdict.
            if is_symlink_or_reparse_point(&entry_metadata) {
                // A link is reported, never traversed.
                usage.symlink_count += 1;
                continue;
            }
            if entry_metadata.is_dir() {
                usage.directory_count += 1;
                if depth < budget.max_depth {
                    stack.push((entry_path, depth + 1));
                } else {
                    usage.truncated = true;
                }
                continue;
            }
            if entry_metadata.is_file() {
                usage.file_count += 1;
                usage.total_bytes = usage.total_bytes.saturating_add(entry_metadata.len());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if entry_metadata.nlink() > 1 {
                        usage.multiply_linked_count += 1;
                    }
                }
                continue;
            }
            usage.non_regular_count += 1;
        }
    }
    // The verdict is the validator's own verdict — but only over the part of
    // the tree the walk actually read. A truncated walk leaves the verdict
    // unknown: an unread entry could be one the validator would refuse, so
    // claiming reclaimability here would be a claim the census cannot make.
    //
    // The rule itself is the validator's: a tree whose owner record is
    // missing may only be removed when it holds nothing, so a tree that
    // holds content is refused with `ownership_unverifiable`.
    let entry_count = usage
        .file_count
        .saturating_add(usage.directory_count)
        .saturating_add(usage.symlink_count)
        .saturating_add(usage.non_regular_count);
    let verdict = reclaim_verdict(owner_record, usage.refusal, entry_count);
    if let ReclaimVerdict::Refused(reason) = verdict {
        usage.refusal = Some(reason);
    }
    usage.reclaimable = if usage.truncated {
        None
    } else {
        Some(matches!(verdict, ReclaimVerdict::Acceptable))
    };
    usage
}

/// Fold one entry's modification time into the tree's age range.
#[cfg(any(unix, windows))]
fn note_modified(usage: &mut ScratchTreeUsage, metadata: &std::fs::Metadata) {
    let Ok(modified) = metadata.modified() else {
        return;
    };
    let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return;
    };
    let millis = duration.as_millis().min(i64::MAX as u128) as i64;
    usage.oldest_modified_ms = Some(match usage.oldest_modified_ms {
        Some(existing) => existing.min(millis),
        None => millis,
    });
    usage.newest_modified_ms = Some(match usage.newest_modified_ms {
        Some(existing) => existing.max(millis),
        None => millis,
    });
}

/// A scratch tree the daemon owns, addressed by its opaque digest.
///
/// The digest is the only handle: the path is resolved from it, never taken
/// from a caller.
#[cfg(any(unix, windows))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchTreeRef {
    pub digest: String,
    pub path: PathBuf,
    pub quarantined: bool,
    /// The tree root's own modification time, in milliseconds since the
    /// epoch.
    ///
    /// This is one `stat`, not a traversal: the root's mtime is when the tree
    /// was created, because creating an entry inside it updates the *parent's*
    /// mtime, not its own. The retention sweep uses it as the orphan grace
    /// period's reference point.
    pub modified_ms: Option<i64>,
}

/// Enumerate the scratch trees under one sessions root.
///
/// `include_quarantined` also yields the tombstones left behind by a refused
/// reclaim, so the residual bytes stay visible. The walk is not truncated
/// here: the caller applies its own budget and reports what it did not
/// reach.
#[cfg(any(unix, windows))]
fn enumerate_trees(sessions_root: &Path, include_quarantined: bool) -> Vec<ScratchTreeRef> {
    let Ok(entries) = std::fs::read_dir(sessions_root) else {
        return Vec::new();
    };
    let mut trees = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let parsed = parse_tree_name(&name)?;
            if parsed.quarantined && !include_quarantined {
                return None;
            }
            let path = entry.path();
            let modified_ms = std::fs::symlink_metadata(&path)
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64);
            Some(ScratchTreeRef {
                digest: parsed.digest,
                path,
                quarantined: parsed.quarantined,
                modified_ms,
            })
        })
        .collect::<Vec<_>>();
    // A deterministic order keeps two runs over the same tree comparable.
    trees.sort_by(|left, right| left.digest.cmp(&right.digest));
    trees
}

/// Enumerate the scratch trees the daemon owns.
///
/// The digest is the only handle: the path is resolved from it, never taken
/// from a caller.
#[cfg(any(unix, windows))]
pub fn enumerate_session_scratch_trees(
    include_quarantined: bool,
    max_trees: usize,
) -> Result<Vec<ScratchTreeRef>, ScratchError> {
    let Some(sessions_root) = existing_sessions_root()? else {
        return Ok(Vec::new());
    };
    let mut trees = enumerate_trees(&sessions_root, include_quarantined);
    trees.truncate(max_trees);
    Ok(trees)
}

/// Find the scratch tree one digest names, if it exists.
///
/// The live tree is preferred; a tree the reclaim path refused is parked
/// under a tombstone that still carries the digest, so the residual stays
/// findable by the same handle.
#[cfg(any(unix, windows))]
pub fn find_session_scratch_tree(digest_hex: &str) -> Result<Option<ScratchTreeRef>, ScratchError> {
    let Some(sessions_root) = existing_sessions_root()? else {
        return Ok(None);
    };
    let managed = sessions_root.join(digest_hex);
    if let Some(tree) = tree_ref(&managed, digest_hex, false) {
        return Ok(Some(tree));
    }
    let prefix = format!(".reclaim-{digest_hex}-");
    let entries = match std::fs::read_dir(&sessions_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ScratchError::Io(error)),
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) {
            continue;
        }
        if let Some(tree) = tree_ref(&entry.path(), digest_hex, true) {
            return Ok(Some(tree));
        }
    }
    Ok(None)
}

/// Build a tree reference for a path that is a real directory.
#[cfg(any(unix, windows))]
fn tree_ref(path: &Path, digest_hex: &str, quarantined: bool) -> Option<ScratchTreeRef> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64);
    Some(ScratchTreeRef {
        digest: digest_hex.to_string(),
        path: path.to_path_buf(),
        quarantined,
        modified_ms,
    })
}

/// Measure one scratch tree, addressed by its opaque digest.
///
/// Read-only: it never renames, unlinks, or writes. A digest that names no
/// tree yields an absent measurement rather than an error.
#[cfg(any(unix, windows))]
pub fn preview_session_scratch(digest_hex: &str) -> Result<ScratchTreeUsage, ScratchError> {
    match find_session_scratch_tree(digest_hex)? {
        Some(tree) => Ok(census_tree(&tree, &ScratchCensusBudget::default())),
        None => Ok(ScratchTreeUsage {
            digest: digest_hex.to_string(),
            quarantined: false,
            total_bytes: 0,
            file_count: 0,
            directory_count: 0,
            symlink_count: 0,
            non_regular_count: 0,
            multiply_linked_count: 0,
            oldest_modified_ms: None,
            newest_modified_ms: None,
            reclaimable: None,
            refusal: None,
            owner_record_present: false,
            truncated: false,
            vanished: true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch tree in a temporary sessions root, named by its digest.
    ///
    /// The tree carries its owner record, so it is a tree the reclaim
    /// validator would accept. A tree with **no** record is a different
    /// shape, and it has its own verdict.
    #[cfg(any(unix, windows))]
    fn census_tree_fixture(root: &Path, session_id: &str) -> PathBuf {
        let digest = session_scratch_digest(session_id);
        let tree = root.join(hex_digest(&digest));
        std::fs::create_dir_all(&tree).unwrap();
        let mut record = b"codegg-session-scratch-v1\n".to_vec();
        record.extend_from_slice(hex_digest(&digest).as_bytes());
        record.push(b'\n');
        std::fs::write(tree.join(OWNER_RECORD_NAME), &record).unwrap();
        tree
    }

    // ── The record-less tree (M008) ───────────────────────────────────
    //
    // A daemon killed between `create_new` and the owner record reaching
    // disk leaves a tree with no record. A daemon killed mid-`remove_dir_all`
    // leaves a tree with no record and no entries. The two are different
    // shapes, and the reclaim path must treat them differently: the first is
    // already reclaimed, the second cannot be proven owned.

    /// The sessions root the allocator owns, under the current redirect.
    ///
    /// The root is created through the product's own path, so it carries the
    /// mode and the owner the reclaim validator checks.
    #[cfg(any(unix, windows))]
    fn sessions_root() -> PathBuf {
        let session_id = format!("scratch-sessions-root-{}", uuid::Uuid::new_v4());
        let root = session_scratch_dir(&session_id).unwrap();
        root.parent()
            .expect("the sessions root has a parent")
            .to_path_buf()
    }

    /// Plant a tree in the sessions root, with no owner record.
    ///
    /// The tree is created directly, because the allocator would never
    /// produce these shapes: it writes the record itself.
    #[cfg(any(unix, windows))]
    fn plant_record_less_tree(digest_hex: &str) -> PathBuf {
        let tree = sessions_root().join(digest_hex);
        std::fs::create_dir_all(&tree).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tree, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        tree
    }

    /// The tombstone names the sessions root holds for one digest.
    #[cfg(any(unix, windows))]
    fn tombstone_names(digest_hex: &str) -> Vec<String> {
        let prefix = format!(".reclaim-{digest_hex}-");
        let Ok(entries) = std::fs::read_dir(sessions_root()) else {
            return Vec::new();
        };
        let mut names = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(&prefix))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    /// The digest of a session id, as the tree's own name.
    #[cfg(any(unix, windows))]
    fn digest_of(session_id: &str) -> String {
        hex_digest(&session_scratch_digest(session_id))
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn an_empty_tree_with_no_owner_record_is_reclaimed() {
        let _root = isolated_scratch_root("empty-record-less");
        let digest_hex = digest_of("empty-record-less");
        let tree = plant_record_less_tree(&digest_hex);
        assert!(!tree.join(OWNER_RECORD_NAME).exists());

        let reclaimed = reclaim_session_scratch_by_digest(&digest_hex).unwrap();

        assert!(
            reclaimed,
            "an empty tree with no owner record is already reclaimed"
        );
        assert!(
            tombstone_names(&digest_hex).is_empty(),
            "the reclaimed tree must leave no tombstone behind"
        );
        assert!(!tree.exists(), "the tree must be gone: {tree:?}");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_content_bearing_tree_with_no_owner_record_is_refused() {
        let _root = isolated_scratch_root("content-record-less");
        let digest_hex = digest_of("content-record-less");
        let tree = plant_record_less_tree(&digest_hex);
        std::fs::write(tree.join("artifact.txt"), "disposable").unwrap();

        let error = reclaim_session_scratch_by_digest(&digest_hex).unwrap_err();

        assert!(
            matches!(
                &error,
                ScratchError::Unreclaimable { reason, .. }
                    if *reason == ReclaimRefusal::OwnershipUnverifiable
            ),
            "a content-bearing tree with no owner record must be refused as \
             ownership-unverifiable: {error:?}"
        );
        assert_eq!(
            tombstone_names(&digest_hex).len(),
            1,
            "the refused tree must stay on disk under its tombstone"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_tree_with_a_malformed_owner_record_is_refused() {
        let _root = isolated_scratch_root("malformed-record");
        let digest_hex = digest_of("malformed-record");
        let tree = plant_record_less_tree(&digest_hex);
        let record = tree.join(OWNER_RECORD_NAME);
        std::fs::write(&record, b"not-a-record\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let error = reclaim_session_scratch_by_digest(&digest_hex).unwrap_err();

        assert!(
            matches!(&error, ScratchError::InvalidPath(_)),
            "a malformed owner record must be refused, not repaired: {error:?}"
        );
        assert_eq!(
            tombstone_names(&digest_hex).len(),
            1,
            "the refused tree must stay on disk under its tombstone"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_tree_with_an_owner_record_that_is_not_owner_only_is_refused() {
        let _root = isolated_scratch_root("foreign-record");
        let digest_hex = digest_of("foreign-record");
        let tree = plant_record_less_tree(&digest_hex);
        let record = tree.join(OWNER_RECORD_NAME);
        let mut expected = b"codegg-session-scratch-v1\n".to_vec();
        expected.extend_from_slice(digest_hex.as_bytes());
        expected.push(b'\n');
        std::fs::write(&record, &expected).unwrap();
        #[cfg(unix)]
        {
            // The ownership check has two halves: the record must be owned by
            // the sessions root's owner, and it must be owner-only. A record
            // another user can read is the same refusal — `WrongOwner` — and
            // it is the half a test can produce without root.
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        let error = reclaim_session_scratch_by_digest(&digest_hex).unwrap_err();

        assert!(
            matches!(&error, ScratchError::WrongOwner(_)),
            "an owner record that is not owner-only must be refused: {error:?}"
        );
        assert_eq!(
            tombstone_names(&digest_hex).len(),
            1,
            "the refused tree must stay on disk under its tombstone"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_tree_whose_owner_record_names_a_different_digest_is_refused() {
        let _root = isolated_scratch_root("foreign-digest-record");
        let digest_hex = digest_of("foreign-digest-record");
        let tree = plant_record_less_tree(&digest_hex);
        // The record vouches for a different generation: this tree is not
        // the one the record speaks for.
        let mut record = b"codegg-session-scratch-v1\n".to_vec();
        record.extend_from_slice(digest_of("a-different-digest").as_bytes());
        record.push(b'\n');
        let record_path = tree.join(OWNER_RECORD_NAME);
        std::fs::write(&record_path, &record).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&record_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let error = reclaim_session_scratch_by_digest(&digest_hex).unwrap_err();

        assert!(
            matches!(&error, ScratchError::InvalidPath(_)),
            "a record that names a different digest must be refused: {error:?}"
        );
        assert_eq!(
            tombstone_names(&digest_hex).len(),
            1,
            "the refused tree must stay on disk under its tombstone"
        );
    }

    /// The emptiness predicate fails closed.
    ///
    /// A directory whose entry list cannot be read is **not** empty: the
    /// predicate must not report "empty" for a tree it could not look at,
    /// because "empty" is what authorises the removal.
    #[cfg(unix)]
    #[test]
    fn the_emptiness_predicate_fails_closed_on_an_unreadable_directory() {
        use std::os::unix::fs::PermissionsExt;

        let _root = isolated_scratch_root("unreadable-predicate");
        let uid = current_owner_uid().unwrap();
        let tree = sessions_root().join("unreadable-predicate");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::set_permissions(&tree, std::fs::Permissions::from_mode(0o000)).unwrap();

        let empty = tree_is_empty(&tree, uid).unwrap();

        std::fs::set_permissions(&tree, std::fs::Permissions::from_mode(0o700)).unwrap();
        if uid == 0 {
            // Root bypasses the permission bits, so the entry list *is*
            // readable and the tree really is empty. The refusal the
            // predicate exists to prevent is not reachable as root.
            assert!(empty, "as root the entry list is readable");
            return;
        }
        assert!(
            !empty,
            "a directory whose entry list cannot be read is not empty"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn the_operator_path_agrees_with_the_automatic_path_about_a_record_less_tree() {
        let _root = isolated_scratch_root("operator-record-less");
        let empty_digest = digest_of("operator-empty");
        plant_record_less_tree(&empty_digest);
        let content_digest = digest_of("operator-content");
        let content_tree = plant_record_less_tree(&content_digest);
        std::fs::write(content_tree.join("artifact.txt"), "disposable").unwrap();

        assert!(
            reclaim_session_scratch_for_operator(&empty_digest).unwrap(),
            "the operator path removes an empty record-less tree"
        );
        assert!(tombstone_names(&empty_digest).is_empty());

        let error = reclaim_session_scratch_for_operator(&content_digest).unwrap_err();
        assert!(
            matches!(
                &error,
                ScratchError::Unreclaimable { reason, .. }
                    if *reason == ReclaimRefusal::OwnershipUnverifiable
            ),
            "the operator path refuses a content-bearing record-less tree too: {error:?}"
        );
        assert_eq!(
            tombstone_names(&content_digest).len(),
            1,
            "the refused tree must stay on disk under its tombstone"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_reclaim_failure_is_not_reported_as_a_provisioning_failure() {
        let _root = isolated_scratch_root("reclaim-error-text");
        let digest_hex = digest_of("reclaim-error-text");
        let tree = plant_record_less_tree(&digest_hex);
        std::fs::write(tree.join("artifact.txt"), "disposable").unwrap();

        let error = reclaim_session_scratch_by_digest(&digest_hex).unwrap_err();
        let rendered = error.to_string();

        assert!(
            !rendered.contains("failed to provision managed scratch"),
            "a reclaim failure must not wear the provisioning text: {rendered}"
        );
        assert!(
            rendered.contains("ownership_unverifiable"),
            "a reclaim failure must name the rule that refused it: {rendered}"
        );

        // The I/O variant carries the same rule: a reclaim failure says so.
        let io = ScratchError::ReclaimIo(std::io::Error::other("boom"));
        assert!(
            io.to_string().contains("failed to reclaim managed scratch"),
            "a reclaim I/O failure must name reclamation: {io}"
        );
        assert!(
            !io.to_string()
                .contains("failed to provision managed scratch"),
            "a reclaim I/O failure must not wear the provisioning text: {io}"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn the_census_agrees_with_the_reclaim_path_about_a_record_less_tree() {
        let _root = isolated_scratch_root("census-record-less");
        let empty_digest = digest_of("census-empty");
        plant_record_less_tree(&empty_digest);
        let content_digest = digest_of("census-content");
        let content_tree = plant_record_less_tree(&content_digest);
        std::fs::write(content_tree.join("artifact.txt"), "disposable").unwrap();

        let empty_usage = preview_session_scratch(&empty_digest).unwrap();
        assert_eq!(
            empty_usage.reclaimable,
            Some(true),
            "an empty record-less tree is reclaimable: the automatic path \
             would remove it"
        );
        assert_eq!(empty_usage.refusal, None);

        let content_usage = preview_session_scratch(&content_digest).unwrap();
        assert_eq!(
            content_usage.reclaimable,
            Some(false),
            "a content-bearing record-less tree is not reclaimable"
        );
        assert_eq!(
            content_usage.refusal,
            Some(ReclaimRefusal::OwnershipUnverifiable),
            "the census must name the rule the validator would apply"
        );

        // The verdict is not a separate opinion: it agrees with what the
        // reclaim path actually does, in both directions.
        assert!(reclaim_session_scratch_by_digest(&empty_digest).unwrap());
        assert!(reclaim_session_scratch_by_digest(&content_digest).is_err());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn the_test_redirect_leaves_the_real_scratch_root_untouched() {
        let real_root = dirs::cache_dir()
            .expect("resolved cache root")
            .join(SCRATCH_VENDOR_NAME)
            .join(SCRATCH_ROOT_NAME)
            .join("sessions");
        let before = scratch_root_manifest(&real_root);

        let root = isolated_scratch_root("redirect-proof");
        let session_id = format!("scratch-redirect-{}", uuid::Uuid::new_v4());
        let tree = session_scratch_dir(&session_id).unwrap();

        assert!(
            tree.starts_with(root.path()),
            "the tree must be allocated under the redirected root: {tree:?}"
        );
        assert!(
            !tree.starts_with(&real_root),
            "the tree must not be allocated in the real scratch root: {tree:?}"
        );
        assert!(
            !scratch_root_manifest(
                &root
                    .path()
                    .join(SCRATCH_VENDOR_NAME)
                    .join(SCRATCH_ROOT_NAME)
                    .join("sessions")
            )
            .is_empty(),
            "the redirected root must hold the tree the test allocated"
        );

        let after = scratch_root_manifest(&real_root);
        let digest = session_scratch_digest_hex(&session_id);
        assert!(
            !after.iter().any(|name| name.contains(&digest)),
            "the real scratch root must hold nothing for the tree the test \
             allocated: {after:?}"
        );
        assert_eq!(
            before, after,
            "the redirect must leave the real scratch root byte-identical"
        );
    }

    /// The product resolves the scratch cache root to the user's cache
    /// directory.
    ///
    /// No redirect is installed on this test's thread, so this reads the
    /// product's own resolution. It allocates nothing, so it writes nothing.
    #[test]
    fn the_scratch_root_resolves_under_the_user_cache_directory() {
        assert_eq!(
            scratch_cache_root().unwrap(),
            dirs::cache_dir().expect("resolved cache root")
        );
    }

    /// The names of the entries in a scratch sessions root.
    ///
    /// The manifest is the *name set*: a test that wrote into the real root
    /// would add an entry, and a test that reclaimed into it would remove
    /// one.
    #[cfg(any(unix, windows))]
    fn scratch_root_manifest(root: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(root) else {
            return Vec::new();
        };
        let mut names = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn the_digest_names_the_tree_and_cannot_be_inverted() {
        let session_id = "census-digest";
        let digest = session_scratch_digest(session_id);
        assert_eq!(hex_digest(&digest), session_scratch_digest_hex(session_id));
        assert_eq!(session_scratch_digest_hex(session_id).len(), 64);
        // The name is one-way: it is not the session id, and the session id
        // cannot be read back out of it.
        assert_ne!(session_scratch_digest_hex(session_id), session_id);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn census_measures_a_tree_without_mutating_it() {
        let root = tempfile::tempdir().unwrap();
        let tree = census_tree_fixture(root.path(), "census-measure");
        std::fs::write(tree.join("a.txt"), b"hello").unwrap();
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested/b.txt"), b"world!").unwrap();

        let report = census_sessions_root(root.path(), &ScratchCensusBudget::default());

        // The census measures every regular file in the tree, and the
        // daemon's own owner record is one of them — in production it is
        // there too, so the measurement a test reads is the measurement the
        // product produces.
        let record_bytes = std::fs::metadata(tree.join(OWNER_RECORD_NAME))
            .unwrap()
            .len();
        assert_eq!(report.tree_count, 1);
        assert_eq!(report.total_bytes, 11 + record_bytes);
        assert!(!report.truncated);
        let usage = &report.trees[0];
        assert_eq!(
            usage.digest,
            hex_digest(&session_scratch_digest("census-measure"))
        );
        assert_eq!(usage.file_count, 3);
        assert_eq!(usage.directory_count, 1);
        assert_eq!(usage.reclaimable, Some(true));
        assert_eq!(usage.refusal, None);
        assert!(usage.oldest_modified_ms.is_some());
        assert!(usage.newest_modified_ms.is_some());
        // The census is read-only: the tree is still exactly as it was.
        assert!(tree.join("a.txt").exists());
        assert!(tree.join("nested/b.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn census_reports_a_planted_symlink_as_a_refusal_without_traversing_it() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, b"outside-content").unwrap();

        let tree = census_tree_fixture(root.path(), "census-symlink");
        std::os::unix::fs::symlink(&outside_file, tree.join("escape")).unwrap();

        let report = census_sessions_root(root.path(), &ScratchCensusBudget::default());

        assert_eq!(report.tree_count, 1);
        let usage = &report.trees[0];
        assert_eq!(usage.symlink_count, 1);
        assert_eq!(usage.refusal, Some(ReclaimRefusal::Symlink));
        assert_eq!(usage.reclaimable, Some(false));
        // The link was reported, never traversed: the outside file is intact
        // and its content appears nowhere in the report.
        assert!(outside_file.exists());
        assert_eq!(std::fs::read(&outside_file).unwrap(), b"outside-content");
        assert!(!format!("{report:?}").contains("outside-content"));
    }

    #[cfg(unix)]
    #[test]
    fn census_counts_a_multiply_linked_file_without_refusing_the_tree() {
        let root = tempfile::tempdir().unwrap();
        let tree = census_tree_fixture(root.path(), "census-hardlink");
        std::fs::write(tree.join("a.txt"), b"hello").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::hard_link(tree.join("a.txt"), outside.path().join("alias.txt")).unwrap();

        let report = census_sessions_root(root.path(), &ScratchCensusBudget::default());

        let usage = &report.trees[0];
        // The count is an observation. A hard link is a file name, not a
        // recursive edge, so it does not stop the reclaim path.
        assert_eq!(usage.multiply_linked_count, 1);
        assert_eq!(usage.refusal, None);
        assert_eq!(usage.reclaimable, Some(true));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn census_reports_a_tombstone_as_quarantined() {
        let root = tempfile::tempdir().unwrap();
        let digest = hex_digest(&session_scratch_digest("census-tombstone"));
        let tombstone = root
            .path()
            .join(format!(".reclaim-{digest}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tombstone).unwrap();
        std::fs::write(tombstone.join("a.txt"), b"hello").unwrap();

        let report = census_sessions_root(root.path(), &ScratchCensusBudget::default());

        assert_eq!(report.tree_count, 1);
        let usage = &report.trees[0];
        assert!(usage.quarantined);
        assert_eq!(usage.digest, digest);
        assert_eq!(usage.total_bytes, 5);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn census_treats_a_missing_root_as_an_empty_report() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("absent");

        let report = census_sessions_root(&missing, &ScratchCensusBudget::default());

        assert_eq!(report.tree_count, 0);
        assert!(report.trees.is_empty());
        assert!(!report.truncated);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn census_stops_at_its_tree_budget_and_says_so() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..3 {
            let tree = census_tree_fixture(root.path(), &format!("census-budget-{index}"));
            std::fs::write(tree.join("a.txt"), b"hello").unwrap();
        }
        let budget = ScratchCensusBudget {
            max_trees: 2,
            ..ScratchCensusBudget::default()
        };

        let report = census_sessions_root(root.path(), &budget);

        assert_eq!(report.tree_count, 2);
        assert_eq!(report.unreported_trees, 1);
        assert!(report.truncated);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn census_stops_at_its_entry_budget_and_says_so() {
        let root = tempfile::tempdir().unwrap();
        let tree = census_tree_fixture(root.path(), "census-entries");
        for index in 0..8 {
            std::fs::write(tree.join(format!("f{index}.txt")), b"x").unwrap();
        }
        let budget = ScratchCensusBudget {
            max_entries_per_tree: 3,
            ..ScratchCensusBudget::default()
        };

        let report = census_sessions_root(root.path(), &budget);

        let usage = &report.trees[0];
        assert!(usage.truncated);
        assert!(report.truncated);
        assert!(usage.file_count <= 3);
        // A partially-read tree has no verdict. The entries the walk did not
        // reach could be ones the validator would refuse, so the census must
        // not claim the tree is reclaimable — nor claim it is not.
        assert_eq!(
            usage.reclaimable, None,
            "a truncated tree must not carry a reclaim verdict"
        );
    }

    #[cfg(unix)]
    #[test]
    fn census_reports_a_fifo_without_opening_it() {
        let root = tempfile::tempdir().unwrap();
        let tree = census_tree_fixture(root.path(), "census-fifo");
        let fifo_path = tree.join("pipe");
        nix::unistd::mkfifo(&fifo_path, nix::sys::stat::Mode::from_bits_truncate(0o600)).unwrap();

        let report = census_sessions_root(root.path(), &ScratchCensusBudget::default());

        let usage = &report.trees[0];
        assert_eq!(usage.non_regular_count, 1);
        assert_eq!(usage.refusal, Some(ReclaimRefusal::NonRegular));
        assert_eq!(usage.reclaimable, Some(false));
        // The FIFO was counted from metadata, never opened: opening one blocks
        // until a writer appears, which would hang the walk.
        assert!(fifo_path.exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn census_reports_a_tree_that_vanished_mid_walk() {
        let root = tempfile::tempdir().unwrap();
        let tree = census_tree_fixture(root.path(), "census-vanished");
        std::fs::write(tree.join("a.txt"), b"hello").unwrap();
        // The tree disappears between the enumeration and the measurement.
        std::fs::remove_dir_all(&tree).unwrap();

        let usage = census_tree(
            &ScratchTreeRef {
                digest: hex_digest(&session_scratch_digest("census-vanished")),
                path: tree,
                quarantined: false,
                modified_ms: None,
            },
            &ScratchCensusBudget::default(),
        );

        assert!(usage.vanished);
        assert_eq!(usage.file_count, 0);
        assert_eq!(usage.total_bytes, 0);
        // A tree that vanished was never read, so it carries no verdict.
        assert_eq!(
            usage.reclaimable, None,
            "a vanished tree must not carry a reclaim verdict"
        );
    }

    /// A name, size, and mtime manifest of a tree.
    #[cfg(any(unix, windows))]
    fn manifest(root: &Path) -> Vec<(String, u64, Option<std::time::SystemTime>)> {
        let mut entries = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(directory) = stack.pop() {
            let Ok(children) = std::fs::read_dir(&directory) else {
                continue;
            };
            for child in children.flatten() {
                let name = child.file_name().to_string_lossy().into_owned();
                let Ok(metadata) = std::fs::symlink_metadata(child.path()) else {
                    continue;
                };
                if metadata.is_dir() {
                    entries.push((format!("{name}/"), 0u64, metadata.modified().ok()));
                    stack.push(child.path());
                } else {
                    entries.push((name, metadata.len(), metadata.modified().ok()));
                }
            }
        }
        entries.sort();
        entries
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_full_census_leaves_the_root_entry_for_entry_identical() {
        let root = tempfile::tempdir().unwrap();
        let tree = census_tree_fixture(root.path(), "census-manifest");
        std::fs::write(tree.join("a.txt"), b"hello").unwrap();
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested/b.txt"), b"world!").unwrap();

        let before = manifest(root.path());
        let report = census_sessions_root(root.path(), &ScratchCensusBudget::default());
        assert_eq!(report.tree_count, 1);
        let after = manifest(root.path());

        // No file was created, renamed, removed, or changed, and no tombstone
        // appeared: the census cannot reclaim.
        assert_eq!(before, after);
        assert!(!after
            .iter()
            .any(|(name, _, _)| name.starts_with(".reclaim-")));
    }

    #[test]
    fn session_paths_are_stable_and_separate() {
        let _root = isolated_scratch_root("session-paths");
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
        let root = isolated_scratch_root("concurrent");
        let root_path = root.path().to_path_buf();
        let session_id = format!("scratch-concurrent-{}", uuid::Uuid::new_v4());
        let workers = (0..8)
            .map(|_| {
                let session_id = session_id.clone();
                let root_path = root_path.clone();
                std::thread::spawn(move || {
                    // The redirect is thread-local, so the worker installs it
                    // for itself rather than inheriting it.
                    let _root = TestScratchRoot::install(root_path);
                    session_scratch_dir(&session_id).unwrap()
                })
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
        let _root = isolated_scratch_root("delegated-roots");
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
        let _root = isolated_scratch_root("run-cleanup");
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
        let _root = isolated_scratch_root("session-cleanup");
        let session_id = format!("scratch-session-cleanup-{}", uuid::Uuid::new_v4());
        let run_id = codegg_core::identity::AgentRunId::new();
        let scratch_id = codegg_core::identity::ScratchId::new();
        let run_root = agent_run_scratch_dir(&session_id, &run_id).unwrap();
        let shared_root = shared_scratch_dir(&session_id, &scratch_id).unwrap();
        std::fs::write(run_root.join("run.tmp"), "temporary").unwrap();
        std::fs::write(shared_root.join("shared.tmp"), "temporary").unwrap();

        assert!(reclaim_session_scratch_dir(&session_id).unwrap());
        assert!(!run_root.exists());
        assert!(!shared_root.exists());
        assert!(!reclaim_session_scratch_dir(&session_id).unwrap());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn revoked_cleanup_recovers_an_orphaned_tombstone_after_restart() {
        let _root = isolated_scratch_root("orphaned-tombstone");
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
        let scratch_root = isolated_scratch_root("concurrent-reclaim");
        let root_path = scratch_root.path().to_path_buf();
        let session_id = format!("scratch-concurrent-reclaim-{}", uuid::Uuid::new_v4());
        let scratch_id = codegg_core::identity::ScratchId::new();
        let root = shared_scratch_dir(&session_id, &scratch_id).unwrap();
        std::fs::write(root.join("artifact.txt"), "disposable").unwrap();
        let workers = (0..2)
            .map(|_| {
                let session_id = session_id.clone();
                let scratch_id = scratch_id.clone();
                let root_path = root_path.clone();
                std::thread::spawn(move || {
                    // The redirect is thread-local, so the worker installs it
                    // for itself rather than inheriting it.
                    let _root = TestScratchRoot::install(root_path);
                    reclaim_shared_scratch_dir(&session_id, &scratch_id)
                })
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
    fn revoked_shared_cleanup_quarantines_symlinks_and_reclaims_hardlinks() {
        let _root = isolated_scratch_root("symlink-cleanup");
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
        // A hard link is a file name, not a recursive edge. Removing the name
        // under the tombstone only decrements its link count; it does not
        // modify or remove the outside file. Unix now behaves as Windows
        // already did.
        assert!(reclaim_shared_scratch_dir(&session_id, &hardlink_id).unwrap());
        assert_eq!(
            std::fs::read_to_string(&external_file).unwrap(),
            "outside content"
        );
        assert!(
            !hardlink_root.exists(),
            "the hardlinked resource is reclaimed, not quarantined"
        );
    }

    #[cfg(windows)]
    #[test]
    fn revoked_shared_cleanup_quarantines_reparse_points_and_unlinks_hardlinks_safely() {
        let _root = isolated_scratch_root("reparse-cleanup");
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

    /// A cache-root path inside a temporary directory.
    ///
    /// The allocator refuses a path that has a symlinked component, and a
    /// temporary directory's own ancestors can contain one (`/tmp` is a
    /// symlink on macOS), so the fixture canonicalizes the way the product's
    /// own scratch home does before it hands the path to the allocator.
    #[cfg(unix)]
    fn private_fixture_root(parent: &tempfile::TempDir) -> std::path::PathBuf {
        std::fs::canonicalize(parent.path())
            .expect("canonical fixture parent")
            .join("cache-root")
    }

    #[cfg(unix)]
    #[test]
    fn a_cache_root_the_product_creates_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let cache_root = private_fixture_root(&parent);
        let owner = current_owner_uid().unwrap();

        ensure_private_cache_root(&cache_root, owner).unwrap();

        let mode = std::fs::metadata(&cache_root).unwrap().permissions().mode() & 0o7777;
        assert_eq!(
            mode, 0o700,
            "a cache root the product creates must be private rather than umask-dependent: {mode:04o}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_group_writable_cache_root_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let cache_root = private_fixture_root(&parent);
        std::fs::create_dir(&cache_root).unwrap();
        std::fs::set_permissions(&cache_root, std::fs::Permissions::from_mode(0o775)).unwrap();
        let owner = current_owner_uid().unwrap();

        let error = ensure_private_cache_root(&cache_root, owner).unwrap_err();
        assert!(
            matches!(&error, ScratchError::WrongOwner(path) if path == &cache_root),
            "a group-writable cache root must be refused, not used: {error:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_world_writable_cache_root_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let cache_root = private_fixture_root(&parent);
        std::fs::create_dir(&cache_root).unwrap();
        std::fs::set_permissions(&cache_root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let owner = current_owner_uid().unwrap();

        let error = ensure_private_cache_root(&cache_root, owner).unwrap_err();
        assert!(
            matches!(&error, ScratchError::WrongOwner(path) if path == &cache_root),
            "a world-writable cache root must be refused, not used: {error:?}"
        );
    }

    #[test]
    fn the_session_root_lives_under_the_resolved_cache_root() {
        let redirect = isolated_scratch_root("cache-root");
        let session_id = format!("scratch-cache-root-{}", uuid::Uuid::new_v4());
        let root = session_scratch_dir(&session_id).unwrap();
        // The resolved cache root is the one the allocator resolved — the
        // redirect, in a test. The property under test is the containment,
        // not the value; `the_scratch_root_resolves_under_the_user_cache_directory`
        // pins the value.
        let cache_root = redirect.path().to_path_buf();
        // `session_scratch_dir` returns a **canonicalized** path, and on
        // Windows `canonicalize` yields the extended-length (`\\?\`) device
        // form while `dirs::cache_dir()` yields the plain drive form. The two
        // spell the same directory, so a raw comparison of one against the
        // other fails on its own spelling rather than on its own claim — the
        // containment check below has to compare like with like. Both sides
        // are canonicalized, which is the form the product itself resolves
        // and hands out.
        let canonical_cache_root =
            std::fs::canonicalize(&cache_root).expect("canonical resolved cache root");
        assert!(
            root.starts_with(&canonical_cache_root),
            "the scratch root must live under the resolved cache root: {root:?} is not under {canonical_cache_root:?}"
        );
        // The check above must still be able to *fail*: a path that is
        // genuinely outside the resolved cache root must not read as
        // contained. Without this, a comparison that always holds would
        // satisfy the assertion above.
        let outside = canonical_cache_root
            .parent()
            .expect("the cache root has a parent")
            .join("codegg-scratch-outside-the-cache-root");
        assert!(
            !outside.starts_with(&canonical_cache_root),
            "a path outside the resolved cache root must not read as contained: {outside:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn structured_file_tools_are_scoped_to_the_host_session() {
        use crate::tool::Tool;

        let _root = isolated_scratch_root("structured-tools");

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

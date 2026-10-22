//! The C007 publisher: private staging, compare-and-swap, atomic publication.
//!
//! See the module documentation in [`crate::git_publication`] for the
//! contract this implements.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use codegg_core::git_publication::{
    GitIndexPublicationIntent, GitIndexPublicationState, GitIndexPublicationStore,
    GitIndexPublicationStoreError, IndexFingerprint,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::staging::{GitIndexStaging, GitIndexStagingRequest};
use super::{
    GitIndexPublicationConflict, GitIndexPublicationOutcome, GitIndexPublicationReceipt,
    GitIndexPublicationRefusal, GitIndexPublicationRequest, ObtainedNetworkBoundary,
    PublishedResourceScope,
};

/// The configuration a private staging gitdir is created with.
///
/// The private gitdir is **authored by the daemon**, not produced by `git
/// init`. That is deliberate: the configuration a Git child reads is the
/// only place a clean/smudge filter could be defined, and a configuration we
/// author contains no `filter.*` section at all. A repository's own
/// `.gitattributes` may still *name* a filter, but with no `filter.<name>`
/// configuration in effect, Git has no command to run for it and passes the
/// content through unchanged.
///
/// `core.attributesFile` is pinned to `/dev/null` so the user-level
/// attributes file cannot reintroduce one, and the staging child runs with
/// `GIT_CONFIG_NOSYSTEM=1` and `GIT_CONFIG_GLOBAL=/dev/null` so the system
/// and global configuration cannot either.
const PRIVATE_GITDIR_CONFIG: &str = "\
[core]
\trepositoryformatversion = 0
\tfilemode = true
\tbare = true
\tattributesFile = /dev/null
\thooksPath = /dev/null
\tfsmonitor = false
\tautocrlf = false
[gc]
\tauto = 0
";

/// Failures of the publication machinery itself.
///
/// A *refusal* (the host cannot do it, the repository layout is out of
/// scope, the paths are unsupported) is not an error: it is a typed outcome
/// the caller can act on. These errors are the cases where the machinery
/// itself did not work.
#[derive(Debug, Error)]
pub enum GitIndexPublicationError {
    /// The daemon has no publication backend wired.
    #[error("git index publication backend is not configured: {0}")]
    BackendNotConfigured(String),
    /// The durable journal rejected an operation.
    #[error("git index publication journal error: {0}")]
    Journal(#[from] GitIndexPublicationStoreError),
    /// A filesystem operation failed.
    #[error("git index publication filesystem error: {0}")]
    Io(#[from] std::io::Error),
    /// The staging step did not produce a usable private index.
    #[error("git index publication staging error: {0}")]
    Staging(String),
    /// The publication was refused before it was attempted.
    ///
    /// The refusal is a typed outcome, not a failure: the caller decides
    /// what to do about it.
    #[error("git index publication refused: {0}")]
    Refused(GitIndexPublicationRefusal),
}

/// The publication backend.
///
/// It owns the durable journal, the daemon-owned scratch, and the staging
/// step. Every publication it performs is recorded in the journal before
/// the repository is touched, so a crash is recoverable.
#[derive(Clone)]
pub struct GitIndexPublisher {
    store: Arc<GitIndexPublicationStore>,
    scratch_root: PathBuf,
    staging: Arc<dyn GitIndexStaging>,
    timeout: Duration,
}

impl std::fmt::Debug for GitIndexPublisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitIndexPublisher")
            .field("scratch_root", &self.scratch_root)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl GitIndexPublisher {
    /// Build a publisher.
    ///
    /// The scratch root must be daemon-owned and must survive a restart: the
    /// reconciler needs to find the private index a crashed publication left
    /// behind, and its path is recorded in the journal.
    pub fn new(
        store: Arc<GitIndexPublicationStore>,
        scratch_root: PathBuf,
        staging: Arc<dyn GitIndexStaging>,
        timeout: Duration,
    ) -> Self {
        Self {
            store,
            scratch_root,
            staging,
            timeout,
        }
    }

    /// The durable journal this publisher records into.
    pub fn store(&self) -> &Arc<GitIndexPublicationStore> {
        &self.store
    }

    /// The daemon-owned scratch root.
    pub fn scratch_root(&self) -> &Path {
        &self.scratch_root
    }

    /// Publish an authorized `git add`.
    ///
    /// The sequence is: validate, stage privately, verify the repository was
    /// not touched, record the durable intent, then publish under Git's own
    /// index lock with a compare-and-swap against the index generation the
    /// staging observed.
    pub async fn publish_add(
        &self,
        request: &GitIndexPublicationRequest,
    ) -> Result<GitIndexPublicationOutcome, GitIndexPublicationError> {
        let prepared = match self.stage_private_index(request).await {
            Ok(prepared) => prepared,
            Err(GitIndexPublicationError::Refused(refusal)) => {
                return Ok(GitIndexPublicationOutcome::Refused(refusal))
            }
            Err(error) => return Err(error),
        };
        self.publish_prepared_index(request, &prepared).await
    }

    /// Stage the requested paths into a private index, without publishing.
    ///
    /// This is the staging half of the vertical. It is public so the window
    /// between staging and publication can be reached deterministically —
    /// which is where a concurrent external writer has to be caught.
    ///
    /// The repository is verified to be byte-identical when this returns: the
    /// staging must not have touched the real index or the real object
    /// database.
    pub async fn stage_private_index(
        &self,
        request: &GitIndexPublicationRequest,
    ) -> Result<PreparedPrivateIndex, GitIndexPublicationError> {
        let paths = match validate_paths(&request.paths) {
            Ok(paths) => paths,
            Err(reason) => {
                return Err(GitIndexPublicationError::Refused(
                    GitIndexPublicationRefusal::PathsUnsupported { reason },
                ))
            }
        };

        let root = canonicalize_existing(&request.repository_root)?;
        let gitdir = match resolve_gitdir(&root) {
            Ok(gitdir) => gitdir,
            Err(reason) => {
                return Err(GitIndexPublicationError::Refused(
                    GitIndexPublicationRefusal::RepositoryLayoutUnsupported { reason },
                ))
            }
        };
        if let Err(reason) = reject_submodule_paths(&root, &paths) {
            return Err(GitIndexPublicationError::Refused(
                GitIndexPublicationRefusal::PathsUnsupported { reason },
            ));
        }

        let Some((dev, ino)) = path_identity(&gitdir) else {
            return Err(GitIndexPublicationError::Refused(
                GitIndexPublicationRefusal::HostUnsupported {
                    reason: "this platform does not expose a Git directory identity, so a \
                             publication cannot be bound to the repository it observed"
                        .into(),
                },
            ));
        };

        let component = match scratch_component(&request.intent_id) {
            Ok(component) => component,
            Err(reason) => {
                return Err(GitIndexPublicationError::Refused(
                    GitIndexPublicationRefusal::BackendNotConfigured { reason },
                ))
            }
        };
        let scratch = self.scratch_root.join(&component);
        fs::create_dir_all(&scratch)?;
        let scratch = fs::canonicalize(&scratch)?;
        if scratch.starts_with(&root) {
            return Err(GitIndexPublicationError::Refused(
                GitIndexPublicationRefusal::BackendNotConfigured {
                    reason: "the publication scratch must live outside the repository".into(),
                },
            ));
        }

        // The index generation this publication is based on. It is the
        // compare-and-swap operand and it is recorded in the journal before
        // the repository is touched.
        let expected = fingerprint_index(&gitdir)?;

        let private = match PrivateGitDir::prepare(&scratch, &gitdir) {
            Ok(private) => private,
            Err(error) => {
                let _ = fs::remove_dir_all(&scratch);
                return Err(error);
            }
        };

        let network_boundary = match self
            .staging
            .stage(&GitIndexStagingRequest {
                private_gitdir: private.gitdir.clone(),
                private_index: private.index_path.clone(),
                repository_root: root.clone(),
                repository_gitdir: gitdir.clone(),
                paths: paths.clone(),
                intent_id: component.clone(),
                scratch_root: self.scratch_root.clone(),
                timeout: self.timeout,
            })
            .await
        {
            Ok(boundary) => boundary,
            Err(reason) => {
                // A failed staging must leave the repository byte-identical, and
                // the private artifact is discarded rather than left to be
                // reconciled.
                let _ = fs::remove_dir_all(&scratch);
                return Err(GitIndexPublicationError::Refused(
                    GitIndexPublicationRefusal::StagingFailed { reason },
                ));
            }
        };

        // The staging child must not have touched the repository. The mount
        // view makes the repository's Git metadata read-only to the child, so
        // this is a check on the containment, not on the child's goodwill.
        if fingerprint_index(&gitdir)? != expected {
            let _ = fs::remove_dir_all(&scratch);
            return Err(GitIndexPublicationError::Staging(
                "the staging child changed the real index".into(),
            ));
        }
        if path_identity(&gitdir) != Some((dev, ino)) {
            let _ = fs::remove_dir_all(&scratch);
            return Err(GitIndexPublicationError::Staging(
                "the real Git directory was replaced during staging".into(),
            ));
        }

        let published = match fingerprint_file(&private.index_path)? {
            Some(digest) => IndexFingerprint::Digest(digest),
            None => {
                let _ = fs::remove_dir_all(&scratch);
                return Err(GitIndexPublicationError::Staging(
                    "the staging child produced no private index".into(),
                ));
            }
        };

        Ok(PreparedPrivateIndex {
            private_gitdir: private.gitdir.clone(),
            private_index: private.index_path.clone(),
            expected_index_fingerprint: expected,
            published_index_fingerprint: published,
            gitdir_dev: dev,
            gitdir_ino: ino,
            network_boundary,
        })
    }

    /// The publication half of the vertical.
    ///
    /// This is the compare-and-swap, the Git index lock, the object import,
    /// and the atomic replacement — everything after the staging. It is
    /// public so the publication behaviour can be exercised on a host that
    /// cannot provide the M004/M005 staging tier, and so the window between
    /// staging and publication can be reached deterministically.
    ///
    /// The authority boundary is unchanged: the repository is derived from
    /// the request, not from the prepared artifact, and the compare-and-swap
    /// operand is the fingerprint the staging observed.
    pub async fn publish_prepared_index(
        &self,
        request: &GitIndexPublicationRequest,
        prepared: &PreparedPrivateIndex,
    ) -> Result<GitIndexPublicationOutcome, GitIndexPublicationError> {
        let published = prepared.published_index_fingerprint.clone();
        let root = canonicalize_existing(&request.repository_root)?;
        let gitdir = resolve_gitdir(&root).map_err(GitIndexPublicationError::Staging)?;
        let paths = request.paths.as_slice();
        let component = scratch_component(&request.intent_id).map_err(|reason| {
            GitIndexPublicationError::Staging(format!(
                "the publication identity is not usable: {reason}"
            ))
        })?;
        let private = PrivateGitDir {
            gitdir: prepared.private_gitdir.clone(),
            objects: prepared.private_gitdir.join("objects"),
            index_path: prepared.private_index.clone(),
        };
        let expected = prepared.expected_index_fingerprint.clone();
        let (dev, ino) = (prepared.gitdir_dev, prepared.gitdir_ino);

        let intent = GitIndexPublicationIntent {
            id: request.intent_id.clone(),
            workspace_root: request.workspace_root.to_string_lossy().into_owned(),
            gitdir_path: gitdir.to_string_lossy().into_owned(),
            gitdir_dev: dev,
            gitdir_ino: ino,
            private_index_path: private.index_path.to_string_lossy().into_owned(),
            expected_index_fingerprint: expected.clone(),
            published_index_fingerprint: published.clone(),
            staged_paths: paths.to_vec(),
            created_at_ms: now_ms(),
        };
        self.store.record_intent(&intent).await?;

        match self
            .publish_under_lock(&intent, &gitdir, &private, &component, expected, (dev, ino))
            .await
        {
            Ok(imported) => {
                self.store
                    .mark(
                        &intent.id,
                        GitIndexPublicationState::Committed,
                        None,
                        now_ms(),
                    )
                    .await?;
                Ok(GitIndexPublicationOutcome::Committed(
                    GitIndexPublicationReceipt {
                        intent_id: intent.id,
                        state: GitIndexPublicationState::Committed,
                        scope: PublishedResourceScope::IndexOnly,
                        published_index_fingerprint: published,
                        staged_paths: paths.to_vec(),
                        imported_objects: imported,
                        // The boundary is copied from what the staging step
                        // observed. The receipt therefore states the network
                        // it actually ran under rather than staying silent
                        // about the network, and it never claims a boundary
                        // the staging did not obtain.
                        network_boundary: prepared.network_boundary.clone(),
                        published_at_ms: now_ms(),
                    },
                ))
            }
            Err(PublishFailure::Conflict(conflict)) => {
                self.store
                    .mark(
                        &intent.id,
                        GitIndexPublicationState::Aborted,
                        Some(conflict.as_str()),
                        now_ms(),
                    )
                    .await?;
                Ok(GitIndexPublicationOutcome::Conflict(conflict))
            }
            Err(PublishFailure::Fatal(error)) => {
                // The journal entry stays non-terminal only if this mark
                // itself fails; the reconciler then resolves it from the
                // index fingerprint, so a failed mark is self-healing.
                self.store
                    .mark(
                        &intent.id,
                        GitIndexPublicationState::Aborted,
                        Some("publication failed before the index was replaced"),
                        now_ms(),
                    )
                    .await?;
                Err(error)
            }
        }
    }

    /// Take Git's own index lock, re-check the compare-and-swap, import the
    /// staged objects, and replace the index atomically.
    async fn publish_under_lock(
        &self,
        intent: &GitIndexPublicationIntent,
        gitdir: &Path,
        private: &PrivateGitDir,
        component: &str,
        expected: IndexFingerprint,
        identity: (u64, u64),
    ) -> Result<Vec<String>, PublishFailure> {
        // Durable marker that the lock acquisition is in flight. A crash
        // after this point is recoverable, and a crash before it leaves a
        // `prepared` entry that the reconciler aborts.
        self.store
            .mark(
                &intent.id,
                GitIndexPublicationState::Publishing,
                None,
                now_ms(),
            )
            .await
            .map_err(|error| PublishFailure::Fatal(error.into()))?;

        // The new index is written into a temporary file **in the gitdir**,
        // so that taking Git's lock is a same-filesystem hard link. The lock
        // therefore never exists with the wrong or empty content: it is
        // correct from the instant it exists, which is what lets the
        // reconciler attribute a leftover lock to this publication.
        let temporary = gitdir.join(format!(".codegg-publish-{component}"));
        let bytes =
            fs::read(&private.index_path).map_err(|error| PublishFailure::Fatal(error.into()))?;
        write_new_file(&temporary, &bytes).map_err(|error| PublishFailure::Fatal(error.into()))?;

        let lock = gitdir.join("index.lock");
        match fs::hard_link(&temporary, &lock) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temporary);
                return Err(PublishFailure::Conflict(
                    GitIndexPublicationConflict::IndexLockHeld,
                ));
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(PublishFailure::Fatal(error.into()));
            }
        }
        let _ = fs::remove_file(&temporary);

        // The compare-and-swap, evaluated under the lock. The gitdir identity
        // is checked first: if the directory was replaced, the index path we
        // are about to read belongs to a different repository.
        if path_identity(gitdir) != Some(identity) {
            let _ = fs::remove_file(&lock);
            return Err(PublishFailure::Conflict(
                GitIndexPublicationConflict::GitDirIdentityChanged {
                    expected_dev: identity.0,
                    expected_ino: identity.1,
                },
            ));
        }
        let observed = fingerprint_index(gitdir).map_err(PublishFailure::Fatal)?;
        if observed != expected {
            let _ = fs::remove_file(&lock);
            return Err(PublishFailure::Conflict(
                GitIndexPublicationConflict::StaleIndex { expected, observed },
            ));
        }

        // The staged objects reach the real object database only now: after
        // the lock is held and the compare-and-swap has succeeded, so a
        // conflicting publication writes nothing at all.
        let imported = match self.import_objects(gitdir, private).await {
            Ok(imported) => imported,
            Err(error) => {
                let _ = fs::remove_file(&lock);
                return Err(PublishFailure::Fatal(error));
            }
        };

        // The publication itself: an atomic rename within the gitdir.
        fs::rename(&lock, gitdir.join("index"))
            .map_err(|error| PublishFailure::Fatal(error.into()))?;
        fsync_directory(gitdir).map_err(|error| PublishFailure::Fatal(error.into()))?;
        Ok(imported)
    }

    /// Copy the objects the staging wrote into the real object database.
    ///
    /// The private object database contains only what the staging child
    /// wrote — the real one is reachable read-only through
    /// `objects/info/alternates` — so enumerating it is exactly the set of
    /// objects the published index needs and does not yet have.
    ///
    /// The copy is content-addressed and therefore idempotent: the object's
    /// name *is* its content hash, so an object that is already present has
    /// identical content and is left alone.
    async fn import_objects(
        &self,
        gitdir: &Path,
        private: &PrivateGitDir,
    ) -> Result<Vec<String>, GitIndexPublicationError> {
        let real_objects = gitdir.join("objects");
        let mut imported = Vec::new();
        for (identity, source) in private.new_loose_objects()? {
            let target = real_objects.join(&identity[..2]).join(&identity[2..]);
            if target.exists() {
                continue;
            }
            let parent = target
                .parent()
                .ok_or_else(|| GitIndexPublicationError::Staging("invalid object path".into()))?;
            fs::create_dir_all(parent)?;
            // Write to a temporary name and rename, so a reader never sees a
            // half-written object. This is Git's own loose-object protocol.
            let temporary = real_objects.join(format!("tmp_codegg_{identity}"));
            fs::copy(&source, &temporary)?;
            verify_identical_bytes(&source, &temporary)?;
            fs::rename(&temporary, &target)?;
            fsync_directory(parent)?;
            imported.push(identity);
        }
        Ok(imported)
    }
}

/// A private index that has been staged and is ready to be published.
///
/// The staging step produces this; the publication step consumes it. It
/// carries the compare-and-swap operand — the fingerprint of the
/// repository's index the staging was based on — and the gitdir identity the
/// staging observed, so the publication can detect a repository that changed
/// underneath it.
#[derive(Debug, Clone)]
pub struct PreparedPrivateIndex {
    /// The private gitdir, which holds the private object database.
    pub private_gitdir: PathBuf,
    /// The private index file.
    pub private_index: PathBuf,
    /// The fingerprint of the repository's index the staging observed.
    pub expected_index_fingerprint: IndexFingerprint,
    /// The fingerprint of the private index that was staged.
    pub published_index_fingerprint: IndexFingerprint,
    /// The device identity of the gitdir the staging observed.
    pub gitdir_dev: u64,
    /// The inode identity of the gitdir the staging observed.
    pub gitdir_ino: u64,
    /// The network boundary the staging step obtained, stated truthfully.
    ///
    /// It is copied verbatim into the publication receipt, so the receipt
    /// can never claim a boundary the staging did not obtain.
    pub network_boundary: ObtainedNetworkBoundary,
}

/// How a publication attempt ended before it could commit.
enum PublishFailure {
    /// The repository changed under the publication. Nothing was written.
    Conflict(GitIndexPublicationConflict),
    /// The machinery failed. Nothing was published.
    Fatal(GitIndexPublicationError),
}

/// The private staging gitdir.
struct PrivateGitDir {
    /// The private gitdir root.
    gitdir: PathBuf,
    /// The private object database.
    objects: PathBuf,
    /// The private index.
    index_path: PathBuf,
}

impl PrivateGitDir {
    /// Prepare the private gitdir and seed its index from the real index.
    fn prepare(scratch: &Path, real_gitdir: &Path) -> Result<Self, GitIndexPublicationError> {
        let gitdir = scratch.join("gitdir");
        let objects = gitdir.join("objects");
        fs::create_dir_all(objects.join("info"))?;
        fs::create_dir_all(objects.join("pack"))?;
        fs::create_dir_all(gitdir.join("refs").join("heads"))?;
        fs::create_dir_all(gitdir.join("refs").join("tags"))?;

        // The real object database, reachable read-only. Git never writes
        // through an alternate, so the staging child's new blobs stay in the
        // private object database.
        let real_objects = canonicalize_existing(&real_gitdir.join("objects"))?;
        fs::write(
            objects.join("info").join("alternates"),
            format!("{}\n", real_objects.display()),
        )?;

        // A gitdir we author. Its configuration contains no `filter.*`
        // section, so no repository-controlled command can run in the
        // staging child.
        fs::write(gitdir.join("HEAD"), "ref: refs/heads/main\n")?;
        fs::write(gitdir.join("config"), PRIVATE_GITDIR_CONFIG)?;
        fs::write(
            gitdir.join("description"),
            "CodeGG private staging gitdir\n",
        )?;

        // Seed the private index from the **real** index, byte for byte, so
        // the staged result is based on the current index generation. Seeding
        // from `HEAD` instead would silently drop changes that are already
        // staged but not committed.
        let index_path = gitdir.join("index");
        let real_index = real_gitdir.join("index");
        if real_index.exists() {
            fs::copy(&real_index, &index_path)?;
        }

        Ok(Self {
            gitdir,
            objects,
            index_path,
        })
    }

    /// The loose objects the staging child wrote into the private object
    /// database.
    fn new_loose_objects(&self) -> Result<Vec<(String, PathBuf)>, GitIndexPublicationError> {
        let mut found = Vec::new();
        let entries = fs::read_dir(&self.objects)?;
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let fanout = entry.file_name();
            let fanout = fanout.to_string_lossy();
            if fanout.len() != 2 || !fanout.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            for object in fs::read_dir(entry.path())? {
                let object = object?;
                if !object.file_type()?.is_file() {
                    continue;
                }
                let name = object.file_name();
                let name = name.to_string_lossy();
                // SHA-1 and SHA-256 repositories both use a hexadecimal
                // object name; the two lengths are the only ones Git emits.
                if !matches!(name.len(), 38 | 64) || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
                    continue;
                }
                found.push((format!("{fanout}{name}"), object.path()));
            }
        }
        Ok(found)
    }
}

/// Validate the requested paths' shape.
///
/// The caller has already validated that each path is inside the repository
/// root. This is the second, independent check: a path that is absolute, or
/// that contains a parent component, is refused before it can reach a Git
/// child.
fn validate_paths(paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("no paths to stage".into());
    }
    let mut validated = Vec::with_capacity(paths.len());
    for path in paths {
        if path.is_empty() {
            return Err("a staged path is empty".into());
        }
        if path.contains('\0') {
            return Err("a staged path contains a NUL byte".into());
        }
        let candidate = Path::new(path);
        if candidate.is_absolute() {
            return Err(format!("staged path '{path}' is absolute"));
        }
        if candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        }) {
            return Err(format!("staged path '{path}' escapes the repository"));
        }
        validated.push(path.clone());
    }
    Ok(validated)
}

/// Refuse paths that would stage a submodule.
///
/// A submodule is staged as a gitlink that references a commit object. That
/// object is not written by the staging child, so it would not be imported
/// with the rest, and the published index could reference an object the
/// repository does not have.
fn reject_submodule_paths(root: &Path, paths: &[String]) -> Result<(), String> {
    for path in paths {
        let candidate = root.join(path);
        if candidate.is_dir() && candidate.join(".git").exists() {
            return Err(format!(
                "staged path '{path}' is a submodule, which this vertical does not publish"
            ));
        }
    }
    Ok(())
}

/// Resolve the repository's real gitdir.
///
/// Only an ordinary repository with a directory `.git` is supported. A
/// linked worktree (a `.git` *file*) and a bare repository are out of scope
/// for this vertical, and saying so is better than publishing into a
/// geometry the publisher does not understand.
fn resolve_gitdir(root: &Path) -> Result<PathBuf, String> {
    let entry = root.join(".git");
    let metadata = fs::symlink_metadata(&entry)
        .map_err(|error| format!("the repository metadata is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("a symlinked .git metadata path is not supported by this vertical".into());
    }
    if !metadata.is_dir() {
        return Err(
            "this vertical supports ordinary repositories with a directory .git only".into(),
        );
    }
    fs::canonicalize(&entry)
        .map_err(|error| format!("the repository metadata is unusable: {error}"))
}

/// The device/inode identity of a directory, when the platform exposes one.
pub fn path_identity(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).ok()?;
        Some((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// The fingerprint of the repository's real index.
pub fn fingerprint_index(gitdir: &Path) -> Result<IndexFingerprint, GitIndexPublicationError> {
    Ok(match fingerprint_file(&gitdir.join("index"))? {
        Some(digest) => IndexFingerprint::Digest(digest),
        None => IndexFingerprint::Absent,
    })
}

/// The SHA-256 of a file's bytes, or `None` when the file does not exist.
pub fn fingerprint_file(path: &Path) -> Result<Option<String>, GitIndexPublicationError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(sha256_hex(&bytes))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The lowercase hexadecimal SHA-256 of some bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The current time in milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// A safe scratch directory component for an intent identity.
pub fn scratch_component(intent_id: &str) -> Result<String, String> {
    if intent_id.is_empty() || intent_id.len() > 200 {
        return Err("the publication identity is not a usable directory name".into());
    }
    if !intent_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("the publication identity is not a usable directory name".into());
    }
    Ok(intent_id.to_string())
}

/// Resolve an existing path to its canonical form.
fn canonicalize_existing(path: &Path) -> Result<PathBuf, GitIndexPublicationError> {
    fs::canonicalize(path).map_err(|error| {
        GitIndexPublicationError::Io(std::io::Error::new(
            error.kind(),
            format!("{}: {error}", path.display()),
        ))
    })
}

/// Create a new file with the given contents and flush it to disk.
fn write_new_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Verify that two files hold the same bytes.
fn verify_identical_bytes(left: &Path, right: &Path) -> std::io::Result<()> {
    let left_bytes = fs::read(left)?;
    let right_bytes = fs::read(right)?;
    if left_bytes != right_bytes {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "the copied object does not match its source",
        ));
    }
    Ok(())
}

/// Flush a directory entry so a rename survives a crash.
fn fsync_directory(path: &Path) -> std::io::Result<()> {
    let file = fs::File::open(path)?;
    file.sync_all()
}

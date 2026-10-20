//! C007: production publication of a private Git index.
//!
//! This is the C004 production vertical. An authorized `git add` is staged
//! into a **private** index in daemon-owned scratch and is only then
//! published into the real repository index.
//!
//! # Why the publication is not a plain `git add`
//!
//! A plain `git add` mutates the repository index in place, under Git's
//! `index.lock`, with no compare-and-swap and no durable record of what was
//! intended. Two things go wrong with that:
//!
//! * a **concurrent writer** can have its index silently overwritten, because
//!   the lock only excludes *cooperating* writers and the publisher cannot
//!   fence an external `git`;
//! * a **crash** between "the intent existed" and "the index was replaced"
//!   leaves no evidence of what was intended, so the outcome cannot be
//!   recovered — only guessed at.
//!
//! The publisher fixes both: it stages privately, records a durable intent,
//! then publishes under Git's own lock with a compare-and-swap against the
//! index generation the staging observed.
//!
//! # What this vertical publishes
//!
//! Exactly one resource: the repository **index**. Refs are not published and
//! the worktree is not modified. Multi-resource atomicity across index, refs,
//! and the worktree is a separate and larger problem; a receipt that implied
//! otherwise would repeat the failure mode this corrective exists to avoid.
//! See [`PublishedResourceScope`].
//!
//! # What this vertical does not do
//!
//! Merge, rebase, checkout, reset, clean, fetch, push, and pull are all out
//! of scope. A single `Add` is the whole surface.
//!
//! # The staging child
//!
//! The staging child is the daemon-pinned Git executable, launched through
//! [`crate::managed_process::ManagedProcessService`] with the M004
//! process/IPC tier and the M005 offline-network candidate. The repository
//! therefore cannot influence what the child executes, and the real index and
//! the real object database are read-only to it (the mount view protects the
//! repository's Git metadata under the writable workspace root).
//!
//! The child writes into a private gitdir in daemon-owned scratch:
//!
//! * a **private index**, seeded byte-for-byte from the real index so the
//!   staged result is based on the *current* index generation rather than on
//!   `HEAD` (an already-staged change must not be lost by staging);
//! * a **private object database**, whose `objects/info/alternates` points at
//!   the real object database so existing objects resolve read-only while new
//!   blobs stay private.
//!
//! `GIT_INDEX_FILE` is deliberately **not** used: `EnvironmentPolicy` denies
//! that variable by design (`codegg_git::ALWAYS_STRIPPED_ENV_VARS`), and
//! reintroducing it would weaken a deliberate deny rule to obtain a property
//! the private gitdir already provides — the child's index is in scratch and
//! the real index is unreachable.
//!
//! # The object import
//!
//! A staged blob lives in the private object database, so it has to reach the
//! real one before the index that references it is published. The import is
//! performed by the **daemon**, not by a child: the M004/M005 mount view
//! makes the repository's Git metadata read-only to children *by design*, so
//! a child cannot perform it, and a containment profile that could would
//! contradict that invariant.
//!
//! The import is content-addressed and therefore idempotent: a loose object
//! is copied under its own content hash, an object that is already present is
//! left alone, and the copy is verified byte-for-byte against its source. It
//! cannot corrupt an existing object, because an existing object with the
//! same name has the same content.
//!
//! The import runs **under the index lock and after the compare-and-swap
//! succeeds**, so a conflicting publication writes nothing at all.

pub mod publisher;
pub mod reconciler;
pub mod staging;

use std::path::PathBuf;

use codegg_core::git_publication::{GitIndexPublicationState, IndexFingerprint};

pub use publisher::{GitIndexPublicationError, GitIndexPublisher, PreparedPrivateIndex};
pub use reconciler::{
    reconcile_git_index_publications_at_startup, GitIndexPublicationReconciliation,
};
pub use staging::{ContainedGitIndexStaging, GitIndexStaging, GitIndexStagingRequest};

/// The resource scope of a publication.
///
/// C007 publishes exactly one resource. This is not a formality: the receipt
/// is the only durable statement the caller gets about what happened, and a
/// receipt that implied the refs were published too would be a false claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishedResourceScope {
    /// Only the repository index was published. No ref was written and the
    /// worktree was not modified.
    IndexOnly,
}

impl PublishedResourceScope {
    /// A stable, human-readable name for the scope.
    pub fn as_str(&self) -> &'static str {
        match self {
            PublishedResourceScope::IndexOnly => "index",
        }
    }
}

/// The network boundary a publication obtained, stated truthfully.
///
/// The boundary is **stated**, never implied. A receipt that said nothing
/// about the network would leave a caller unable to tell "there was no
/// boundary" from "the boundary was not surfaced" — and a receipt that
/// claimed a boundary the system did not obtain would be worse than either.
///
/// Each variant has a producer:
///
/// * `Enforced` — the publication's staging child was observed inside a
///   private network namespace. The producer is the launch itself: the
///   Bubblewrap setup receipt proves the namespace setup completed, and the
///   launch reports the boundary it obtained on
///   `SandboxExecutionOutcome::Enforced::network`.
/// * `Unrestricted` — the publication's staging child ran and obtained no
///   network namespace.
/// * `NotReported` — the staging step does not run a contained child, so
///   there is no launch whose obtained boundary could be read. This is the
///   truthful answer for a test-supplied staging, and it is a *statement*,
///   not an omission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObtainedNetworkBoundary {
    /// The staging step did not run a contained child, so the system does
    /// not report the network boundary it obtained.
    NotReported,
    /// The staging child was observed inside a private network namespace.
    Enforced { backend: String },
    /// The staging child ran without a network namespace.
    Unrestricted,
}

impl ObtainedNetworkBoundary {
    /// A stable, human-readable name for the boundary.
    pub fn as_str(&self) -> &'static str {
        match self {
            ObtainedNetworkBoundary::NotReported => "not_reported",
            ObtainedNetworkBoundary::Enforced { .. } => "enforced",
            ObtainedNetworkBoundary::Unrestricted => "unrestricted",
        }
    }

    /// Read the obtained boundary off a launch outcome.
    ///
    /// This is the only place the receipt's reported case is produced. It
    /// reads the launch's own obtained fact; it never reads the *request*,
    /// so a receipt can never claim a boundary the launch did not obtain.
    pub fn from_launch(outcome: &crate::managed_process::SandboxExecutionOutcome) -> Self {
        use codegg_core::approval::NetworkEnforcement;
        match outcome {
            crate::managed_process::SandboxExecutionOutcome::Enforced {
                network: NetworkEnforcement::Enforced { backend },
                ..
            } => ObtainedNetworkBoundary::Enforced {
                backend: backend.clone(),
            },
            crate::managed_process::SandboxExecutionOutcome::Enforced { .. } => {
                ObtainedNetworkBoundary::Unrestricted
            }
            _ => ObtainedNetworkBoundary::NotReported,
        }
    }
}

/// A request to publish a staged `git add` into the real repository index.
#[derive(Debug, Clone)]
pub struct GitIndexPublicationRequest {
    /// The stable identity of this publication attempt. It is also the
    /// journal key, so it must never be reused for a different attempt.
    pub intent_id: String,
    /// The repository root. The publisher resolves the real gitdir from it.
    pub repository_root: PathBuf,
    /// The workspace the mutation was authorized against. It is recorded in
    /// the journal so a recovered publication can be traced back to the
    /// workspace lease that authorized it.
    pub workspace_root: PathBuf,
    /// The repository-relative paths to stage.
    ///
    /// The caller is responsible for validating that each path is inside the
    /// repository root (the existing `codegg_git::path::RepoPath` validation
    /// does this). The publisher re-validates the shape — relative, no parent
    /// components, no prefix — and additionally forces literal pathspec
    /// semantics so a magic pathspec cannot widen the staged set.
    pub paths: Vec<String>,
}

/// The outcome of a publication attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitIndexPublicationOutcome {
    /// The real index now holds the published content.
    Committed(GitIndexPublicationReceipt),
    /// The repository changed under the publication, so it was refused.
    ///
    /// This is a typed, retryable result — not a crash and not a failure of
    /// the publication machinery.
    Conflict(GitIndexPublicationConflict),
    /// The publication was never attempted.
    Refused(GitIndexPublicationRefusal),
}

impl GitIndexPublicationOutcome {
    /// Whether the publication committed.
    pub fn is_committed(&self) -> bool {
        matches!(self, GitIndexPublicationOutcome::Committed(_))
    }
}

/// A durable statement of what a publication did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexPublicationReceipt {
    /// The journal identity of the publication.
    pub intent_id: String,
    /// The terminal state the publication ended in.
    pub state: GitIndexPublicationState,
    /// What this publication published. Exactly one resource.
    pub scope: PublishedResourceScope,
    /// The fingerprint of the index that was published.
    pub published_index_fingerprint: IndexFingerprint,
    /// The repository-relative paths that were staged.
    pub staged_paths: Vec<String>,
    /// The object identities that were imported into the real object
    /// database to make the published index resolvable.
    pub imported_objects: Vec<String>,
    /// The network boundary this publication obtained.
    ///
    /// Stated explicitly even when the answer is "not reported", so a caller
    /// never has to infer an omission. A receipt that said nothing about the
    /// network would leave "there was no boundary" and "the boundary was not
    /// surfaced" indistinguishable.
    pub network_boundary: ObtainedNetworkBoundary,
    /// When the publication committed.
    pub published_at_ms: i64,
}

/// Why a publication was refused because the repository changed under it.
///
/// Each variant is a distinct, retryable condition. A caller can decide
/// deliberately whether to re-stage and retry, and the receipt-free nature
/// of the refusal is honest: nothing was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitIndexPublicationConflict {
    /// Another writer holds the Git index lock.
    ///
    /// The publication did not touch the index and did not import any object.
    IndexLockHeld,
    /// The real index changed after the staging observed it.
    ///
    /// A concurrent writer's index is never overwritten.
    StaleIndex {
        /// The fingerprint the staging observed.
        expected: IndexFingerprint,
        /// The fingerprint found at publication time.
        observed: IndexFingerprint,
    },
    /// The repository's gitdir was replaced after the staging observed it.
    GitDirIdentityChanged {
        /// The device identity the staging observed.
        expected_dev: u64,
        /// The inode identity the staging observed.
        expected_ino: u64,
    },
    /// The staged artifact no longer matches what the journal recorded.
    StagedArtifactChanged {
        /// What did not match.
        reason: String,
    },
}

impl GitIndexPublicationConflict {
    /// A stable, human-readable name for the conflict.
    pub fn as_str(&self) -> &'static str {
        match self {
            GitIndexPublicationConflict::IndexLockHeld => "index_lock_held",
            GitIndexPublicationConflict::StaleIndex { .. } => "stale_index",
            GitIndexPublicationConflict::GitDirIdentityChanged { .. } => "gitdir_identity_changed",
            GitIndexPublicationConflict::StagedArtifactChanged { .. } => "staged_artifact_changed",
        }
    }
}

impl std::fmt::Display for GitIndexPublicationConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitIndexPublicationConflict::IndexLockHeld => {
                f.write_str("another writer holds the Git index lock")
            }
            GitIndexPublicationConflict::StaleIndex { expected, observed } => write!(
                f,
                "the repository's index changed (expected {expected}, observed {observed})"
            ),
            GitIndexPublicationConflict::GitDirIdentityChanged {
                expected_dev,
                expected_ino,
            } => write!(
                f,
                "the repository's Git directory was replaced (expected device {expected_dev}, \
                 inode {expected_ino})"
            ),
            GitIndexPublicationConflict::StagedArtifactChanged { reason } => {
                write!(f, "the staged artifact changed: {reason}")
            }
        }
    }
}

/// Why a publication was refused before it was attempted.
///
/// A refusal is not a conflict: nothing about the repository's state caused
/// it, so retrying the same request would refuse again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitIndexPublicationRefusal {
    /// The daemon has no publication backend configured.
    BackendNotConfigured {
        /// What is missing.
        reason: String,
    },
    /// This host cannot provide the containment the staging requires.
    HostUnsupported {
        /// Why the host cannot provide it.
        reason: String,
    },
    /// The repository's layout is outside this vertical's scope.
    RepositoryLayoutUnsupported {
        /// What about the layout is unsupported.
        reason: String,
    },
    /// The requested paths cannot be published as one index generation.
    PathsUnsupported {
        /// Why the paths are unsupported.
        reason: String,
    },
    /// The staging child failed.
    StagingFailed {
        /// What the staging child reported.
        reason: String,
    },
}

impl GitIndexPublicationRefusal {
    /// A stable, human-readable name for the refusal.
    pub fn as_str(&self) -> &'static str {
        match self {
            GitIndexPublicationRefusal::BackendNotConfigured { .. } => "backend_not_configured",
            GitIndexPublicationRefusal::HostUnsupported { .. } => "host_unsupported",
            GitIndexPublicationRefusal::RepositoryLayoutUnsupported { .. } => {
                "repository_layout_unsupported"
            }
            GitIndexPublicationRefusal::PathsUnsupported { .. } => "paths_unsupported",
            GitIndexPublicationRefusal::StagingFailed { .. } => "staging_failed",
        }
    }
}

impl std::fmt::Display for GitIndexPublicationRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitIndexPublicationRefusal::BackendNotConfigured { reason } => {
                write!(f, "the publication backend is not configured: {reason}")
            }
            GitIndexPublicationRefusal::HostUnsupported { reason } => {
                write!(f, "this host cannot publish an index: {reason}")
            }
            GitIndexPublicationRefusal::RepositoryLayoutUnsupported { reason } => {
                write!(f, "this repository layout is not supported: {reason}")
            }
            GitIndexPublicationRefusal::PathsUnsupported { reason } => {
                write!(f, "these paths cannot be published: {reason}")
            }
            GitIndexPublicationRefusal::StagingFailed { reason } => {
                write!(f, "the staging step failed: {reason}")
            }
        }
    }
}

/// The daemon-owned scratch root for private Git index staging.
///
/// The scratch must survive a daemon restart: the reconciler needs to find
/// the private index a crashed publication left behind, and its path is
/// recorded in the journal.
pub fn default_publication_scratch_root() -> PathBuf {
    codegg_client::LocalDaemonPaths::resolve()
        .root
        .join("git-publication")
}

/// Resolve the Git executable the publisher pins.
///
/// The publisher resolves the executable **once**, at construction, and uses
/// the resolved absolute path for every child. The repository therefore
/// cannot influence which executable the staging child runs: the path is
/// fixed before the child exists, and it is not a shell lookup at spawn
/// time.
///
/// Returns `None` when no executable `git` is on `PATH`, which leaves the
/// `Add` operation refused rather than silently unbacked.
pub fn resolve_git_binary() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("git");
        if !candidate.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let Ok(metadata) = std::fs::metadata(&candidate) else {
                continue;
            };
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return std::fs::canonicalize(&candidate).ok().or(Some(candidate));
    }
    None
}

/// What one startup reconciliation pass resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitIndexPublicationReconciliationReport {
    /// Publications that had committed before the daemon died.
    pub committed: usize,
    /// Publications that had not happened and will not be retried.
    pub aborted: usize,
    /// Publications whose outcome could not be determined. These are
    /// escalated, never reported as success.
    pub ambiguous: usize,
    /// Entries the pass could not resolve. They stay unresolved and are
    /// picked up by the next start.
    pub unresolved: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The receipt's boundary is read off the launch's own obtained fact.
    ///
    /// A launch that obtained a network namespace reports it; a launch that
    /// did not reports unrestricted; a launch that never ran reports that the
    /// system does not report a boundary. The receipt never claims a
    /// boundary the launch did not obtain.
    #[test]
    fn the_receipt_boundary_is_read_off_the_launch() {
        use crate::managed_process::SandboxExecutionOutcome;
        use codegg_core::approval::NetworkEnforcement;

        let enforced = SandboxExecutionOutcome::Enforced {
            backend: crate::security::sandbox::BackendId::BUBBLEWRAP,
            abi: None,
            network: NetworkEnforcement::Enforced {
                backend: "bubblewrap-netns".into(),
            },
            guarantees: vec![],
            limits: vec![],
        };
        assert_eq!(
            ObtainedNetworkBoundary::from_launch(&enforced),
            ObtainedNetworkBoundary::Enforced {
                backend: "bubblewrap-netns".into()
            }
        );

        // The anti-overclaim case: the launch obtained no network
        // namespace, so the receipt must not claim one.
        let unrestricted = SandboxExecutionOutcome::Enforced {
            backend: crate::security::sandbox::BackendId::BUBBLEWRAP,
            abi: None,
            network: NetworkEnforcement::Unrestricted,
            guarantees: vec![],
            limits: vec![],
        };
        assert_eq!(
            ObtainedNetworkBoundary::from_launch(&unrestricted),
            ObtainedNetworkBoundary::Unrestricted
        );

        // A launch that never ran has no obtained boundary to report.
        assert_eq!(
            ObtainedNetworkBoundary::from_launch(&SandboxExecutionOutcome::Disabled),
            ObtainedNetworkBoundary::NotReported
        );
    }

    /// Every boundary has a name, so a receipt states its boundary rather
    /// than leaving a caller to infer an omission.
    #[test]
    fn every_boundary_states_itself() {
        assert_eq!(
            ObtainedNetworkBoundary::NotReported.as_str(),
            "not_reported"
        );
        assert_eq!(
            ObtainedNetworkBoundary::Enforced {
                backend: "bubblewrap-netns".into()
            }
            .as_str(),
            "enforced"
        );
        assert_eq!(
            ObtainedNetworkBoundary::Unrestricted.as_str(),
            "unrestricted"
        );
    }
}

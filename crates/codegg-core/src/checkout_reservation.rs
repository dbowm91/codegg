//! M005 C004 — the authoritative checkout reservation check.
//!
//! The C003 writer fence mediates the **model's** native writers. It is a
//! run-scoped contract: a request names the run that holds the checkout
//! lease, and the fence authorizes that run to write. CodeGG has many
//! writers that are *not* model tools — the document service, the LSP
//! applier, project initialization, checkpoint restore, the HTTP file
//! routes — and those writers arrive without a run. They are exactly the
//! paths that could interleave with a shared checkout and corrupt it.
//!
//! This module is the authority that answers the question the fence cannot:
//! **is this checkout reserved right now, and if so, who may write it?**
//!
//! The answer is derived from durable state only:
//!
//! 1. The target path is resolved to a **canonical checkout identity** — the
//!    managed worktree whose canonical root contains it. A path alias (a
//!    symlinked parent, a trailing `..`) resolves to the same identity, and a
//!    linked worktree stays distinct from its parent repository.
//! 2. The worktree record names the **lease owner run**.
//! 3. The owner run belongs to a **live** (non-terminal) agent-run group.
//!
//! A checkout with a live group behind it is *reserved*: the group's members
//! are working in it together, and an outside writer must not interleave.
//! A checkout with no live group behind it is ordinary — API writes proceed
//! without any writer authority.
//!
//! The service deliberately takes **no locks and holds no permit**. It is a
//! read-only pre-effect check. That is what keeps it free of the lock-order
//! inversion the C003 closure record warned about: a non-broker writer that
//! already holds a document revision lease or the repository lock asks this
//! service a question, it never waits on the writer permit.
//!
//! Ownership: this crate owns the typed state machine. The root crate's
//! daemon adapter supplies the principal and decides what a refusal means
//! for the caller.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use super::agent_run_group::AgentRunGroupService;
use crate::identity::{AgentRunGroupId, AgentRunId, WorktreeId};
use crate::worktree_service::{WorktreeQuery, WorktreeRecord, WorktreeService};

/// The claim a live agent-run group holds over a checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutReservationClaim {
    /// The managed checkout the claim is over.
    pub worktree_id: WorktreeId,
    /// The live group that holds the checkout.
    pub group_id: AgentRunGroupId,
    /// The run that owns the checkout lease.
    pub owner_run_id: AgentRunId,
    /// Every run the live group admits. The owner is included when the
    /// group lists it as a member.
    pub member_run_ids: Vec<AgentRunId>,
}

/// Whether a checkout is reserved for a live agent-run group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutReservation {
    /// No live group reserves this checkout. An ordinary write may proceed
    /// without writer authority.
    Unreserved,
    /// A live group reserves this checkout for its members.
    Reserved(CheckoutReservationClaim),
}

impl CheckoutReservation {
    /// Whether a live group currently reserves this checkout.
    pub fn is_reserved(&self) -> bool {
        matches!(self, CheckoutReservation::Reserved(_))
    }

    /// The reservation claim, when the checkout is reserved.
    pub fn claim(&self) -> Option<&CheckoutReservationClaim> {
        match self {
            CheckoutReservation::Reserved(claim) => Some(claim),
            CheckoutReservation::Unreserved => None,
        }
    }

    /// Whether the given run may write this checkout under the reservation.
    ///
    /// The lease owner and every member of the live group may. Nobody else
    /// may — a human with a broader approval profile does not get to
    /// interleave with a shared checkout.
    pub fn authorizes(&self, run_id: Option<&AgentRunId>) -> bool {
        let Some(claim) = self.claim() else {
            // An unreserved checkout needs no writer authority.
            return true;
        };
        let Some(run_id) = run_id else {
            // No run: a human or an administrative path. An unreserved
            // checkout is still fine (handled above); a reserved one is not.
            return false;
        };
        if claim.owner_run_id == *run_id {
            return true;
        }
        claim.member_run_ids.iter().any(|member| member == run_id)
    }
}

/// Why the reservation lookup could not be answered.
///
/// A lookup failure is never silently treated as "unreserved": the daemon
/// adapter refuses the write when the authority cannot be consulted.
#[derive(Debug, thiserror::Error)]
pub enum CheckoutReservationError {
    /// The durable state backing the lookup could not be read.
    #[error("checkout reservation lookup failed: {0}")]
    Lookup(String),
}

/// The authoritative checkout reservation service.
///
/// The service answers read-only questions about durable state. It never
/// acquires a permit, never waits, and never mutates.
pub struct CheckoutReservationService {
    worktrees: Arc<WorktreeService>,
    groups: Option<Arc<AgentRunGroupService>>,
}

impl CheckoutReservationService {
    /// Build the service.
    ///
    /// `groups` is optional because a local-only daemon has no durable
    /// group store. Without it there are no live groups, so no checkout is
    /// ever reserved — which is the correct answer for a single-user
    /// daemon that never shares a checkout.
    pub fn new(
        worktrees: Arc<WorktreeService>,
        groups: Option<Arc<AgentRunGroupService>>,
    ) -> Arc<Self> {
        Arc::new(Self { worktrees, groups })
    }

    /// The reservation over the checkout that contains `path`.
    ///
    /// `path` may be the checkout root itself or any file inside it. The
    /// lookup resolves the canonical checkout identity first, so an alias
    /// path resolves to the same worktree as the canonical one.
    pub async fn reservation_for_path(
        &self,
        path: &Path,
    ) -> Result<CheckoutReservation, CheckoutReservationError> {
        let Some(record) = self.record_for_path(path).await? else {
            return Ok(CheckoutReservation::Unreserved);
        };
        self.reservation_for_record(&record).await
    }

    /// The reservation over a known managed checkout.
    pub async fn reservation_for_worktree(
        &self,
        worktree_id: &WorktreeId,
    ) -> Result<CheckoutReservation, CheckoutReservationError> {
        let record = self
            .worktrees
            .get(worktree_id)
            .await
            .map_err(|error| CheckoutReservationError::Lookup(error.to_string()))?;
        self.reservation_for_record(&record).await
    }

    /// The managed checkout that contains `path`, if any.
    ///
    /// The match is by **canonical containment**: the target resolves inside
    /// the worktree's canonical root. A path outside every managed checkout
    /// is not a managed checkout at all, so it is never reserved.
    pub async fn record_for_path(
        &self,
        path: &Path,
    ) -> Result<Option<WorktreeRecord>, CheckoutReservationError> {
        let target = canonical_identity(path);
        let records = self
            .worktrees
            .list(WorktreeQuery::default())
            .await
            .map_err(|error| CheckoutReservationError::Lookup(error.to_string()))?;
        let mut best: Option<(usize, WorktreeRecord)> = None;
        for record in records {
            let root = canonical_identity(&record.path);
            if !target.starts_with(&root) {
                continue;
            }
            let depth = root.components().count();
            // Nested checkouts: the deepest containing root wins, so a
            // linked worktree inside a managed root resolves to itself.
            if best
                .as_ref()
                .is_none_or(|(best_depth, _)| depth > *best_depth)
            {
                best = Some((depth, record));
            }
        }
        Ok(best.map(|(_, record)| record))
    }

    /// The reservation derived from a worktree record.
    async fn reservation_for_record(
        &self,
        record: &WorktreeRecord,
    ) -> Result<CheckoutReservation, CheckoutReservationError> {
        let Some(owner_run_id) = record.owner_run_id.clone() else {
            // An unleased checkout is not reserved.
            return Ok(CheckoutReservation::Unreserved);
        };
        let Some(groups) = self.groups.as_ref() else {
            return Ok(CheckoutReservation::Unreserved);
        };
        let owned = groups
            .groups_for_run(&owner_run_id)
            .await
            .map_err(|error| CheckoutReservationError::Lookup(error.to_string()))?;
        // The owner may hold the checkout for a group that is already
        // finished. Only a live group reserves the checkout.
        let Some(group) = owned.into_iter().find(|group| !group.status.is_terminal()) else {
            return Ok(CheckoutReservation::Unreserved);
        };
        let mut member_run_ids = group.member_run_ids.clone();
        if !member_run_ids.contains(&owner_run_id) {
            member_run_ids.push(owner_run_id.clone());
        }
        Ok(CheckoutReservation::Reserved(CheckoutReservationClaim {
            worktree_id: record.worktree_id.clone(),
            group_id: group.group_id.clone(),
            owner_run_id,
            member_run_ids,
        }))
    }
}

/// The canonical identity of a filesystem path.
///
/// The longest existing prefix is canonicalized and the not-yet-existing tail
/// is appended unchanged, so a path that names a file that does not exist
/// yet still resolves to the same identity as its parent. Symlinked parents
/// and redundant `..` components collapse, so two spellings of the same
/// checkout resolve to one identity.
pub fn canonical_identity(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let normalized = lexical_normalize(&absolute);
    canonicalize_existing_prefix(&normalized)
}

/// Fold redundant `.` and `..` components out of an absolute path.
///
/// This is lexical only: it never touches the filesystem, so a path whose
/// tail does not exist yet still normalizes.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let pops_a_normal = matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                );
                if pops_a_normal {
                    normalized.pop();
                } else {
                    normalized.push(component);
                }
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// Canonicalize the longest existing prefix of `path` and re-append the rest.
///
/// A file that does not exist yet still resolves to the identity of its
/// existing parent, so a write target and the checkout that contains it
/// agree even before the write lands.
fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canonical) = existing.canonicalize() {
            let mut resolved = canonical;
            for part in tail.iter().rev() {
                resolved.push(part);
            }
            return resolved;
        }
        let Some(name) = existing.file_name().map(std::ffi::OsString::from) else {
            // Nothing resolvable (the root itself does not exist): keep the
            // lexical form.
            return path.to_path_buf();
        };
        if !existing.pop() {
            return path.to_path_buf();
        }
        tail.push(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_identity_collapses_redundant_components() {
        let base = std::env::temp_dir();
        let a = canonical_identity(&base.join("a").join("..").join("a"));
        let b = canonical_identity(&base.join("a"));
        assert_eq!(a, b);
    }

    #[test]
    fn canonical_identity_keeps_a_missing_tail() {
        let base = std::env::temp_dir();
        let missing = base.join("codegg-c004-missing").join("file.txt");
        assert!(canonical_identity(&missing).ends_with("file.txt"));
    }

    #[test]
    fn an_unreserved_checkout_authorizes_anyone() {
        let reservation = CheckoutReservation::Unreserved;
        assert!(reservation.authorizes(None));
    }

    #[test]
    fn a_reserved_checkout_refuses_a_writer_without_a_run() {
        let claim = CheckoutReservationClaim {
            worktree_id: WorktreeId::parse("wt-1").expect("valid worktree id"),
            group_id: AgentRunGroupId::parse("grp-1").expect("valid group id"),
            owner_run_id: AgentRunId::parse("run-1").expect("valid run id"),
            member_run_ids: vec![AgentRunId::parse("run-1").expect("valid run id")],
        };
        let reservation = CheckoutReservation::Reserved(claim);
        assert!(reservation.is_reserved());
        assert!(!reservation.authorizes(None));
    }
}

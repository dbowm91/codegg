//! The C007 reconciler: resolve publications a prior daemon generation left
//! unresolved.
//!
//! The reconciler runs once at daemon start. It reads every journal entry
//! that is not terminal and decides, from the **recorded evidence alone**,
//! what happened to the publication.
//!
//! The evidence is:
//!
//! * the gitdir identity the publication was bound to;
//! * the fingerprint of the real index the staging observed;
//! * the fingerprint of the index that was staged.
//!
//! And the decision is:
//!
//! | the real index now | verdict |
//! |---|---|
//! | matches the **published** fingerprint | **committed** — the rename happened |
//! | matches the **expected** fingerprint | **aborted** — the rename never happened |
//! | matches neither | **ambiguous** — the outcome cannot be determined |
//!
//! The published fingerprint is checked first. When the staged index is
//! byte-identical to the index that was already there, the two fingerprints
//! coincide and the committed verdict is the accurate one: the repository's
//! index holds the published content either way.
//!
//! # Ambiguity is never success
//!
//! An ambiguous entry is **failed with a typed "unknown outcome" result** and
//! the private index is **retained** for inspection. It is not reported as a
//! success and it is not replayed. Guessing is not recovery.
//!
//! # Recovery is idempotent
//!
//! A committed publication is never re-applied: the reconciler closes the
//! entry, it does not publish again. An aborted entry is never retried. The
//! store refuses a transition out of a terminal state, so a second pass
//! cannot re-decide an entry that is already closed.

use std::fs;
use std::path::{Path, PathBuf};

use codegg_core::git_publication::{
    GitIndexPublicationRecord, GitIndexPublicationState, GitIndexPublicationStore,
};

use super::publisher::{fingerprint_file, fingerprint_index, path_identity, scratch_component};
use super::GitIndexPublicationReconciliationReport;

/// The bound on how many unresolved entries one startup pass reconciles.
///
/// A startup pass is a bounded sweep, not a poller. Anything beyond this
/// bound stays unresolved and is picked up by the next start.
pub const GIT_INDEX_PUBLICATION_MAX_RECONCILE: i64 = 256;

/// The verdict for one unresolved publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexPublicationReconciliation {
    /// The journal identity of the publication.
    pub intent_id: String,
    /// The state the entry was closed in.
    pub state: GitIndexPublicationState,
    /// Why, in one line.
    pub detail: String,
}

/// Resolve every publication a prior daemon generation left unresolved.
///
/// The pass is best-effort: a failure to resolve one entry never fails the
/// daemon's recovery, and the entry stays unresolved for the next start.
pub async fn reconcile_git_index_publications_at_startup(
    store: &GitIndexPublicationStore,
    scratch_root: &Path,
    limit: i64,
) -> GitIndexPublicationReconciliationReport {
    let mut report = GitIndexPublicationReconciliationReport::default();
    let records = match store.list_unresolved(limit).await {
        Ok(records) => records,
        Err(error) => {
            tracing::warn!(
                ?error,
                "git index publication reconciliation could not list entries"
            );
            return report;
        }
    };
    for record in records {
        match reconcile_one(store, scratch_root, &record).await {
            Ok(Some(reconciliation)) => {
                match reconciliation.state {
                    GitIndexPublicationState::Committed => report.committed += 1,
                    GitIndexPublicationState::Aborted => report.aborted += 1,
                    GitIndexPublicationState::Ambiguous => report.ambiguous += 1,
                    _ => report.unresolved += 1,
                }
                tracing::info!(
                    intent_id = %reconciliation.intent_id,
                    state = %reconciliation.state,
                    detail = %reconciliation.detail,
                    "git index publication reconciled"
                );
            }
            Ok(None) => report.unresolved += 1,
            Err(error) => {
                report.unresolved += 1;
                tracing::warn!(
                    intent_id = %record.intent.id,
                    ?error,
                    "git index publication reconciliation failed"
                );
            }
        }
    }
    report
}

/// Resolve one unresolved publication.
async fn reconcile_one(
    store: &GitIndexPublicationStore,
    scratch_root: &Path,
    record: &GitIndexPublicationRecord,
) -> Result<Option<GitIndexPublicationReconciliation>, String> {
    let intent = &record.intent;
    let gitdir = PathBuf::from(&intent.gitdir_path);

    // The repository is gone: no publication could have committed into it.
    if !gitdir.exists() {
        let state = GitIndexPublicationState::Aborted;
        close(store, &intent.id, state, "the repository no longer exists").await?;
        discard_scratch(scratch_root, &intent.id);
        return Ok(Some(GitIndexPublicationReconciliation {
            intent_id: intent.id.clone(),
            state,
            detail: "the repository no longer exists".into(),
        }));
    }

    // The gitdir was replaced: the path now names a different repository, so
    // nothing about the recorded evidence can be trusted.
    let Some((dev, ino)) = path_identity(&gitdir) else {
        let state = GitIndexPublicationState::Ambiguous;
        close(
            store,
            &intent.id,
            state,
            "the Git directory identity could not be read",
        )
        .await?;
        retain_scratch(scratch_root, &intent.id);
        return Ok(Some(GitIndexPublicationReconciliation {
            intent_id: intent.id.clone(),
            state,
            detail: "the Git directory identity could not be read".into(),
        }));
    };
    if dev != intent.gitdir_dev || ino != intent.gitdir_ino {
        let state = GitIndexPublicationState::Ambiguous;
        close(
            store,
            &intent.id,
            state,
            "the Git directory was replaced after the publication observed it",
        )
        .await?;
        retain_scratch(scratch_root, &intent.id);
        return Ok(Some(GitIndexPublicationReconciliation {
            intent_id: intent.id.clone(),
            state,
            detail: "the Git directory was replaced after the publication observed it".into(),
        }));
    }

    let observed = fingerprint_index(&gitdir).map_err(|error| error.to_string())?;
    let published = &intent.published_index_fingerprint;
    let expected = &intent.expected_index_fingerprint;

    if observed == *published {
        // The rename happened. Close the entry; never replay it.
        let state = GitIndexPublicationState::Committed;
        close(
            store,
            &intent.id,
            state,
            "the published index is the repository's index",
        )
        .await?;
        discard_scratch(scratch_root, &intent.id);
        return Ok(Some(GitIndexPublicationReconciliation {
            intent_id: intent.id.clone(),
            state,
            detail: "the published index is the repository's index".into(),
        }));
    }

    if observed == *expected {
        // The rename never happened. The publication is aborted, and the
        // private artifact is discarded.
        let state = GitIndexPublicationState::Aborted;
        remove_leftover_lock(&gitdir, published);
        close(
            store,
            &intent.id,
            state,
            "the repository's index is unchanged, so the publication never happened",
        )
        .await?;
        discard_scratch(scratch_root, &intent.id);
        return Ok(Some(GitIndexPublicationReconciliation {
            intent_id: intent.id.clone(),
            state,
            detail: "the repository's index is unchanged, so the publication never happened".into(),
        }));
    }

    // The index matches neither fingerprint: the outcome cannot be
    // determined. This is never reported as success.
    let state = GitIndexPublicationState::Ambiguous;
    close(
        store,
        &intent.id,
        state,
        "the repository's index matches neither the expected nor the published fingerprint",
    )
    .await?;
    retain_scratch(scratch_root, &intent.id);
    Ok(Some(GitIndexPublicationReconciliation {
        intent_id: intent.id.clone(),
        state,
        detail: "the repository's index matches neither the expected nor the published fingerprint"
            .into(),
    }))
}

/// Close a journal entry in a terminal state.
async fn close(
    store: &GitIndexPublicationStore,
    intent_id: &str,
    state: GitIndexPublicationState,
    detail: &str,
) -> Result<(), String> {
    store
        .mark(intent_id, state, Some(detail), super::publisher::now_ms())
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Remove a leftover `index.lock` that this publication provably created.
///
/// A lock is only removed when its content is the **published** index, which
/// is this publication's own artifact. A lock that belongs to another writer
/// is never touched, even when it is empty.
fn remove_leftover_lock(gitdir: &Path, published: &codegg_core::git_publication::IndexFingerprint) {
    let lock = gitdir.join("index.lock");
    if !lock.exists() {
        return;
    }
    if let Ok(Some(digest)) = fingerprint_file(&lock) {
        if digest == published.as_str() {
            let _ = fs::remove_file(&lock);
        }
    }
}

/// Discard the private artifact of a resolved publication.
fn discard_scratch(scratch_root: &Path, intent_id: &str) {
    let Ok(component) = scratch_component(intent_id) else {
        return;
    };
    let _ = fs::remove_dir_all(scratch_root.join(component));
}

/// Retain the private artifact of an ambiguous publication, for inspection.
fn retain_scratch(scratch_root: &Path, intent_id: &str) {
    let Ok(component) = scratch_component(intent_id) else {
        return;
    };
    let path = scratch_root.join(component);
    if path.exists() {
        tracing::warn!(
            intent_id,
            path = %path.display(),
            "an ambiguous git index publication retained its private index for inspection"
        );
    }
}

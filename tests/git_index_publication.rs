//! C007: the private index publication vertical.
//!
//! These tests cover the parts of the vertical that do **not** need the
//! M004/M005 containment tier: the durable journal, the reconciler, and the
//! publication half — the compare-and-swap, the Git index lock, the object
//! import, and the atomic replacement.
//!
//! The staging step is the one part that needs the tier, so it is injected:
//! the test provides its own implementation of
//! [`codegg::git_publication::GitIndexStaging`] that runs the same `git add`
//! against the same private gitdir. Everything above the staging boundary —
//! the journal, the compare-and-swap, the lock, the atomic replacement, the
//! crash recovery — is the production code in both cases.
//!
//! The contained staging step itself is proven by the hosted Linux runner in
//! `tests/linux_m004_process_ipc.rs`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use codegg::git_publication::{
    reconcile_git_index_publications_at_startup, GitIndexPublicationConflict,
    GitIndexPublicationOutcome, GitIndexPublicationRefusal, GitIndexPublicationRequest,
    GitIndexPublisher, GitIndexStaging, GitIndexStagingRequest, ObtainedNetworkBoundary,
};
use codegg_core::git_publication::{
    GitIndexPublicationIntent, GitIndexPublicationState, GitIndexPublicationStore, IndexFingerprint,
};
use common::pool::isolated_pool;
use egggit::GitReadConsistency;
use sqlx::SqlitePool;
use tempfile::TempDir;

// ── Helpers ────────────────────────────────────────────────────────

fn git_binary() -> PathBuf {
    codegg::git_publication::resolve_git_binary().expect("git is available on PATH")
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(git_binary())
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A repository with one commit, so the real index exists.
fn init_repo(dir: &Path) {
    run_git(dir, &["init", "--quiet", "--initial-branch=main"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
    std::fs::write(dir.join("tracked.txt"), "original\n").unwrap();
    run_git(dir, &["add", "tracked.txt"]);
    run_git(dir, &["commit", "--quiet", "-m", "init"]);
}

fn repo_gitdir(repo: &Path) -> PathBuf {
    repo.join(".git")
}

/// The staging step used by these tests.
///
/// It runs the same `git add` the contained staging step runs, against the
/// same private gitdir, without the containment tier.
#[derive(Default)]
struct LocalGitStaging;

#[async_trait]
impl GitIndexStaging for LocalGitStaging {
    async fn stage(
        &self,
        request: &GitIndexStagingRequest,
    ) -> Result<ObtainedNetworkBoundary, String> {
        let mut args = vec![
            format!("--git-dir={}", request.private_gitdir.display()),
            format!("--work-tree={}", request.repository_root.display()),
            "--literal-pathspecs".to_string(),
            "add".to_string(),
            "--".to_string(),
        ];
        args.extend(request.paths.iter().cloned());
        let output = Command::new(git_binary())
            .args(&args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "git add exited with {}: {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        // This staging runs the Git child directly, without the containment
        // tier, so there is no launch whose obtained network boundary could
        // be read. `NotReported` is the truthful answer — it is a statement,
        // not an omission.
        Ok(ObtainedNetworkBoundary::NotReported)
    }
}

fn publisher(pool: &SqlitePool, scratch: &Path, staging: LocalGitStaging) -> GitIndexPublisher {
    let store = Arc::new(GitIndexPublicationStore::new(pool.clone()));
    GitIndexPublisher::new(
        store,
        scratch.to_path_buf(),
        Arc::new(staging),
        Duration::from_secs(30),
    )
}

fn request(repo: &Path, paths: &[&str], intent_id: &str) -> GitIndexPublicationRequest {
    GitIndexPublicationRequest {
        intent_id: intent_id.to_string(),
        repository_root: repo.to_path_buf(),
        workspace_root: repo.to_path_buf(),
        paths: paths.iter().map(|p| (*p).to_string()).collect(),
    }
}

/// The bytes of a file, or `None` when it does not exist.
fn read_bytes(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// The loose object identities in an object database.
fn loose_objects(objects: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(objects) else {
        return found;
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let fanout = entry.file_name().to_string_lossy().into_owned();
        if fanout.len() != 2 || !fanout.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        for object in std::fs::read_dir(entry.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = object.file_name().to_string_lossy().into_owned();
            if name.len() == 38 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
                found.push(format!("{fanout}{name}"));
            }
        }
    }
    found.sort();
    found
}

// ── The durable journal ─────────────────────────────────────────────

/// The device/inode identity of a directory.
///
/// A path that does not exist has no identity; the journal tests use one, and
/// only the reconciler tests care about the value.
fn gitdir_identity(gitdir: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(gitdir) {
        Ok(metadata) => (metadata.dev(), metadata.ino()),
        Err(_) => (0, 0),
    }
}

fn intent(id: &str, gitdir: &Path) -> GitIndexPublicationIntent {
    let (dev, ino) = gitdir_identity(gitdir);
    GitIndexPublicationIntent {
        id: id.to_string(),
        workspace_root: "/workspace".to_string(),
        gitdir_path: gitdir.to_string_lossy().into_owned(),
        gitdir_dev: dev,
        gitdir_ino: ino,
        private_index_path: "/scratch/index".to_string(),
        expected_index_fingerprint: IndexFingerprint::Absent,
        published_index_fingerprint: IndexFingerprint::Digest("a".repeat(64)),
        staged_paths: vec!["tracked.txt".to_string()],
        created_at_ms: 1_000,
    }
}

#[tokio::test]
async fn journal_records_reads_and_lists_an_unresolved_intent() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool);
    store
        .record_intent(&intent("intent-1", Path::new("/repo/.git")))
        .await
        .expect("record");

    let record = store.get("intent-1").await.expect("get").expect("present");
    assert_eq!(record.state, GitIndexPublicationState::Prepared);
    assert_eq!(record.intent.staged_paths, vec!["tracked.txt".to_string()]);
    assert_eq!(
        record.intent.expected_index_fingerprint,
        IndexFingerprint::Absent
    );

    let unresolved = store.list_unresolved(16).await.expect("list");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].intent.id, "intent-1");
}

#[tokio::test]
async fn journal_refuses_a_duplicate_intent() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool);
    store
        .record_intent(&intent("intent-1", Path::new("/repo/.git")))
        .await
        .expect("record");
    let error = store
        .record_intent(&intent("intent-1", Path::new("/repo/.git")))
        .await
        .expect_err("a duplicate identity is refused");
    assert!(
        matches!(
            error,
            codegg_core::git_publication::GitIndexPublicationStoreError::Duplicate(_)
        ),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn journal_lists_only_unresolved_intents() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool);
    store
        .record_intent(&intent("open", Path::new("/repo/.git")))
        .await
        .expect("record");
    store
        .record_intent(&intent("closed", Path::new("/repo/.git")))
        .await
        .expect("record");
    store
        .mark("closed", GitIndexPublicationState::Committed, None, 2_000)
        .await
        .expect("mark");

    let unresolved = store.list_unresolved(16).await.expect("list");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].intent.id, "open");
}

#[tokio::test]
async fn journal_refuses_a_transition_out_of_a_terminal_state() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool);
    store
        .record_intent(&intent("closed", Path::new("/repo/.git")))
        .await
        .expect("record");
    store
        .mark("closed", GitIndexPublicationState::Committed, None, 2_000)
        .await
        .expect("mark");

    let error = store
        .mark("closed", GitIndexPublicationState::Aborted, None, 3_000)
        .await
        .expect_err("a terminal state never transitions");
    assert!(
        matches!(
            error,
            codegg_core::git_publication::GitIndexPublicationStoreError::IllegalTransition { .. }
        ),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn journal_refuses_publishing_back_to_prepared() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool);
    store
        .record_intent(&intent("moving", Path::new("/repo/.git")))
        .await
        .expect("record");
    store
        .mark("moving", GitIndexPublicationState::Publishing, None, 2_000)
        .await
        .expect("mark");
    let error = store
        .mark("moving", GitIndexPublicationState::Prepared, None, 3_000)
        .await
        .expect_err("publishing never returns to prepared");
    assert!(
        matches!(
            error,
            codegg_core::git_publication::GitIndexPublicationStoreError::IllegalTransition { .. }
        ),
        "unexpected error: {error}"
    );
}

// ── The reconciler ──────────────────────────────────────────────────

/// A repository whose real index holds `index_bytes`.
fn repo_with_index(dir: &Path, index_bytes: &[u8]) -> PathBuf {
    init_repo(dir);
    let gitdir = repo_gitdir(dir);
    std::fs::write(gitdir.join("index"), index_bytes).unwrap();
    gitdir
}

#[tokio::test]
async fn reconciler_aborts_an_unchanged_index_and_discards_the_private_artifact() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool.clone());
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let gitdir = repo_with_index(&repo, b"index-generation-one");

    let mut recorded = intent("crash-before", &gitdir);
    recorded.expected_index_fingerprint = IndexFingerprint::Digest(
        codegg::git_publication::publisher::sha256_hex(b"index-generation-one"),
    );
    recorded.published_index_fingerprint = IndexFingerprint::Digest("e".repeat(64));
    recorded.private_index_path = scratch
        .join("crash-before")
        .join("index")
        .display()
        .to_string();
    std::fs::create_dir_all(scratch.join("crash-before")).unwrap();
    std::fs::write(scratch.join("crash-before").join("index"), b"private").unwrap();
    store.record_intent(&recorded).await.expect("record");

    let report = reconcile_git_index_publications_at_startup(&store, &scratch, 16).await;
    assert_eq!(report.aborted, 1);
    assert_eq!(report.committed, 0);
    assert_eq!(report.ambiguous, 0);

    let record = store.get("crash-before").await.unwrap().unwrap();
    assert_eq!(record.state, GitIndexPublicationState::Aborted);
    // The private artifact of an aborted publication is discarded.
    assert!(!scratch.join("crash-before").exists());
}

#[tokio::test]
async fn reconciler_closes_a_committed_publication() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool.clone());
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let published_bytes = b"the-published-index";
    let gitdir = repo_with_index(&repo, published_bytes);

    let mut recorded = intent("crash-after-move", &gitdir);
    recorded.expected_index_fingerprint = IndexFingerprint::Digest("f".repeat(64));
    recorded.published_index_fingerprint = IndexFingerprint::Digest(
        codegg::git_publication::publisher::sha256_hex(published_bytes),
    );
    store.record_intent(&recorded).await.expect("record");

    let report = reconcile_git_index_publications_at_startup(&store, &scratch, 16).await;
    assert_eq!(report.committed, 1);
    assert_eq!(report.aborted, 0);
    assert_eq!(report.ambiguous, 0);

    let record = store.get("crash-after-move").await.unwrap().unwrap();
    assert_eq!(record.state, GitIndexPublicationState::Committed);
}

#[tokio::test]
async fn reconciler_escalates_an_ambiguous_publication_and_retains_the_private_artifact() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool.clone());
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let gitdir = repo_with_index(&repo, b"somebody-elses-index");

    let mut recorded = intent("crash-ambiguous", &gitdir);
    recorded.expected_index_fingerprint = IndexFingerprint::Digest("f".repeat(64));
    recorded.published_index_fingerprint = IndexFingerprint::Digest("e".repeat(64));
    recorded.private_index_path = scratch
        .join("crash-ambiguous")
        .join("index")
        .display()
        .to_string();
    std::fs::create_dir_all(scratch.join("crash-ambiguous")).unwrap();
    std::fs::write(scratch.join("crash-ambiguous").join("index"), b"private").unwrap();
    store.record_intent(&recorded).await.expect("record");

    let report = reconcile_git_index_publications_at_startup(&store, &scratch, 16).await;
    assert_eq!(report.ambiguous, 1);
    assert_eq!(report.committed, 0);
    assert_eq!(report.aborted, 0);

    let record = store.get("crash-ambiguous").await.unwrap().unwrap();
    assert_eq!(record.state, GitIndexPublicationState::Ambiguous);
    // An ambiguous publication retains its private index for inspection.
    assert!(scratch.join("crash-ambiguous").join("index").exists());
}

#[tokio::test]
async fn reconciler_removes_only_a_leftover_lock_it_can_attribute() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool.clone());
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let published_bytes = b"the-published-index";
    let gitdir = repo_with_index(&repo, b"unchanged-index");

    let mut recorded = intent("leftover-lock", &gitdir);
    recorded.expected_index_fingerprint = IndexFingerprint::Digest(
        codegg::git_publication::publisher::sha256_hex(b"unchanged-index"),
    );
    recorded.published_index_fingerprint = IndexFingerprint::Digest(
        codegg::git_publication::publisher::sha256_hex(published_bytes),
    );
    store.record_intent(&recorded).await.expect("record");

    // A lock that holds the published index is this publication's own and is
    // removed.
    std::fs::write(gitdir.join("index.lock"), published_bytes).unwrap();
    // A lock that holds something else belongs to another writer and is left
    // alone.
    let foreign = temp.path().join("foreign.lock");
    std::fs::write(&foreign, b"another writer").unwrap();

    let report = reconcile_git_index_publications_at_startup(&store, &scratch, 16).await;
    assert_eq!(report.aborted, 1);
    assert!(!gitdir.join("index.lock").exists());
    assert!(foreign.exists());
}

#[tokio::test]
async fn reconciliation_is_idempotent_across_restarts() {
    let pool = isolated_pool().await;
    let store = GitIndexPublicationStore::new(pool.clone());
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let gitdir = repo_with_index(&repo, b"unchanged-index");

    let mut recorded = intent("repeat", &gitdir);
    recorded.expected_index_fingerprint = IndexFingerprint::Digest(
        codegg::git_publication::publisher::sha256_hex(b"unchanged-index"),
    );
    recorded.published_index_fingerprint = IndexFingerprint::Digest("e".repeat(64));
    store.record_intent(&recorded).await.expect("record");

    let first = reconcile_git_index_publications_at_startup(&store, &scratch, 16).await;
    assert_eq!(first.aborted, 1);
    // A second pass finds nothing unresolved and changes nothing.
    let second = reconcile_git_index_publications_at_startup(&store, &scratch, 16).await;
    assert_eq!(second.aborted, 0);
    assert_eq!(second.committed, 0);
    assert_eq!(second.ambiguous, 0);
    assert_eq!(second.unresolved, 0);
    let record = store.get("repeat").await.unwrap().unwrap();
    assert_eq!(record.state, GitIndexPublicationState::Aborted);
}

// ── The publication ─────────────────────────────────────────────────

#[tokio::test]
async fn publication_replaces_the_index_and_imports_the_object() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let gitdir = repo_gitdir(&repo);

    let before_index = read_bytes(&gitdir.join("index")).expect("index exists");
    let before_objects = loose_objects(&gitdir.join("objects"));

    std::fs::write(repo.join("tracked.txt"), "changed\n").unwrap();
    let publisher = publisher(&pool, &scratch, LocalGitStaging);
    let outcome = publisher
        .publish_add(&request(&repo, &["tracked.txt"], "publish-1"))
        .await
        .expect("publish");

    let receipt = match outcome {
        GitIndexPublicationOutcome::Committed(receipt) => receipt,
        other => panic!("expected a committed publication, got {other:?}"),
    };
    assert_eq!(receipt.scope.as_str(), "index");
    assert_eq!(receipt.staged_paths, vec!["tracked.txt".to_string()]);
    assert_eq!(receipt.imported_objects.len(), 1);
    // The receipt states the obtained network boundary explicitly. The system
    // does not report the fact it obtained, so the truthful answer is "not
    // reported" — and it has to be *stated*, because a receipt that said
    // nothing about the network would leave "there was no boundary" and "the
    // boundary was not surfaced" indistinguishable.
    assert_eq!(
        receipt.network_boundary,
        ObtainedNetworkBoundary::NotReported
    );
    assert_eq!(receipt.network_boundary.as_str(), "not_reported");

    // The repository's index now holds the published content.
    let after_index = read_bytes(&gitdir.join("index")).expect("index exists");
    assert_ne!(after_index, before_index);
    // The new blob reached the repository's object database.
    let after_objects = loose_objects(&gitdir.join("objects"));
    assert_eq!(after_objects.len(), before_objects.len() + 1);
    assert!(after_objects.contains(&receipt.imported_objects[0]));
    // The staged change is now staged.
    let status = run_git(&repo, &["status", "--porcelain"]);
    assert!(
        status.starts_with("M  tracked.txt"),
        "the change should be staged, got {status:?}"
    );
}

#[tokio::test]
async fn a_failed_staging_leaves_the_repository_byte_identical() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let gitdir = repo_gitdir(&repo);

    std::fs::write(repo.join("tracked.txt"), "changed\n").unwrap();
    let before_index = read_bytes(&gitdir.join("index")).expect("index exists");
    let before_objects = loose_objects(&gitdir.join("objects"));

    // A staging step that fails leaves the repository untouched.
    struct FailingStaging;
    #[async_trait]
    impl GitIndexStaging for FailingStaging {
        async fn stage(
            &self,
            _request: &GitIndexStagingRequest,
        ) -> Result<ObtainedNetworkBoundary, String> {
            Err("the staging child failed".into())
        }
    }
    let store = Arc::new(GitIndexPublicationStore::new(pool.clone()));
    let publisher = GitIndexPublisher::new(
        store,
        scratch.clone(),
        Arc::new(FailingStaging),
        Duration::from_secs(30),
    );
    let outcome = publisher
        .publish_add(&request(&repo, &["tracked.txt"], "failed-staging"))
        .await
        .expect("publish");
    assert!(
        matches!(outcome, GitIndexPublicationOutcome::Refused(_)),
        "a failed staging is a refusal, got {outcome:?}"
    );

    assert_eq!(
        read_bytes(&gitdir.join("index")),
        Some(before_index.clone())
    );
    assert_eq!(loose_objects(&gitdir.join("objects")), before_objects);
    // Nothing was recorded: a failed staging never reaches the journal.
    let store = GitIndexPublicationStore::new(pool);
    assert!(store.get("failed-staging").await.unwrap().is_none());
}

#[tokio::test]
async fn a_concurrent_external_writer_produces_a_typed_conflict_and_is_not_overwritten() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let gitdir = repo_gitdir(&repo);

    std::fs::write(repo.join("tracked.txt"), "changed\n").unwrap();

    let publisher = publisher(&pool, &scratch, LocalGitStaging);
    let publication = request(&repo, &["tracked.txt"], "concurrent");
    // Stage first, then let a concurrent external writer replace the index,
    // then publish. This is the exact window the compare-and-swap exists for.
    let prepared = publisher
        .stage_private_index(&publication)
        .await
        .expect("stage");
    std::fs::write(repo.join("other.txt"), "other\n").unwrap();
    run_git(&repo, &["add", "other.txt"]);
    let external_index = read_bytes(&gitdir.join("index")).expect("the external index exists");
    let outcome = publisher
        .publish_prepared_index(&publication, &prepared)
        .await
        .expect("publish");

    match outcome {
        GitIndexPublicationOutcome::Conflict(conflict) => {
            assert!(
                matches!(conflict, GitIndexPublicationConflict::StaleIndex { .. }),
                "expected a stale-index conflict, got {conflict:?}"
            );
        }
        other => panic!("expected a typed conflict, got {other:?}"),
    }
    // The concurrent writer's index is not overwritten.
    assert_eq!(read_bytes(&gitdir.join("index")), Some(external_index));
    // The publication is not applied: no lock is left behind, the publisher's
    // change is still unstaged, and the external writer's change is still
    // staged.
    assert!(!gitdir.join("index.lock").exists());
    let status = run_git(&repo, &["status", "--porcelain"]);
    assert!(
        status.contains(" M tracked.txt"),
        "the publisher's change should still be unstaged, got {status:?}"
    );
    assert!(
        status.contains("A  other.txt"),
        "the external writer's change should still be staged, got {status:?}"
    );
}

#[tokio::test]
async fn a_held_index_lock_produces_a_typed_conflict() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let gitdir = repo_gitdir(&repo);

    std::fs::write(repo.join("tracked.txt"), "changed\n").unwrap();
    // A concurrent official Git process holds the lock.
    std::fs::write(gitdir.join("index.lock"), b"another writer holds this").unwrap();

    let publisher = publisher(&pool, &scratch, LocalGitStaging);
    let outcome = publisher
        .publish_add(&request(&repo, &["tracked.txt"], "locked"))
        .await
        .expect("publish");

    match outcome {
        GitIndexPublicationOutcome::Conflict(conflict) => {
            assert_eq!(conflict, GitIndexPublicationConflict::IndexLockHeld);
        }
        other => panic!("expected a typed conflict, got {other:?}"),
    }
    // The other writer's lock is untouched.
    assert!(gitdir.join("index.lock").exists());
}

#[tokio::test]
async fn an_unconfigured_backend_refuses_the_add_mutation() {
    // The `Add` arm must be refused rather than silently downgraded to the
    // legacy in-place route when no publication backend is configured.
    //
    // The refusal is **typed**, and which typed refusal it is depends on the
    // host: the `Add` arm runs the same preflight every other mutation runs,
    // and `git add` requires the contained-execution tier, so on a host
    // without that backend the preflight refuses first with
    // `Policy(BackendUnavailable)`, while where the preflight passes the
    // missing publisher refuses with `PublicationFailed`. Both are
    // refusals. What must never happen is a silent in-place `git add`.
    let executor = codegg::git_mutations::GitMutationExecutor::new();
    let repo = TempDir::new().unwrap();
    init_repo(repo.path());
    // A change that an in-place `git add` would stage, so the check below
    // can tell "refused" from "silently staged".
    std::fs::write(repo.path().join("tracked.txt"), "changed\n").unwrap();
    let gitdir = repo_gitdir(repo.path());
    let index_before = read_bytes(&gitdir.join("index")).expect("index exists");
    let root = codegg_git::path::RepoRoot::new(repo.path()).expect("repo root");
    let path = codegg_git::path::RepoPath::new(&root, "tracked.txt").expect("repo path");
    let operation = codegg_git::GitOperation::Add { paths: vec![path] };
    let error = executor
        .execute(&operation, repo.path())
        .await
        .expect_err("an unconfigured backend refuses the add");
    assert!(
        matches!(
            error,
            codegg::git_mutations::GitMutationError::Policy(_)
                | codegg::git_mutations::GitMutationError::PublicationFailed(_)
        ),
        "unexpected error: {error}"
    );
    // Nothing was staged: the refusal is not a downgrade to the in-place
    // route.
    let index_after = read_bytes(&gitdir.join("index")).expect("index exists");
    assert_eq!(
        index_after, index_before,
        "a refused Add must not stage anything"
    );
}

#[tokio::test]
async fn a_path_outside_the_repository_is_refused_before_any_child_runs() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    let publisher = publisher(&pool, &scratch, LocalGitStaging);
    let outcome = publisher
        .publish_add(&request(&repo, &["../escape.txt"], "escape"))
        .await
        .expect("publish");
    assert!(
        matches!(outcome, GitIndexPublicationOutcome::Refused(_)),
        "an escaping path is refused, got {outcome:?}"
    );
    let store = GitIndexPublicationStore::new(pool);
    assert!(store.get("escape").await.unwrap().is_none());
}

/// C007: the dispatch path publishes an `Add` and nothing else.
///
/// The publisher's preflight is the same one the test-only path used, and it
/// serves `Add` only. A non-`Add` mutation is never handed to the publisher:
/// it keeps the legacy route, and the journal records no publication intent
/// for it. That is what makes the publisher's own refusal the decisive one —
/// a caller cannot route around the preflight by choosing a different
/// mutation, because the publisher never sees one.
#[tokio::test]
async fn a_non_add_mutation_is_never_published() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    let publisher = Arc::new(publisher(&pool, &scratch, LocalGitStaging));
    let executor = codegg::git_mutations::GitMutationExecutor::new()
        .with_env_policy(codegg::git_mutations::GitEnvPolicy::default())
        .with_publication(publisher);

    let operation = codegg_git::GitOperation::BranchCreate {
        name: codegg_git::BranchName::new("side").expect("branch name"),
        start_point: None,
        force: false,
    };
    let result = executor
        .execute(&operation, &repo)
        .await
        .expect("the legacy route still serves a non-Add mutation");
    assert!(result.success, "the legacy route failed: {result:?}");

    // The publisher was never asked to publish it: the journal is empty.
    let store = GitIndexPublicationStore::new(pool);
    assert!(
        store.list_unresolved(16).await.unwrap().is_empty(),
        "a non-Add mutation must not reach the publication journal"
    );
    // And the mutation did what it was asked to do, through the legacy route.
    let branches = run_git(&repo, &["branch", "--list", "side"]);
    assert!(
        branches.contains("side"),
        "the branch was not created: {branches:?}"
    );
}

// ── The C007 publication invariants ─────────────────────────────────
//
// The closure record claims four invariants of the publication vertical.
// These tests are the regression cover for them:
//
// 1. the real `Add` dispatch runs its **preflight** before the publisher is
//    consulted;
// 2. the publisher's scratch root is scoped **outside** the repository;
// 3. the publisher never invokes a repository-declared clean/smudge filter;
// 4. a live-class prepared index is refused **before** any mutation.

/// The bytes of the repository's refs, as a byte-identity witness.
///
/// A publication publishes the index. It must not touch a ref, so this
/// reads every ref file plus `HEAD` and `packed-refs`: a change to any of
/// them shows up as a difference.
fn refs_bytes(gitdir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, found);
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            found.push((relative, bytes));
        }
    }

    let mut found = Vec::new();
    let refs = gitdir.join("refs");
    walk(&refs, &refs, &mut found);
    for name in ["HEAD", "packed-refs"] {
        if let Ok(bytes) = std::fs::read(gitdir.join(name)) {
            found.push((PathBuf::from(name), bytes));
        }
    }
    found.sort();
    found
}

/// The repository-relative paths that are staged right now.
fn staged_paths(repo: &Path) -> Vec<String> {
    run_git(repo, &["diff", "--cached", "--name-only"])
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// C007: the real `Add` dispatch runs its preflight before the publisher is
/// consulted.
///
/// The `Add` arm's preflight is the execution-policy classification of the
/// rendered argv (`git add …`). `git add` may run a clean/smudge filter, so
/// the policy classifies it as requiring the contained-execution tier; the
/// policy this crate carries cannot provide that tier, so the preflight
/// refuses with the typed `BackendUnavailable` outcome.
///
/// That refusal is what makes the claim checkable: a publisher that was
/// configured and ready still saw nothing, because the preflight refused
/// **first**. The repository's index and refs are byte-identical afterwards
/// and the publication journal holds no row.
#[tokio::test]
async fn the_real_add_dispatch_preflight_refuses_before_the_publisher_is_consulted() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let gitdir = repo_gitdir(&repo);

    // A change an in-place `git add` would stage, so "refused" is
    // distinguishable from "silently staged".
    std::fs::write(repo.join("tracked.txt"), "changed\n").unwrap();
    let index_before = read_bytes(&gitdir.join("index")).expect("index exists");
    let refs_before = refs_bytes(&gitdir);

    // A publisher that is configured and would publish if it were reached.
    let publisher = Arc::new(publisher(&pool, &scratch, LocalGitStaging));
    let executor = codegg::git_mutations::GitMutationExecutor::new()
        .with_env_policy(codegg::git_mutations::GitEnvPolicy::default())
        .with_publication(publisher);

    let root = codegg_git::path::RepoRoot::new(&repo).expect("repo root");
    let path = codegg_git::path::RepoPath::new(&root, "tracked.txt").expect("repo path");
    let operation = codegg_git::GitOperation::Add { paths: vec![path] };

    let error = executor
        .execute(&operation, &repo)
        .await
        .expect_err("the preflight refuses the add");
    match &error {
        codegg::git_mutations::GitMutationError::Policy(outcome) => {
            assert!(
                matches!(
                    outcome,
                    egggit::GitExecutionPolicyOutcome::BackendUnavailable { capabilities }
                        if !capabilities.is_empty()
                ),
                "expected the typed backend refusal, got {outcome:?}"
            );
        }
        other => panic!("expected the preflight's policy refusal, got {other:?}"),
    }

    // The repository is byte-identical: the preflight refused before any
    // child ran and before the publisher was consulted.
    assert_eq!(
        read_bytes(&gitdir.join("index")),
        Some(index_before.clone()),
        "a refused Add must not touch the index"
    );
    assert_eq!(
        refs_bytes(&gitdir),
        refs_before,
        "a refused Add must not touch a ref"
    );
    assert!(
        staged_paths(&repo).is_empty(),
        "a refused Add must not stage anything"
    );

    // The publisher was never consulted: the journal holds no row.
    let store = GitIndexPublicationStore::new(pool);
    assert!(
        store.list_unresolved(16).await.unwrap().is_empty(),
        "the preflight must refuse before the publisher records an intent"
    );

    // The refusal belongs to the `Add` arm's preflight and not to the
    // executor: the same executor, with the same publisher, still serves a
    // non-`Add` mutation through the legacy route.
    let operation = codegg_git::GitOperation::BranchCreate {
        name: codegg_git::BranchName::new("side").expect("branch name"),
        start_point: None,
        force: false,
    };
    let result = executor
        .execute(&operation, &repo)
        .await
        .expect("the legacy route still serves a non-Add mutation");
    assert!(result.success, "the legacy route failed: {result:?}");
}

/// C007: the publisher's scratch is scoped **outside** the repository.
///
/// A scratch root inside the repository would put the private index — and
/// every other staging artifact — inside the very tree the publication is
/// about to change, so a failed publication could leave the user's worktree
/// littered with daemon-owned state. The publisher refuses such a scratch,
/// and it refuses before the repository is touched.
#[tokio::test]
async fn a_scratch_root_inside_the_repository_is_refused_before_the_repository_changes() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let gitdir = repo_gitdir(&repo);

    // The scratch root is *inside* the repository.
    let scratch = repo.join("publisher-scratch");

    std::fs::write(repo.join("tracked.txt"), "changed\n").unwrap();
    let index_before = read_bytes(&gitdir.join("index")).expect("index exists");
    let refs_before = refs_bytes(&gitdir);

    let publisher = publisher(&pool, &scratch, LocalGitStaging);
    let outcome = publisher
        .publish_add(&request(&repo, &["tracked.txt"], "scratch-inside"))
        .await
        .expect("publish");

    match outcome {
        GitIndexPublicationOutcome::Refused(GitIndexPublicationRefusal::BackendNotConfigured {
            reason,
        }) => assert!(
            reason.contains("outside the repository"),
            "unexpected refusal reason: {reason}"
        ),
        other => panic!("expected the scoped-scratch refusal, got {other:?}"),
    }

    // The repository is byte-identical: the refusal came before the
    // staging and before the journal.
    assert_eq!(
        read_bytes(&gitdir.join("index")),
        Some(index_before.clone()),
        "a refused publication must not touch the index"
    );
    assert_eq!(
        refs_bytes(&gitdir),
        refs_before,
        "a refused publication must not touch a ref"
    );
    assert!(
        staged_paths(&repo).is_empty(),
        "a refused publication must not stage anything"
    );

    // Nothing was recorded: the refusal precedes the journal.
    let store = GitIndexPublicationStore::new(pool);
    assert!(store.get("scratch-inside").await.unwrap().is_none());
}

/// A hostile clean/smudge filter command.
///
/// It records that it ran — by creating the sentinel — and rewrites the
/// content, so a publication that *applied* the filter would publish
/// different bytes than the worktree holds.
fn filter_script(sentinel: &Path) -> String {
    format!(
        "#!/bin/sh\ncat >/dev/null\n: > '{path}'\nprintf 'FILTERED\\n'\n",
        path = sentinel.display()
    )
}

/// Declare the hostile filter in the repository's own metadata.
///
/// `.gitattributes` names the filter and `.git/config` gives it a command,
/// which is exactly the metadata a hostile repository ships.
fn declare_filter(repo: &Path, command: &str) {
    std::fs::write(
        repo.join(".gitattributes"),
        "*.bin filter=codegg-hostile-filter\n",
    )
    .unwrap();
    run_git(
        repo,
        &["config", "filter.codegg-hostile-filter.clean", command],
    );
    run_git(
        repo,
        &["config", "filter.codegg-hostile-filter.smudge", command],
    );
}

/// C007: the publisher never invokes a repository-declared clean/smudge
/// filter.
///
/// The staging child reads the **daemon-authored** private gitdir, whose
/// configuration contains no `filter.*` section at all, so Git has no
/// command to run for the filter the repository names and passes the
/// content through unchanged.
///
/// The absence assertion would be vacuous if the filter could never have
/// run, so the test carries two positive controls: the filter command is
/// run directly, and Git is shown to invoke the repository-declared filter
/// for the same path in a separate repository. Both must produce the
/// sentinel, or the test fails loudly.
#[tokio::test]
async fn the_publisher_never_invokes_a_repository_declared_clean_filter() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    // The filter's own scratch: the command and the sentinel live under a
    // temporary directory, never at a fixed or shared location.
    let filter_dir = TempDir::new().unwrap();
    let sentinel = filter_dir.path().join("clean-filter-sentinel");
    let command = filter_dir.path().join("clean-filter.sh");
    std::fs::write(&command, filter_script(&sentinel)).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let clean_command = format!("sh '{}'", command.display());

    // ── Positive control 1: the command itself can produce the sentinel.
    let mut child = Command::new("sh")
        .arg(&command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the filter command runs");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(b"PAYLOAD\n")
            .expect("the filter command accepts the content");
    }
    let output = child
        .wait_with_output()
        .expect("the filter command finishes");
    assert!(
        output.status.success(),
        "the filter command failed: {output:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).into_owned(),
        "FILTERED\n",
        "the filter must rewrite the content, or the published bytes prove nothing"
    );
    assert!(
        sentinel.exists(),
        "the filter command must be able to create the sentinel, or the absence assertion below \
         would be vacuous"
    );
    std::fs::remove_file(&sentinel).unwrap();

    // ── Positive control 2: Git itself invokes the declared filter.
    //
    // The same repository metadata, in a separate repository, so Git runs
    // the filter for the same path in this environment.
    let control_repo = temp.path().join("control-repo");
    std::fs::create_dir_all(&control_repo).unwrap();
    init_repo(&control_repo);
    declare_filter(&control_repo, &clean_command);
    std::fs::write(control_repo.join("payload.bin"), "PAYLOAD\n").unwrap();
    run_git(&control_repo, &["add", "--", "payload.bin"]);
    assert!(
        sentinel.exists(),
        "Git must invoke the repository-declared clean filter for the covered path, or the \
         absence assertion below would be vacuous"
    );
    assert_eq!(
        run_git(&control_repo, &["show", ":payload.bin"]),
        "FILTERED\n",
        "the control must show the declared filter applied"
    );
    std::fs::remove_file(&sentinel).unwrap();

    // ── The repository under test declares the same hostile filter.
    declare_filter(&repo, &clean_command);
    std::fs::write(repo.join("payload.bin"), "PAYLOAD\n").unwrap();

    let publisher = publisher(&pool, &scratch, LocalGitStaging);
    let outcome = publisher
        .publish_add(&request(&repo, &["payload.bin"], "filter-free"))
        .await
        .expect("publish");
    let receipt = match outcome {
        GitIndexPublicationOutcome::Committed(receipt) => receipt,
        other => panic!("expected a committed publication, got {other:?}"),
    };
    assert_eq!(receipt.staged_paths, vec!["payload.bin".to_string()]);

    // The filter never ran.
    assert!(
        !sentinel.exists(),
        "the publisher invoked the repository-declared clean filter"
    );

    // The ordinary path still works: the publication committed and the
    // published blob is the **unfiltered** worktree content.
    let staged = run_git(&repo, &["ls-files", "--stage", "payload.bin"]);
    assert!(
        staged.contains("payload.bin"),
        "the published index must hold the path, got {staged:?}"
    );
    assert_eq!(
        run_git(&repo, &["show", ":payload.bin"]),
        "PAYLOAD\n",
        "the published blob must be the unfiltered content"
    );
}

/// A staging step that observed the repository's index **live**.
///
/// The publisher's compare-and-swap can only be trusted when the staged
/// content and the fingerprint it swaps against describe the same
/// generation. A live read cannot promise that, so the publisher refuses
/// it.
struct LiveMutableStaging;

#[async_trait]
impl GitIndexStaging for LiveMutableStaging {
    async fn stage(
        &self,
        request: &GitIndexStagingRequest,
    ) -> Result<ObtainedNetworkBoundary, String> {
        LocalGitStaging.stage(request).await
    }

    fn observed_consistency(&self) -> GitReadConsistency {
        GitReadConsistency::LiveMutable
    }
}

/// C003: the publisher refuses a live-class prepared index before any
/// mutation.
///
/// A live-class preparation was read from a concurrently mutable
/// repository, so its staged content and its compare-and-swap operand may
/// not describe the same generation. The refusal is a *fence*: it runs
/// before the lock, before the journal, and before any object import, so
/// the real index and the refs are byte-identical afterwards and the
/// journal holds no row.
#[tokio::test]
async fn a_live_class_prepared_index_is_refused_before_any_mutation() {
    let pool = isolated_pool().await;
    let temp = TempDir::new().unwrap();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let gitdir = repo_gitdir(&repo);

    std::fs::write(repo.join("tracked.txt"), "changed\n").unwrap();
    let index_before = read_bytes(&gitdir.join("index")).expect("index exists");
    let refs_before = refs_bytes(&gitdir);
    let objects_before = loose_objects(&gitdir.join("objects"));

    let store = Arc::new(GitIndexPublicationStore::new(pool.clone()));
    let live_publisher = GitIndexPublisher::new(
        store,
        scratch.clone(),
        Arc::new(LiveMutableStaging),
        Duration::from_secs(30),
    );
    let outcome = live_publisher
        .publish_add(&request(&repo, &["tracked.txt"], "live-class"))
        .await
        .expect("publish");

    assert_eq!(
        outcome,
        GitIndexPublicationOutcome::Refused(GitIndexPublicationRefusal::StagingNotCaptured {
            class: GitReadConsistency::LiveMutable,
        })
    );

    // Nothing was written: the refusal came before the lock, before the
    // journal and before any object import.
    assert_eq!(
        read_bytes(&gitdir.join("index")),
        Some(index_before.clone()),
        "a refused publication must not touch the index"
    );
    assert_eq!(
        refs_bytes(&gitdir),
        refs_before,
        "a refused publication must not touch a ref"
    );
    assert_eq!(
        loose_objects(&gitdir.join("objects")),
        objects_before,
        "a refused publication must not import an object"
    );
    assert!(!gitdir.join("index.lock").exists());

    // No journal row was written for the refused intent.
    let store = GitIndexPublicationStore::new(pool.clone());
    assert!(store.get("live-class").await.unwrap().is_none());
    assert!(store.list_unresolved(16).await.unwrap().is_empty());

    // The fence does not break the ordinary captured path.
    let publisher = publisher(&pool, &scratch, LocalGitStaging);
    let outcome = publisher
        .publish_add(&request(&repo, &["tracked.txt"], "captured-class"))
        .await
        .expect("publish");
    let receipt = match outcome {
        GitIndexPublicationOutcome::Committed(receipt) => receipt,
        other => panic!("expected a committed publication, got {other:?}"),
    };
    assert_eq!(receipt.consistency, GitReadConsistency::CapturedNotCurrent);
    assert_ne!(
        read_bytes(&gitdir.join("index")),
        Some(index_before),
        "the ordinary captured path must still publish"
    );
}

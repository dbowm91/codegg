//! Integration test for M008: the record-less scratch tree, driven through a
//! **full sweep pass**.
//!
//! The unit tests in `src/core/scratch_sweep.rs` decide a candidate
//! in-process. This member drives the same production entry point from
//! outside the crate, under an isolated environment, and asserts the **end
//! state of the sessions root** — which is what an operator actually sees.
//!
//! Two shapes are covered, because they are different shapes:
//!
//! - a tree whose owner record is missing and that holds **no entries** is
//!   already reclaimed: the sweep removes it and reports no failure;
//! - a tree whose owner record is missing and that holds **content** cannot
//!   be proven owned, so the sweep refuses it — as a refusal, not as a
//!   failure — and leaves the same tombstone behind on every pass.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use codegg::core::scratch_sweep::{sweep_scratch_once, ScratchRetentionPolicy};
use codegg_core::agent_run::InMemoryAgentRunStore;
use codegg_core::jobs::InMemoryJobStore;

use super::common::isolated_env::IsolatedEnv;

/// The sessions root the daemon owns, under the isolated environment.
fn sessions_root() -> PathBuf {
    dirs::cache_dir()
        .expect("cache dir")
        .join("codegg")
        .join("scratch")
        .join("sessions")
}

/// The tombstone names the sessions root holds for one digest.
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

/// Backdate a tree so the orphan grace period has passed.
fn age_tree(path: &Path, age_ms: i64) {
    let modified = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_millis(
            (chrono::Utc::now().timestamp_millis() - age_ms).max(0) as u64
        );
    let times = std::fs::FileTimes::new().set_modified(modified);
    let _ = std::fs::File::open(path).and_then(|file| file.set_times(times));
}

/// An in-memory store for the sweep to read.
async fn sweep_pool() -> sqlx::SqlitePool {
    let url = format!(
        "file:scratch-record-less-{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4().simple()
    );
    let options = sqlx::sqlite::SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite url")
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("sqlite pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("session schema");
    pool
}

/// An isolated environment, so the developer's own scratch root is never
/// this test's fixture.
fn isolated_environment(label: &str) -> (tempfile::TempDir, IsolatedEnv) {
    let root = tempfile::Builder::new()
        .prefix(&format!("codegg-scratch-{label}-"))
        .tempdir()
        .expect("scratch environment");
    // The scratch root refuses a symlinked path component, and a temporary
    // directory's own ancestors can contain one (`/tmp` is a symlink on
    // macOS), so the root is canonicalized the way the product's own scratch
    // home is before it hands the path to the allocator.
    let canonical = std::fs::canonicalize(root.path()).expect("canonical scratch environment");
    let env = IsolatedEnv::new(&canonical);
    (root, env)
}

/// A tree whose owner record is missing.
///
/// The tree is created through the product's own allocator, so it carries
/// the mode and the owner the reclaim validator checks; only the record is
/// removed, which is the shape a daemon killed between `create_new` and the
/// record reaching disk leaves behind.
fn plant_record_less_tree(session_id: &str) -> PathBuf {
    let root = codegg::scratch::session_scratch_dir(session_id).expect("create the tree");
    std::fs::remove_file(root.join(".codegg-owner")).expect("drop the owner record");
    root
}

#[tokio::test(flavor = "current_thread")]
async fn a_sweep_pass_reclaims_an_empty_tree_whose_owner_record_is_missing() {
    let (_root, _env) = isolated_environment("empty-record-less");
    let pool = sweep_pool().await;
    let runs = InMemoryAgentRunStore::new();
    let jobs = InMemoryJobStore::new();
    // A tree with no session row at all.
    let orphan_id = format!("orphan-{}", uuid::Uuid::new_v4());
    let tree = plant_record_less_tree(&orphan_id);
    age_tree(&tree, 48 * 60 * 60 * 1000);
    let digest = codegg::scratch::session_scratch_digest_hex(&orphan_id);

    let report = sweep_scratch_once(
        &pool,
        &runs,
        &jobs,
        ScratchRetentionPolicy::default(),
        chrono::Utc::now().timestamp_millis(),
    )
    .await;

    assert_eq!(report.failed, 0, "report: {report:?}");
    assert_eq!(report.reclaimed_orphan, 1, "report: {report:?}");
    assert!(
        tombstone_names(&digest).is_empty(),
        "the reclaimed tree must leave no tombstone behind"
    );
    assert!(!tree.exists(), "the tree is gone");
}

#[tokio::test(flavor = "current_thread")]
async fn two_sweep_passes_over_a_refused_tree_keep_the_same_tombstone() {
    let (_root, _env) = isolated_environment("content-record-less");
    let pool = sweep_pool().await;
    let runs = InMemoryAgentRunStore::new();
    let jobs = InMemoryJobStore::new();
    let orphan_id = format!("orphan-{}", uuid::Uuid::new_v4());
    let tree = plant_record_less_tree(&orphan_id);
    std::fs::write(tree.join("artifact.bin"), b"hello").expect("write an artifact");
    age_tree(&tree, 48 * 60 * 60 * 1000);
    let digest = codegg::scratch::session_scratch_digest_hex(&orphan_id);

    let mut names_after_first_pass: Option<Vec<String>> = None;
    for pass in 1..=2 {
        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            ScratchRetentionPolicy::default(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.failed, 0, "pass {pass} must not fail: {report:?}");
        assert_eq!(
            report.refused, 1,
            "pass {pass} must count the refusal: {report:?}"
        );

        let names = tombstone_names(&digest);
        assert_eq!(
            names.len(),
            1,
            "pass {pass} must leave exactly one tombstone: {names:?}"
        );
        match &names_after_first_pass {
            None => names_after_first_pass = Some(names),
            Some(previous) => assert_eq!(
                previous, &names,
                "the tombstone's name must be stable across passes"
            ),
        }
    }
}

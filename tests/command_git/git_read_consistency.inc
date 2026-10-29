//! C003: the typed Git read-consistency contract and its admissibility fence.
//!
//! A Git read observes the repository under very different guarantees, and the
//! difference decides whether the result may feed a generation-bound consumer.
//! `egggit` supplies the vocabulary and [`admit_generation_consistent`] is the
//! fence; the root crate's own fence over the same API is
//! [`codegg::git_consistency::admit_generation_bound`].
//!
//! The suite is split in two:
//!
//! * **Suite A — the fence itself.** Pure, no repository: a live-class read is
//!   inadmissible without the capability, a read with no object generation is
//!   refused, captured classes are admitted, and the class cannot be dropped.
//! * **Suite B — external-writer negatives and provenance.** Real temporary
//!   repositories: a stale generation after an outside mutation, a concurrent
//!   outside writer, an alternate object database, a replace ref, a
//!   producer/consumer mismatch that must be refused *before* any mutation, and
//!   the provenance of a failure.
//!
//! The fixture follows `tests/command_git/git_index_publication.inc`: a `tempfile::tempdir()`
//! root, a real `git` binary resolved once, and `GIT_CONFIG_NOSYSTEM` /
//! `GIT_CONFIG_GLOBAL` pinned so the host's own Git configuration cannot
//! influence a fixture.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use codegg::git_consistency::{
    admit_generation_bound, admit_generation_consistent, live_read_capability,
    GenerationConsistent, GitConsistencyRefusal, GitReadConsistency, GitReadFact,
    LiveReadCapability,
};
use codegg::git_service::GitServiceError;
use egggit::process::{self, GitCommandOutput};
use egggit::{EgggitError, GitExecutionPolicyOutcome};

// ── Suite A: the fence itself ───────────────────────────────────────────────

#[test]
fn a_live_class_read_is_inadmissible_to_a_generation_bound_consumer() {
    let fact = GitReadFact::new("payload".to_string(), GitReadConsistency::LiveMutable);

    let refusal = admit_generation_consistent(fact, None)
        .expect_err("a live read must be refused without the live-read capability");

    assert_eq!(
        refusal,
        GitConsistencyRefusal::LiveClassWithoutCapability {
            class: GitReadConsistency::LiveMutable
        }
    );
    assert_eq!(refusal.as_str(), "live_class_without_capability");
}

#[test]
fn a_read_that_observed_no_object_generation_is_refused() {
    let fact = GitReadFact::new(
        "payload".to_string(),
        GitReadConsistency::NoObjectGeneration,
    );

    let refusal = admit_generation_consistent(fact, None)
        .expect_err("a read that observed no object generation must be refused");

    assert_eq!(refusal, GitConsistencyRefusal::NoObjectGeneration);
    assert_eq!(refusal.as_str(), "no_object_generation");
    // The refusal is a typed error the caller cannot mistake for a warning.
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("object generation"),
        "the refusal must explain itself: {rendered}"
    );
    let as_error: Box<dyn std::error::Error> = Box::new(refusal);
    assert!(as_error.to_string().contains("object generation"));
}

#[test]
fn captured_classes_are_admitted_and_round_trip_their_class() {
    for class in [
        GitReadConsistency::ImmutableGeneration,
        GitReadConsistency::CapturedNotCurrent,
    ] {
        assert!(
            class.is_generation_consistent(),
            "{} must be generation-consistent",
            class.as_str()
        );
        assert!(
            !class.is_live(),
            "{} must not be a live class",
            class.as_str()
        );

        let fact = GitReadFact::new(7u32, class);
        let admitted = admit_generation_consistent(fact, None)
            .expect("a captured generation must be admitted");
        assert_eq!(admitted.class(), class);
        assert_eq!(*admitted.value(), 7);
        assert_eq!(admitted.into_value(), 7);
    }
}

#[test]
fn the_class_cannot_be_dropped_from_a_read_fact() {
    // The class travels with the value.
    let fact = GitReadFact::new(vec![1u8, 2, 3], GitReadConsistency::CapturedNotCurrent);
    assert_eq!(fact.class(), GitReadConsistency::CapturedNotCurrent);

    // Mapping the payload preserves the class.
    let mapped = fact.map(|bytes| bytes.len());
    assert_eq!(mapped.class(), GitReadConsistency::CapturedNotCurrent);
    assert_eq!(*mapped.value(), 3);

    // The admitted value still carries the class it was admitted under.
    let fact = GitReadFact::new(1u8, GitReadConsistency::ImmutableGeneration);
    let admitted = admit_generation_consistent(fact, None).expect("admitted");
    assert_eq!(admitted.class(), GitReadConsistency::ImmutableGeneration);
}

#[test]
fn a_read_fact_is_not_a_generation_consistent_value() {
    // The type-level property that matters: a generation-bound consumer takes
    // a `GenerationConsistent<T>`, and the only way to build one is
    // `admit_generation_consistent`. Handing the consumer a `GitReadFact<T>`
    // instead is a type error (E0308, mismatched types) — the compiler rejects
    // it, so the class cannot be dropped by accident. The code below is the
    // compiling half of that statement: the consumer accepts the admitted
    // value and nothing else.
    fn generation_bound_consumer<T>(admitted: &GenerationConsistent<T>) -> GitReadConsistency {
        admitted.class()
    }

    let fact = GitReadFact::new(1u8, GitReadConsistency::CapturedNotCurrent);
    let admitted = admit_generation_consistent(fact, None).expect("admitted");
    assert_eq!(
        generation_bound_consumer(&admitted),
        GitReadConsistency::CapturedNotCurrent
    );

    // The two types are distinct, so no implicit conversion bridges them.
    assert_ne!(
        std::any::TypeId::of::<GitReadFact<u8>>(),
        std::any::TypeId::of::<GenerationConsistent<u8>>(),
        "a read fact and an admitted value must be different types"
    );
}

#[cfg(all(feature = "linux-git-test-support", target_os = "linux"))]
#[test]
fn a_live_read_is_admitted_only_when_the_capability_is_held() {
    let capability: &LiveReadCapability =
        &codegg::git_consistency::mint_test_only_live_read_capability();

    // The crate accessor still hands out nothing: the capability is minted
    // at the point of use by the reviewed route, never handed out.
    assert!(
        live_read_capability().is_none(),
        "the crate accessor must not hand out a live-read capability"
    );

    let fact = GitReadFact::new("live payload".to_string(), GitReadConsistency::LiveMutable);
    let admitted = admit_generation_consistent(fact, Some(capability))
        .expect("a live read is admissible to a holder of the live-read capability");

    // The capability authorises the use; it does not rewrite the class.
    assert_eq!(admitted.class(), GitReadConsistency::LiveMutable);
    assert_eq!(*admitted.value(), "live payload");

    // The same fact without the capability is refused.
    let fact = GitReadFact::new("live payload".to_string(), GitReadConsistency::LiveMutable);
    let refusal = admit_generation_consistent(fact, None).expect_err("refused without capability");
    assert_eq!(
        refusal,
        GitConsistencyRefusal::LiveClassWithoutCapability {
            class: GitReadConsistency::LiveMutable
        }
    );
}

#[cfg(not(all(feature = "linux-git-test-support", target_os = "linux")))]
#[test]
fn the_live_read_capability_cannot_be_obtained_in_a_production_build() {
    // The production guarantee. `LiveReadCapability`'s only constructor is
    // compiled out of `egggit` unless its test-only support feature is
    // enabled, so this file compiles at all only because the capability can be
    // *named* but never *built*: the field is private and the constructor does
    // not exist. The fence therefore cannot be relaxed in a shipped build.
    let capability: Option<&LiveReadCapability> = None;
    assert!(
        capability.is_none(),
        "a production build must not hold a live-read capability"
    );
    assert!(
        live_read_capability().is_none(),
        "the crate accessor must not hand out a live-read capability"
    );

    // And a live-class fact is therefore always refused.
    let fact = GitReadFact::new("live payload".to_string(), GitReadConsistency::LiveMutable);
    let refusal = admit_generation_consistent(fact, None).expect_err("refused");
    assert_eq!(
        refusal,
        GitConsistencyRefusal::LiveClassWithoutCapability {
            class: GitReadConsistency::LiveMutable
        }
    );
}

#[test]
fn the_weaker_class_wins_for_a_composite_read() {
    use GitReadConsistency::*;

    // `LiveMutable` is the weakest: the bytes may be torn.
    assert_eq!(LiveMutable.weaker(ImmutableGeneration), LiveMutable);
    assert_eq!(ImmutableGeneration.weaker(LiveMutable), LiveMutable);
    assert_eq!(CapturedNotCurrent.weaker(LiveMutable), LiveMutable);
    assert_eq!(NoObjectGeneration.weaker(LiveMutable), LiveMutable);

    // `NoObjectGeneration` sits between `LiveMutable` and `CapturedNotCurrent`:
    // such a part of the read observed no object generation at all, so the
    // composite cannot claim one.
    assert_eq!(
        NoObjectGeneration.weaker(CapturedNotCurrent),
        NoObjectGeneration
    );
    assert_eq!(
        CapturedNotCurrent.weaker(NoObjectGeneration),
        NoObjectGeneration
    );
    assert_eq!(
        NoObjectGeneration.weaker(ImmutableGeneration),
        NoObjectGeneration
    );
    assert_eq!(
        ImmutableGeneration.weaker(NoObjectGeneration),
        NoObjectGeneration
    );

    // `CapturedNotCurrent` is weaker than `ImmutableGeneration`.
    assert_eq!(
        CapturedNotCurrent.weaker(ImmutableGeneration),
        CapturedNotCurrent
    );
    assert_eq!(
        ImmutableGeneration.weaker(CapturedNotCurrent),
        CapturedNotCurrent
    );

    // The operation is idempotent.
    assert_eq!(
        ImmutableGeneration.weaker(ImmutableGeneration),
        ImmutableGeneration
    );
    assert_eq!(
        CapturedNotCurrent.weaker(CapturedNotCurrent),
        CapturedNotCurrent
    );
    assert_eq!(
        NoObjectGeneration.weaker(NoObjectGeneration),
        NoObjectGeneration
    );
    assert_eq!(LiveMutable.weaker(LiveMutable), LiveMutable);
}

// ── Suite B: fixtures ───────────────────────────────────────────────────────

/// Serialises the tests in this file that share the trusted route's
/// process-global preparation cache.
///
/// The cache is a process-global, best-effort mechanism with eviction: two
/// tests that snapshot object stores at the same time can evict each other's
/// cached generation. The lock makes the cache's state deterministic — within
/// this integration-test binary the repository fixtures are used one at a
/// time — so a read that follows another read of the same repository is
/// guaranteed to be the cache-served one.
///
/// The mutex is an async one on purpose: a `std::sync::Mutex` guard held
/// across an await point is a deadlock waiting to happen, and every read in
/// this file is asynchronous.
static OBJECT_VIEW_CACHE_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
    std::sync::OnceLock::new();

fn object_view_cache_lock() -> &'static tokio::sync::Mutex<()> {
    OBJECT_VIEW_CACHE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// A scratch root for a repository fixture.
fn repository_scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn git_binary() -> PathBuf {
    codegg::git_publication::resolve_git_binary().expect("git is available on PATH")
}

/// Run a plain `git` command, outside the trusted read route.
fn run_git_raw(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(git_binary())
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs")
}

/// Run a plain `git` command and return its stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = run_git_raw(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A repository with one commit, so the real index and refs exist.
fn init_repo(dir: &Path) {
    init_repo_with(dir, "original\n", "init");
}

/// A repository with one commit whose content is distinct, so two fixtures
/// never share an object identity.
fn init_repo_with(dir: &Path, content: &str, message: &str) {
    run_git(dir, &["init", "--quiet", "--initial-branch=main"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
    std::fs::write(dir.join("tracked.txt"), content).expect("write the tracked file");
    run_git(dir, &["add", "tracked.txt"]);
    run_git(dir, &["commit", "--quiet", "-m", message]);
}

/// A read through the trusted route.
async fn trusted_read(root: &Path, args: &[&str]) -> Result<GitCommandOutput, EgggitError> {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    process::run(&args, root).await
}

/// The bytes of every ref file, plus `HEAD` and `packed-refs`, as a comparable
/// snapshot of the repository's refs.
fn snapshot_refs(root: &Path) -> Vec<(String, Vec<u8>)> {
    let git_dir = root.join(".git");
    let mut snapshot: Vec<(String, Vec<u8>)> = Vec::new();
    let mut pending = vec![git_dir.join("refs")];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let name = path
                .strip_prefix(&git_dir)
                .unwrap_or(&path)
                .display()
                .to_string();
            snapshot.push((name, std::fs::read(&path).unwrap_or_default()));
        }
    }
    for name in ["HEAD", "packed-refs"] {
        let path = git_dir.join(name);
        if path.is_file() {
            snapshot.push((name.to_string(), std::fs::read(&path).unwrap_or_default()));
        }
    }
    snapshot.sort();
    snapshot
}

// ── Suite B: external-writer negatives and provenance ──────────────────────

#[tokio::test(flavor = "current_thread")]
async fn a_mixed_snapshot_read_reports_the_weakest_of_its_contributing_spawns() {
    // A composite read is assembled from several spawns, each with its own
    // class: `git branch --show-current` consults no object store, while
    // `git status --porcelain` and `git rev-parse HEAD` do. The composite
    // class must be the weakest of them, so the read never claims a
    // generation it cannot prove.
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let root = dir.path();
    init_repo(root);

    let fact = egggit::status::repo_status(root)
        .await
        .expect("the status read");
    assert_eq!(fact.value().branch, "main");
    assert_eq!(
        fact.class(),
        GitReadConsistency::NoObjectGeneration,
        "the composite class must be the weakest contributing class"
    );

    // And such a read cannot satisfy a generation-bound consumer.
    let refusal = admit_generation_consistent(fact, None)
        .expect_err("a mixed read with no object generation must be refused");
    assert_eq!(refusal, GitConsistencyRefusal::NoObjectGeneration);
    assert_eq!(refusal.as_str(), "no_object_generation");
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_after_an_outside_mutation_still_reports_a_captured_generation() {
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let root = dir.path();
    init_repo(root);

    let first = trusted_read(root, &["status", "--porcelain"])
        .await
        .expect("the trusted route reads the repository");
    assert!(
        first.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        first.consistency.is_generation_consistent(),
        "the trusted route must not report a live class, got {}",
        first.consistency.as_str()
    );

    // An outside writer moves the repository on: a new commit changes the
    // object store the read route observes.
    std::fs::write(root.join("tracked.txt"), "changed\n").expect("write the tracked file");
    run_git(root, &["add", "tracked.txt"]);
    run_git(root, &["commit", "--quiet", "-m", "second"]);

    let second = trusted_read(root, &["status", "--porcelain"])
        .await
        .expect("the trusted route still reads the repository");
    assert!(
        second.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    // The class after the mutation is a captured generation. The exact
    // captured class depends on the preparation cache's state — a rebuild
    // yields `ImmutableGeneration`, a cache hit yields `CapturedNotCurrent` —
    // so the assertion is the one that holds in both cases: the read never
    // claims to be a live read of a concurrently mutable repository, and the
    // class it does claim is admissible to a generation-bound consumer.
    assert!(
        second.consistency.is_generation_consistent(),
        "expected a captured generation, got {}",
        second.consistency.as_str()
    );
    assert!(!second.consistency.is_live());
    assert_ne!(second.consistency, GitReadConsistency::LiveMutable);

    let admitted = admit_generation_consistent(
        GitReadFact::new(
            String::from_utf8_lossy(&second.stdout).into_owned(),
            second.consistency,
        ),
        None,
    )
    .expect("a captured generation is admissible");
    assert_eq!(admitted.class(), second.consistency);

    // The same fact as a live class would not be admissible: the difference
    // is the class, not the payload.
    let live = GitReadFact::new(
        String::from_utf8_lossy(&second.stdout).into_owned(),
        GitReadConsistency::LiveMutable,
    );
    assert!(admit_generation_consistent(live, None).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn a_repeat_read_of_an_unchanged_repository_stays_generation_consistent() {
    // The fixture holds the preparation-cache lock so another test cannot
    // evict this repository's generation between the two reads.
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let root = dir.path();
    init_repo(root);

    let first = trusted_read(root, &["rev-parse", "HEAD"])
        .await
        .expect("the trusted route reads the repository");
    assert!(
        first.status.success(),
        "git rev-parse failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        first.consistency,
        GitReadConsistency::ImmutableGeneration,
        "the first read materialises a fresh, stability-checked generation"
    );

    let second = trusted_read(root, &["rev-parse", "HEAD"])
        .await
        .expect("the trusted route reads the repository");
    assert!(
        second.status.success(),
        "git rev-parse failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    // A clean cache hit is captured; if the fallback fingerprint observes
    // maintenance, rebuilding yields a fresh immutable generation. Either
    // result is generation-consistent and must never be reported live.
    assert!(second.consistency.is_generation_consistent());
    assert!(!second.consistency.is_live());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_concurrent_outside_writer_never_yields_an_over_claiming_class() {
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let root = dir.path().to_path_buf();
    init_repo(&root);

    // An outside writer repacks the object store repeatedly while the reads
    // run, so the reads genuinely overlap a mutation of the repository they
    // are reading rather than merely racing a writer that has not started yet.
    let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
    let writer_root = root.clone();
    let writer = std::thread::spawn(move || {
        started_tx.send(()).ok();
        for _ in 0..8 {
            let _ = Command::new(git_binary())
                .args(["repack", "-adq"])
                .current_dir(&writer_root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output();
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    started_rx.recv().expect("the outside writer started");

    // Every read that overlaps the writer must either complete with a
    // captured class or fail closed.
    for _ in 0..8 {
        match trusted_read(&root, &["log", "--oneline"]).await {
            Ok(output) => {
                assert!(
                    output.status.success(),
                    "the trusted read must not serve a torn result: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    !output.consistency.is_live(),
                    "a live class over-claims: {}",
                    output.consistency.as_str()
                );
                assert!(
                    output.consistency.is_generation_consistent()
                        || output.consistency == GitReadConsistency::NoObjectGeneration,
                    "unexpected class {}",
                    output.consistency.as_str()
                );
            }
            Err(error) => {
                // Failing closed is the other permitted outcome, and it is typed.
                assert!(
                    matches!(
                        error,
                        EgggitError::Policy(GitExecutionPolicyOutcome::BackendUnavailable { .. })
                    ),
                    "the read must fail closed with a typed refusal, got {error:?}"
                );
            }
        }
    }
    writer.join().expect("the outside writer finishes");

    // The repository is still usable afterwards.
    let after = trusted_read(&root, &["log", "--oneline"])
        .await
        .expect("the repository is still usable");
    assert!(
        after.status.success(),
        "git log failed after the concurrent writer: {}",
        String::from_utf8_lossy(&after.stderr)
    );
    assert!(!after.consistency.is_live());
}

#[tokio::test(flavor = "current_thread")]
async fn a_repository_with_an_alternate_object_database_is_refused_by_the_trusted_route() {
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let donor = dir.path().join("donor");
    let consumer = dir.path().join("consumer");
    std::fs::create_dir_all(&donor).expect("donor directory");
    std::fs::create_dir_all(&consumer).expect("consumer directory");
    init_repo_with(&donor, "donor only\n", "donor");
    init_repo_with(&consumer, "consumer only\n", "consumer");

    let donor_head = run_git(&donor, &["rev-parse", "HEAD"]).trim().to_string();

    // Positive control: once the consumer's object store names the donor's as
    // an alternate, a plain Git invocation does serve the donor's object.
    let objects_info = consumer.join(".git/objects/info");
    std::fs::create_dir_all(&objects_info).expect("objects/info");
    std::fs::write(
        objects_info.join("alternates"),
        format!("{}\n", donor.join(".git/objects").display()),
    )
    .expect("write the alternates file");

    let control = run_git_raw(&consumer, &["cat-file", "-p", &donor_head]);
    assert!(
        control.status.success(),
        "the control must show the alternate is effective: {}",
        String::from_utf8_lossy(&control.stderr)
    );
    assert!(
        String::from_utf8_lossy(&control.stdout).contains("donor"),
        "the control must observe the donor's object"
    );

    // The trusted read route must not serve the alternate's objects as if
    // they were the repository's: it refuses the repository outright.
    let error = trusted_read(&consumer, &["cat-file", "-p", &donor_head])
        .await
        .expect_err("the trusted route must refuse a repository with an alternate object database");
    assert!(
        matches!(
            error,
            EgggitError::Policy(GitExecutionPolicyOutcome::BackendUnavailable { .. })
        ),
        "the refusal must be typed, got {error:?}"
    );

    // The repository itself is intact: only the trusted route refuses it.
    let plain = run_git_raw(&consumer, &["cat-file", "-p", &donor_head]);
    assert!(
        plain.status.success(),
        "the repository must remain usable for a plain Git invocation"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_trusted_read_route_does_not_honour_replace_refs() {
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let root = dir.path();
    init_repo(root);

    let head = run_git(root, &["rev-parse", "HEAD"]).trim().to_string();
    let tree = run_git(root, &["rev-parse", "HEAD^{tree}"])
        .trim()
        .to_string();
    let replacement = run_git(
        root,
        &["commit-tree", &tree, "-p", &head, "-m", "replacement"],
    )
    .trim()
    .to_string();
    run_git(
        root,
        &["update-ref", &format!("refs/replace/{head}"), &replacement],
    );

    // Positive control: a plain Git invocation honours the replace ref, so
    // the ref is live and the assertion below is not vacuous.
    let control = run_git_raw(root, &["cat-file", "-p", &head]);
    assert!(
        control.status.success(),
        "the control must succeed: {}",
        String::from_utf8_lossy(&control.stderr)
    );
    assert!(
        String::from_utf8_lossy(&control.stdout).contains("replacement"),
        "the control must observe the replacement object"
    );

    // The trusted route sets `GIT_NO_REPLACE_OBJECTS=1`, so it serves the
    // repository's own object: the substituted object has a different
    // identity and would not describe the generation the repository holds.
    let trusted = trusted_read(root, &["cat-file", "-p", &head])
        .await
        .expect("the trusted route reads the repository");
    assert!(
        trusted.status.success(),
        "git cat-file failed: {}",
        String::from_utf8_lossy(&trusted.stderr)
    );
    let trusted_text = String::from_utf8_lossy(&trusted.stdout);
    assert!(
        !trusted_text.contains("replacement"),
        "the trusted route honoured the replace ref: {trusted_text}"
    );
    assert!(
        trusted_text.contains("init"),
        "the trusted route must serve the repository's own object: {trusted_text}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_producer_consumer_mismatch_is_refused_before_any_mutation() {
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let root = dir.path();
    init_repo(root);

    let index_path = root.join(".git/index");
    let index_before = std::fs::read(&index_path).expect("the index exists");
    let refs_before = snapshot_refs(root);

    // The producer performs a read and hands the result on as a live-class
    // fact: it cannot prove that the bytes came from one object generation.
    let produced = trusted_read(root, &["status", "--porcelain"])
        .await
        .expect("the producer read");
    assert!(
        produced.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&produced.stderr)
    );
    let fact = GitReadFact::new(
        String::from_utf8_lossy(&produced.stdout).into_owned(),
        GitReadConsistency::LiveMutable,
    );

    // The consumer refuses it.
    let refusal = admit_generation_consistent(fact, None)
        .expect_err("a live-class fact must be refused by a generation-bound consumer");
    assert_eq!(
        refusal,
        GitConsistencyRefusal::LiveClassWithoutCapability {
            class: GitReadConsistency::LiveMutable
        }
    );

    // The root crate's own fence maps the refusal onto a typed service error,
    // so a caller that ignores the fence still cannot turn it into a payload.
    let fact = GitReadFact::new(String::new(), GitReadConsistency::LiveMutable);
    let error = admit_generation_bound(fact, None).expect_err("the root fence must refuse it");
    assert!(
        matches!(
            error,
            GitServiceError::Consistency(GitConsistencyRefusal::LiveClassWithoutCapability {
                class: GitReadConsistency::LiveMutable
            })
        ),
        "unexpected error: {error:?}"
    );

    // No mutation occurred: the index file is byte-identical and the refs are
    // unchanged.
    let index_after = std::fs::read(&index_path).expect("the index exists");
    assert_eq!(
        index_before, index_after,
        "the refused read mutated the repository index"
    );
    assert_eq!(
        refs_before,
        snapshot_refs(root),
        "the refused read mutated the repository refs"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_read_is_a_typed_error_and_cannot_be_mistaken_for_a_result() {
    let dir = repository_scratch();
    let _cache_guard = object_view_cache_lock().lock().await;
    let root = dir.path();
    init_repo(root);

    // A command that fails at the Git level: the spawn succeeds, Git exits
    // non-zero, and the outcome still records the consistency class of the
    // bytes that were read.
    let output = trusted_read(
        root,
        &["rev-parse", "--verify", "refs/heads/does-not-exist"],
    )
    .await
    .expect("the spawn itself succeeds");
    assert!(!output.status.success(), "git must fail for a missing ref");
    assert!(
        !output.stderr.is_empty(),
        "a failure must carry the reason it failed"
    );
    assert!(
        output.consistency.is_generation_consistent()
            || output.consistency == GitReadConsistency::NoObjectGeneration,
        "unexpected class {}",
        output.consistency.as_str()
    );

    // The typed read API refuses to produce a fact for a failed command, so a
    // caller cannot mistake a failure for an admissible result.
    let error = egggit::refs::resolve_base_ref(root, "refs/heads/does-not-exist")
        .await
        .expect_err("a missing ref must not produce a read fact");
    assert!(
        matches!(error, EgggitError::Git(_)),
        "the failure must be typed, got {error:?}"
    );

    // And an invalid base ref is refused before Git is spawned at all.
    let error = egggit::refs::resolve_base_ref(root, "not a ref")
        .await
        .expect_err("an invalid base ref must be refused");
    assert!(
        matches!(error, EgggitError::InvalidBaseRef(_)),
        "the refusal must be typed, got {error:?}"
    );
}

#[test]
fn the_operation_state_sentinel_read_reports_no_object_generation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    let clean = egggit::operation_state::detect_repository_operation_state(&root.join(".git"));
    assert_eq!(
        clean.class(),
        GitReadConsistency::NoObjectGeneration,
        "the sentinel inspection consults no object store"
    );
    assert!(clean.value().is_clean());

    // The same holds when a sentinel is present.
    let head = run_git(root, &["rev-parse", "HEAD"]).trim().to_string();
    std::fs::write(root.join(".git/MERGE_HEAD"), format!("{head}\n")).expect("write MERGE_HEAD");
    let merging = egggit::operation_state::detect_repository_operation_state(&root.join(".git"));
    assert_eq!(merging.class(), GitReadConsistency::NoObjectGeneration);
    assert_eq!(merging.value().family(), egggit::OperationFamily::Merge);

    // Such a read cannot satisfy a generation-bound consumer: there is no
    // generation to be consistent with.
    let refusal = admit_generation_consistent(merging, None)
        .expect_err("a read with no object generation must be refused");
    assert_eq!(refusal, GitConsistencyRefusal::NoObjectGeneration);
    assert_eq!(refusal.as_str(), "no_object_generation");
}

//! C005 — the Git read-consistency fence's **runtime** half, as a standalone
//! integration target.
//!
//! C005's §6.4 fenced the security review's two diff reads through
//! `admit_generation_bound`. The **static** half of that contract belongs to
//! `scripts/check_git_consistency_consumers.py`, which can see a call site
//! this target cannot. This target owns the **runtime** half: that an
//! inadmissible read produces a *typed* error at the surface rather than
//! content whose class was never checked. The two halves fail differently — a
//! fence that admits everything still satisfies the static guard, and a
//! consumer that bypasses the fence still returns a plausible report — so
//! only a runtime assertion separates a fence that refuses *correctly* from
//! one that merely exists.
//!
//! Ownership: `plans/closure/linux-sandbox-admission-follow-up-corrective/005-status.md`
//! (§14, A9).

use std::path::{Path, PathBuf};

use codegg::git_consistency::{
    admit_generation_bound, live_read_capability, GitConsistencyRefusal, GitReadConsistency,
    GitReadFact,
};
use codegg::git_service::GitServiceError;
use codegg::security::workflow::report::{discover_targets_from_diff, SecurityReviewReadError};

// ── Fixtures ─────────────────────────────────────────────────────────────

/// Run a git command in `cwd` and fail loudly if it does not succeed.
fn git(args: &[&str], cwd: &Path) -> std::process::Output {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run a git fixture command");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// A real Git repository with one commit and one uncommitted change, so the
/// diff reads the security-review surface performs have something to read.
fn repository_with_a_change() -> PathBuf {
    let root = std::env::temp_dir().join(format!("codegg-c005-consistency-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create the repository directory");
    git(&["init", "--quiet", "--initial-branch=main"], &root);
    git(&["config", "user.email", "test@example.com"], &root);
    git(&["config", "user.name", "Test"], &root);
    std::fs::write(root.join("README.md"), "base\n").expect("write the base file");
    git(&["add", "."], &root);
    git(&["commit", "--quiet", "-m", "base"], &root);
    std::fs::write(root.join("README.md"), "changed\n").expect("write the change");
    root
}

/// A directory that is not a Git repository at all, so the surface's diff read
/// fails before it can be admitted.
fn directory_without_a_repository() -> PathBuf {
    let root = std::env::temp_dir().join(format!("codegg-c005-not-a-repo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create the directory");
    root
}

// ── 1. The real route admits ─────────────────────────────────────────────

/// A real repository's diff read is admitted, and the surface returns the
/// content it built.
///
/// This is the *admitting* half: a fence wired to refuse everything would
/// fail this. It does **not** prove the fence is *reached* — a surface that
/// bypassed the fence would return the same content — and does not claim to;
/// that is the static guard's property.
#[tokio::test]
async fn the_security_review_surface_admits_a_real_repositorys_diff() {
    let root = repository_with_a_change();

    let (targets, hunks, patches) = discover_targets_from_diff(&root, None)
        .await
        .expect("a real repository's diff read must be admitted by the fence");

    assert_eq!(
        targets.len(),
        1,
        "the changed file must become exactly one security-review target"
    );
    assert_eq!(
        targets[0].file_path,
        PathBuf::from("README.md"),
        "the target must name the file that actually changed"
    );
    assert!(
        !hunks.is_empty(),
        "the admitted diff must carry its hunks, not an empty report"
    );
    assert!(
        patches.contains_key(&PathBuf::from("README.md")),
        "the admitted diff must carry the real patch, got {patches:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

// ── 2. The refusal is typed ──────────────────────────────────────────────

/// An inadmissible read fact is refused, and the refusal reaches the caller
/// **typed**.
///
/// The capability is [`codegg::git_consistency::live_read_capability`] — the
/// production accessor, which returns `None` in every build — so this is the
/// production capability state, not a test-only posture.
///
/// The refusal must survive as [`SecurityReviewReadError::Consistency`]
/// carrying the refusal itself. Flattened into the `Read` variant or a bare
/// string, the caller would lose the one fact that matters: *the read was
/// not admissible*, as opposed to *Git did not answer*.
#[tokio::test]
async fn an_inadmissible_diff_read_is_refused_with_a_typed_error() {
    let refused = admit_generation_bound(
        GitReadFact::new("torn diff".to_string(), GitReadConsistency::LiveMutable),
        live_read_capability(),
    )
    .expect_err("a live-class read must not be admitted without the live-read capability");

    // The refusal is typed at the surface's boundary, not flattened.
    match &refused {
        GitServiceError::Consistency(refusal) => {
            assert!(
                matches!(
                    refusal,
                    GitConsistencyRefusal::LiveClassWithoutCapability { class }
                        if *class == GitReadConsistency::LiveMutable
                ),
                "expected a typed live-class refusal, got {refusal:?}"
            );
        }
        other => panic!("expected a consistency refusal, got {other:?}"),
    }

    // The same refusal, carried by the security-review surface's own error
    // type. This is the mapping the surface's `?` depends on.
    let surface_error: SecurityReviewReadError = refused.into();
    match &surface_error {
        SecurityReviewReadError::Consistency(refusal) => {
            assert_eq!(
                refusal.as_str(),
                "live_class_without_capability",
                "the refusal must keep its stable machine name"
            );
        }
        SecurityReviewReadError::Read(message) => {
            panic!("the refusal must stay typed, not become a read failure: {message}")
        }
    }

    // A read that observed no object generation is refused as its own kind of
    // refusal, with its own machine name.
    let generationless: SecurityReviewReadError = admit_generation_bound(
        GitReadFact::new(
            "no generation".to_string(),
            GitReadConsistency::NoObjectGeneration,
        ),
        live_read_capability(),
    )
    .expect_err("a read without an object generation must not be admitted")
    .into();
    match &generationless {
        SecurityReviewReadError::Consistency(refusal) => {
            assert_eq!(
                refusal.as_str(),
                "no_object_generation",
                "a generationless read must carry its own machine name"
            );
        }
        SecurityReviewReadError::Read(message) => {
            panic!("the refusal must stay typed, not become a read failure: {message}")
        }
    }
}

// ── 3. A read failure is not a consistency refusal ───────────────────────

/// A diff read that never produced a result is a **read** failure, and it is
/// distinguishable from a consistency refusal — the property a flattened
/// error would destroy. On the real path (a directory that is not a
/// repository) the read fails before anything can be admitted.
#[tokio::test]
async fn a_failed_diff_read_is_not_a_consistency_refusal() {
    let root = directory_without_a_repository();

    let failed = discover_targets_from_diff(&root, None)
        .await
        .expect_err("a directory that is not a repository cannot produce a diff read");

    match &failed {
        SecurityReviewReadError::Read(message) => {
            assert!(
                !message.is_empty(),
                "a read failure must carry the reason it failed"
            );
        }
        SecurityReviewReadError::Consistency(refusal) => {
            panic!("a failed read must not be reported as a consistency refusal: {refusal:?}")
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}

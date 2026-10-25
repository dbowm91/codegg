//! C008 — contained managed-worktree creation.
//!
//! The milestone's claim is narrow and is asserted in four places:
//!
//! 1. **The grant** names exactly the checkout, the daemon scratch and the
//!    **shared** administration root — the parent of every linked worktree's
//!    own administration directory — and the repository's Git directory is
//!    readable and immutable to the child.
//! 2. **The seam** turns a reserved worktree into a real linked worktree, and
//!    every failure path fails closed.
//! 3. **The contained child** cannot write outside its grant, even when the
//!    repository's own configuration is hostile.
//! 4. **Nothing else changed**: every other containment-requiring Git request
//!    keeps its classification.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// The shared helper tree is bound **publicly**. A private `mod common;`
// makes its `pub` items unreachable from this crate root — and
// `tests/common/secret_scan.rs` carries no `allow(dead_code)` of its own,
// so a private binding reports every one of its items as dead code under
// `-D warnings`. The families bind it the same way.
pub mod common;

use codegg::git_contained_worktree::{
    build_worktree_grant, obtained_write_root_is_present, ContainedGitWorktreeCreator,
    WorktreeChildPaths, WorktreeChildStep, WorktreeLaunchTiers,
};
// The native proof is the only consumer of the child-launch composition, and
// it is Linux-only, so the import carries the same gate. Without it the
// import is unused on every other platform, which is a clippy error.
#[cfg(target_os = "linux")]
use codegg::git_contained_worktree::compose_worktree_child_launch;
use codegg_core::worktree_service::{
    ContainedWorktreeCreator, ContainedWorktreeOutcome, ContainedWorktreeRequest,
    CreateWorktreeRequest, WorktreeService, WorktreeServiceError,
};
use codegg_core::worktree_service::{ManagedWorktreeState, WorktreeHealth};

/// The established opt-in for the native Linux M004/M005 proofs.
#[cfg(target_os = "linux")]
const NATIVE_OPT_IN: &str = "CODEGG_RUN_M005_OFFLINE_NATIVE";

// ── Fixtures ──────────────────────────────────────────────────────────────

fn git_binary() -> PathBuf {
    codegg::git_contained_worktree::resolve_git_binary().expect("a pinned Git executable")
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = std::process::Command::new(git_binary())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_git_output(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new(git_binary())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A repository with one commit that contains a file the hostile smudge filter
/// will match.
///
/// The `.txt` file and the `.gitattributes` that selects the filter are both
/// **committed**: a filter only runs for a path the checkout actually writes,
/// and the attributes that select it have to be in the tree being checked out.
fn init_repository(root: &Path) -> String {
    std::fs::create_dir_all(root).expect("repository directory");
    run_git(root, &["init", "--quiet", "--initial-branch=main"]);
    run_git(root, &["config", "user.email", "test@example.com"]);
    run_git(root, &["config", "user.name", "Test"]);
    run_git(root, &["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join("README.md"), "hello\n").expect("write README");
    std::fs::write(root.join("notes.txt"), "filter me\n").expect("write notes");
    std::fs::write(root.join(".gitattributes"), "*.txt filter=hostile\n")
        .expect("write attributes");
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "--quiet", "-m", "init"]);
    let head = std::process::Command::new(git_binary())
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .expect("git rev-parse");
    String::from_utf8_lossy(&head.stdout).trim().to_string()
}

fn create_request(repository_root: &Path) -> CreateWorktreeRequest {
    CreateWorktreeRequest {
        project_id: codegg_core::identity::ProjectId::new(),
        repository_id: codegg_core::identity::RepositoryId::new(),
        workspace_id: codegg_core::workspace::WorkspaceId::new_unchecked("ws-c008"),
        node_id: None,
        repository_root: repository_root.to_path_buf(),
        base_commit: None,
        base_ref: None,
        target_branch: None,
        base_path: None,
        owner_run_id: codegg_core::identity::AgentRunId::new(),
    }
}

/// The paths one contained worktree creation names.
///
/// The administration root is created first — the grant names it, so it has to
/// exist before the launch — and the checkout is created empty, because Git
/// refuses to create a worktree in a non-empty directory.
fn child_paths(
    repository: &Path,
    checkout: &Path,
    scratch: &Path,
    branch: &str,
    base_commit: &str,
) -> WorktreeChildPaths {
    let gitdir = repository
        .join(".git")
        .canonicalize()
        .expect("canonical gitdir");
    let administration_root = gitdir.join("worktrees");
    std::fs::create_dir_all(&administration_root).expect("administration directory");
    let administration_root = administration_root
        .canonicalize()
        .expect("canonical administration root");
    let administration_dir = administration_root.join(
        checkout
            .file_name()
            .expect("worktree name")
            .to_string_lossy()
            .to_string(),
    );
    WorktreeChildPaths {
        git_binary: git_binary(),
        gitdir,
        repository_root: repository.canonicalize().expect("canonical repository"),
        checkout: checkout.canonicalize().expect("canonical checkout"),
        scratch: scratch.canonicalize().expect("canonical scratch"),
        administration_root,
        administration_dir,
        branch: branch.to_string(),
        base_commit: base_commit.to_string(),
    }
}

// ── 1. The grant ──────────────────────────────────────────────────────────

/// The child's writable set is exactly the checkout, the daemon scratch and
/// the **shared** administration root — the parent of every linked worktree's
/// own administration directory — and the repository's Git directory is
/// readable and immutable.
#[test]
fn the_grant_names_only_the_shared_administration_root() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let checkout = workspace.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("checkout directory");
    let scratch = workspace.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let base = run_git_output(&repository, &["rev-parse", "HEAD"]);

    let paths = child_paths(
        &repository,
        &checkout,
        &scratch,
        "codegg/worktree/c008-grant",
        &base,
    );

    let spec = build_worktree_grant(
        &paths,
        WorktreeChildStep::Create,
        WorktreeLaunchTiers::PRODUCTION,
    )
    .expect("the contained worktree grant");

    let gitdir = paths.gitdir.clone();
    let repository_root = paths.repository_root.clone();
    let administration_root = paths.administration_root.clone();

    // The device nodes are writable under every profile — they hold no
    // persistent state — so they are not part of the grant. Everything else
    // is.
    let devices: Vec<PathBuf> = codegg::security::sandbox::policy::device_write_allowances()
        .into_iter()
        .filter(|path| path.exists())
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    let granted = spec
        .write_paths
        .iter()
        .filter(|write| !devices.contains(write))
        .collect::<Vec<_>>();
    assert_eq!(
        granted.len(),
        3,
        "the child was granted writable roots beyond the grant: {:?}",
        spec.write_paths
    );
    for expected in [&paths.checkout, &paths.scratch, &administration_root] {
        assert!(
            spec.write_paths.contains(expected),
            "the child was not granted {expected:?}",
        );
    }

    // The repository's Git directory is readable and not writable.
    assert!(
        spec.read_paths.contains(&gitdir),
        "the child cannot read the repository's Git directory"
    );
    assert!(
        !spec.write_paths.contains(&gitdir),
        "the repository's Git directory was granted writable"
    );

    // The source working tree is not the child's to write.
    assert!(
        !spec.write_paths.contains(&repository_root),
        "the source working tree was granted writable"
    );

    // Nothing under the Git directory is writable except the shared
    // administration root. This is what makes branch creation by the child
    // impossible rather than merely unintended: the child's own `refs` live
    // under `<common-gitdir>/refs`, not under the administration root.
    for write in &spec.write_paths {
        if write.starts_with(&gitdir) {
            assert_eq!(
                write, &administration_root,
                "the child was granted {write:?} inside the repository's Git directory"
            );
        }
    }

    // The only `.git` the child may create is the checkout's own pointer.
    assert_eq!(
        spec.expected_created_git_pointers,
        vec![paths.checkout.join(".git")],
        "the child may create a Git pointer beyond the checkout's own"
    );

    // Both tiers are requested.
    assert!(spec.linux_process_isolation);
    assert!(spec.linux_offline_network);

    // The argv is exactly one shape, and it never names a ref.
    assert!(spec.args.contains(&"worktree".to_string()));
    assert!(spec.args.contains(&"add".to_string()));
    assert!(spec.args.contains(&"--detach".to_string()));
    assert!(
        !spec.args.iter().any(|arg| arg == "-b" || arg == "-B"),
        "the argv creates a branch: {:?}",
        spec.args
    );
    assert!(
        !spec.args.iter().any(|arg| arg.contains("refs/heads/")),
        "the argv names a ref: {:?}",
        spec.args
    );

    // The exemption from the "hide a `.git` that does not exist yet" rule is
    // narrow: it names the top-level `.git` of a granted write root and
    // nothing else. A pointer beyond a write root, and a nested `.git`, are
    // both refused when the launch description is built.
    let beyond = workspace.path().join("elsewhere");
    std::fs::create_dir_all(&beyond).expect("beyond directory");
    let nested = paths.checkout.join("nested");
    std::fs::create_dir_all(&nested).expect("nested directory");
    for pointer in [beyond.join(".git"), nested.join(".git")] {
        let error = codegg::security::sandbox::SandboxConfig::new()
            .with_enabled(true)
            .with_mode(codegg::security::sandbox::SandboxMode::WorkspaceWrite)
            .with_allowed_paths(vec![paths.checkout.display().to_string()])
            .with_expected_created_git_pointers(vec![pointer.display().to_string()])
            .launch_spec(
                git_binary(),
                &["--version".to_string()],
                Some(&paths.checkout),
            )
            .expect_err("a pointer outside a write root must be refused");
        assert!(
            error
                .to_string()
                .contains("not the top level of a granted write root"),
            "unexpected refusal: {error}"
        );
    }
}

/// The grant's writable roots under the repository's Git directory are exactly
/// **one** path, and it is the shared administration root.
///
/// The per-path equality check above would still pass if the grant added a
/// *second* root under the Git directory — `hooks`, say — because it only
/// compares the roots it knows about. The **count** is what catches that, and
/// the count is the property ADR-0048 clause 3 states and
/// `scripts/check_contained_worktree_grant.py` enforces statically.
#[test]
fn the_grant_writes_exactly_one_path_under_the_git_directory() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let checkout = workspace.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("checkout directory");
    let scratch = workspace.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let base = run_git_output(&repository, &["rev-parse", "HEAD"]);

    let paths = child_paths(
        &repository,
        &checkout,
        &scratch,
        "codegg/worktree/c009-git-directory-root",
        &base,
    );

    let spec = build_worktree_grant(
        &paths,
        WorktreeChildStep::Create,
        WorktreeLaunchTiers::PRODUCTION,
    )
    .expect("the contained worktree grant");

    let gitdir = paths.gitdir.clone();
    let administration_root = paths.administration_root.clone();
    let administration_dir = paths.administration_dir.clone();

    // The device nodes are writable under every profile — they hold no
    // persistent state — so they are not part of the grant.
    let devices: Vec<PathBuf> = codegg::security::sandbox::policy::device_write_allowances()
        .into_iter()
        .filter(|path| path.exists())
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    let under_gitdir = spec
        .write_paths
        .iter()
        .filter(|write| !devices.contains(write))
        .filter(|write| write.starts_with(&gitdir))
        .collect::<Vec<_>>();

    assert_eq!(
        under_gitdir.len(),
        1,
        "the grant produced {} writable roots under the repository's Git directory, \
         expected exactly one: {under_gitdir:?}",
        under_gitdir.len()
    );
    assert_eq!(
        under_gitdir[0], &administration_root,
        "the grant's only writable root under the repository's Git directory is not the \
         shared administration root"
    );

    // The granted root is the **shared** one: it is the parent of the linked
    // worktree's own administration directory, and it is not that directory.
    assert!(
        administration_dir.starts_with(&administration_root),
        "the linked worktree's own administration directory is not under the shared \
         administration root: {administration_dir:?} vs {administration_root:?}"
    );
    assert_ne!(
        administration_dir, administration_root,
        "the shared administration root and the linked worktree's own administration \
         directory are the same path"
    );
}

/// The obtained-root check compares whole paths, not substrings.
///
/// ADR-0048 clause 4 is "containment is asserted, not requested": the launch's
/// *obtained* scope is the evidence, and the check that reads it must not be
/// satisfied by a root that merely shares a prefix with the required one.
/// `/repo/.git/worktrees-old` contains `/repo/.git/worktrees`, so a substring
/// test accepted a root the child did not obtain.
///
/// The comparison is by **membership**, so a backend that reports extra roots
/// alongside the required one still passes.
#[test]
fn the_obtained_root_check_compares_paths_and_not_substrings() {
    let required = Path::new("/repo/.git/worktrees");

    // A prefix-sharing root is not the required root.
    let prefix_sharing =
        vec!["writable grant roots: /repo/.git/worktrees-old, /repo/checkout".to_string()];
    assert!(
        !obtained_write_root_is_present(&prefix_sharing, required),
        "a root that merely shares a prefix satisfied the obtained-root check"
    );

    // The exact path passes.
    let exact = vec!["writable grant roots: /repo/.git/worktrees, /repo/checkout".to_string()];
    assert!(
        obtained_write_root_is_present(&exact, required),
        "the exact administration root did not satisfy the obtained-root check"
    );

    // Order does not matter, and extra roots do not fail the check.
    let reordered = vec![
        "writable grant roots: /repo/checkout, /repo/.git/worktrees, /repo/scratch".to_string(),
    ];
    assert!(
        obtained_write_root_is_present(&reordered, required),
        "the obtained-root check became order-sensitive"
    );

    // A launch that obtained no writable root at all does not pass.
    let empty: Vec<String> = Vec::new();
    assert!(
        !obtained_write_root_is_present(&empty, required),
        "a launch that obtained no writable root satisfied the obtained-root check"
    );
    let read_only = vec!["read-only grant roots: /repo/.git".to_string()];
    assert!(
        !obtained_write_root_is_present(&read_only, required),
        "a read-only root satisfied the obtained-root check"
    );
}

// ── 2. The seam ───────────────────────────────────────────────────────────

/// What the test double does when the worktree service asks it to create a
/// checkout.
#[derive(Clone, Copy)]
enum FakeMode {
    /// Produce a real linked worktree, the way the contained child does.
    LinkedWorktree,
    /// Produce a **nested repository** instead of a linked-worktree pointer —
    /// the shape the grant's exemption must not accept.
    NestedRepository,
    /// Refuse.
    Refuse,
}

struct FakeCreator {
    mode: FakeMode,
    calls: Arc<Mutex<Vec<ContainedWorktreeRequest>>>,
}

#[async_trait::async_trait]
impl ContainedWorktreeCreator for FakeCreator {
    async fn create(
        &self,
        request: &ContainedWorktreeRequest,
    ) -> Result<ContainedWorktreeOutcome, WorktreeServiceError> {
        self.calls
            .lock()
            .expect("creator calls")
            .push(request.clone());
        match self.mode {
            FakeMode::Refuse => Err(WorktreeServiceError::Git(
                "the contained child refused the creation".into(),
            )),
            FakeMode::NestedRepository => {
                std::fs::create_dir_all(request.worktree_path.join(".git"))
                    .expect("nested repository");
                std::fs::write(
                    request.worktree_path.join(".git").join("HEAD"),
                    "ref: refs/heads/main\n",
                )
                .expect("nested HEAD");
                Ok(ContainedWorktreeOutcome {
                    administration_dir: request.repository_gitdir.join("worktrees").join("nested"),
                    backend: "test-double".into(),
                    abi: None,
                    network_enforced: true,
                })
            }
            FakeMode::LinkedWorktree => {
                // The real linked worktree, shared with the suites that need
                // a checkout Git itself recognises.
                common::linked_worktree::LinkedWorktreeCreator::new()
                    .create(request)
                    .await
            }
        }
    }
}

fn service_with_creator(
    managed_root: &Path,
    mode: FakeMode,
    calls: &Arc<Mutex<Vec<ContainedWorktreeRequest>>>,
) -> Arc<WorktreeService> {
    WorktreeService::memory(
        managed_root.to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    )
    .with_contained_creator(Arc::new(FakeCreator {
        mode,
        calls: calls.clone(),
    }))
}

/// The seam is authoritative, and every failure path fails closed.
///
/// A configured creator is used, the checkout is real, and Git reports it on
/// the recorded branch. A creator that refuses orphans the reservation with a
/// Git-error health. A creator that plants a nested repository is rejected by
/// the post-create verification. Without a creator the service still fails
/// closed with the Git policy's own typed refusal — there is no uncontained
/// fallback.
#[tokio::test]
async fn the_seam_creates_a_real_checkout_and_fails_closed() {
    // ── The successful path ──────────────────────────────────────────────
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let managed_root = tempfile::tempdir().expect("managed root");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let service = service_with_creator(managed_root.path(), FakeMode::LinkedWorktree, &calls);

    let reserved = service
        .reserve(&create_request(&repository))
        .await
        .expect("reserve the worktree");
    let record = service
        .create_reserved(&reserved.worktree_id)
        .await
        .expect("the contained creator produces a checkout");

    assert_eq!(record.state, ManagedWorktreeState::Ready);
    assert_eq!(record.health, WorktreeHealth::Clean);
    assert!(record.path.join("README.md").is_file());
    assert!(record.path.join(".git").is_file());

    // The recorded calls are copied out of the lock before the next
    // `await`, so no guard is ever held across a suspension point.
    let calls = calls.lock().expect("creator calls").clone();
    assert_eq!(calls.len(), 1, "the creator was not called exactly once");
    let call = &calls[0];
    assert_eq!(call.branch, record.branch.clone().unwrap_or_default());
    assert_eq!(call.worktree_path, record.path);
    assert_eq!(
        call.repository_gitdir,
        repository
            .join(".git")
            .canonicalize()
            .expect("canonical gitdir")
    );

    // ── A refusing creator orphans the reservation ───────────────────────
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let managed_root = tempfile::tempdir().expect("managed root");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let service = service_with_creator(managed_root.path(), FakeMode::Refuse, &calls);

    let error = service
        .create(&create_request(&repository))
        .await
        .expect_err("a refusing creator must not produce a checkout");
    assert!(
        error.to_string().contains("refused the creation"),
        "unexpected error: {error}"
    );
    let records = service
        .list(codegg_core::worktree_service::WorktreeQuery::default())
        .await
        .expect("list worktrees");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, ManagedWorktreeState::Orphaned);
    assert_eq!(records[0].health, WorktreeHealth::GitError);

    // ── A nested repository is not a checkout ─────────────────────────────
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let managed_root = tempfile::tempdir().expect("managed root");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let service = service_with_creator(managed_root.path(), FakeMode::NestedRepository, &calls);

    let error = service
        .create(&create_request(&repository))
        .await
        .expect_err("a nested repository must not be accepted");
    assert!(
        error.to_string().contains("post-create verification"),
        "unexpected error: {error}"
    );
    let records = service
        .list(codegg_core::worktree_service::WorktreeQuery::default())
        .await
        .expect("list worktrees");
    assert_eq!(records[0].state, ManagedWorktreeState::Orphaned);
    assert_eq!(records[0].health, WorktreeHealth::GitError);

    // ── Without a creator the service still fails closed ─────────────────
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let managed_root = tempfile::tempdir().expect("managed root");
    let service = WorktreeService::memory(
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );

    let error = service
        .create(&create_request(&repository))
        .await
        .expect_err("without a contained creator the creation must fail closed");
    assert!(
        error.to_string().contains("Git backend unavailable"),
        "unexpected error: {error}"
    );
    assert!(error.to_string().contains("CleanSmudgeFilter"));
}

// ── 3. The contained child ────────────────────────────────────────────────

/// Write an executable script the child will run.
// The native proof is the only consumer, and it is Linux-only, so the helper
// carries the same gate. Without it the helper is unused on every other
// platform, which is a clippy error.
#[cfg(target_os = "linux")]
fn write_script(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("make the script executable");
    }
}

/// The child cannot write outside its grant, even when the repository's own
/// configuration is hostile.
///
/// The repository declares a smudge filter and a `post-checkout` hook. Both
/// are the child's own untrusted configuration, and both try to write a **ref**
/// into the repository's Git directory — which is exactly what ADR-0048
/// clause 2 forbids. The grant binds the Git directory read-only, so both
/// attempts must fail on the host.
///
/// The negative assertions alone would prove nothing: they would also hold if
/// the hostile surfaces never ran. So each surface also writes a **positive
/// control** into the daemon scratch, which *is* in the grant. If the control
/// is missing, the containment result is vacuous and the test fails.
///
/// The launch runs the **production** grant composition and the **production**
/// process dispatch. Only the two enforcement dimensions this host cannot
/// obtain are left out — the M004 process/IPC boundary needs Landlock ABI 6
/// (Linux 6.12; this host is 6.8) — so the proof here is of the filesystem
/// grant, which is the dimension this milestone is about. The production
/// tiers are requested by the production creator and are proved on a host that
/// can obtain them.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_contained_child_cannot_write_outside_its_grant() {
    if std::env::var(NATIVE_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {NATIVE_OPT_IN}=1 to run the native contained-worktree proof");
        return;
    }

    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let gitdir = repository
        .join(".git")
        .canonicalize()
        .expect("canonical gitdir");
    let scratch = workspace.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let checkout = workspace.path().join("checkout");
    std::fs::create_dir(&checkout).expect("checkout directory");
    let administration_root = gitdir.join("worktrees");
    std::fs::create_dir_all(&administration_root).expect("administration directory");

    // The two hostile surfaces. Each writes its positive control into the
    // scratch — inside the grant — and then tries to write a ref into the
    // Git directory — outside the grant.
    //
    // The refused write is tolerated (`|| true`) so that the *operation*
    // still completes: the claim under test is that the escape does not
    // reach the host, not that the hostile code aborts the checkout. The
    // marker is the evidence either way — it is absent from the host.
    let filter_ran = scratch.join("filter-ran");
    let hook_ran = scratch.join("hook-ran");
    let filter_ref = gitdir.join("refs/heads/hostile-filter");
    let hook_ref = gitdir.join("refs/heads/hostile-hook");

    let filter_script = scratch.join("hostile-filter.sh");
    write_script(
        &filter_script,
        &format!(
            "#!/bin/sh\necho ran > {filter}\necho pwned > {ref} || true\ncat\n",
            filter = filter_ran.display(),
            ref = filter_ref.display()
        ),
    );
    let hooks = scratch.join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks directory");
    write_script(
        &hooks.join("post-checkout"),
        &format!(
            "#!/bin/sh\necho ran > {ran}\necho pwned > {ref} || true\n",
            ran = hook_ran.display(),
            ref = hook_ref.display()
        ),
    );

    run_git(
        &repository,
        &[
            "config",
            "filter.hostile.smudge",
            &filter_script.display().to_string(),
        ],
    );
    run_git(&repository, &["config", "filter.hostile.required", "true"]);
    run_git(
        &repository,
        &["config", "core.hooksPath", &hooks.display().to_string()],
    );

    // The branch is the daemon's, and it exists before the child runs.
    let branch = "codegg/worktree/c008-native";
    let base = run_git_output(&repository, &["rev-parse", "HEAD"]);
    run_git(&repository, &["branch", branch, &base]);

    let tiers = if codegg::security::sandbox::landlock::probe_process_scopes().is_ok() {
        WorktreeLaunchTiers::PRODUCTION
    } else {
        eprintln!(
            "note: this host cannot obtain the M004 process/IPC tier (Landlock ABI 6); \
             proving the filesystem grant alone"
        );
        WorktreeLaunchTiers::FILESYSTEM_ONLY
    };

    let paths = child_paths(&repository, &checkout, &scratch, branch, &base);

    // Both steps of the contained operation, through the production dispatch.
    for step in [WorktreeChildStep::Create, WorktreeChildStep::AttachBranch] {
        let launch = compose_worktree_child_launch(&paths, step, tiers)
            .expect("compose the contained worktree launch");
        let mut request = codegg::managed_process::ManagedProcessRequest::new(
            launch.argv,
            launch.cwd,
            codegg::managed_process::ProcessProvenance::new("c008-native", branch),
        );
        request.timeout = Some(std::time::Duration::from_secs(60));
        request.environment_policy = launch.environment;
        request.sandbox = codegg::managed_process::SandboxRequest::Required(launch.spec);

        let result = codegg::managed_process::ManagedProcessService::run(request)
            .await
            .expect("the contained child runs");
        assert!(
            result.exit_status.success(),
            "the contained child failed at {step:?}: {}",
            String::from_utf8_lossy(&result.stderr.head).trim()
        );
    }

    // The checkout is real, and its `.git` is a pointer file.
    assert!(checkout.join("README.md").is_file());
    assert!(checkout.join("notes.txt").is_file());
    let pointer = std::fs::read_to_string(checkout.join(".git")).expect("pointer");
    assert!(
        pointer.starts_with("gitdir: "),
        "the checkout's '.git' is not a linked-worktree pointer: {pointer}"
    );

    // The positive control: the hostile filter really ran. The hook is the
    // opposite — the child's `core.hooksPath` is pinned, so it never runs.
    assert!(
        filter_ran.is_file(),
        "the hostile filter never ran, so the containment result is vacuous"
    );
    assert!(
        !hook_ran.is_file(),
        "the repository's post-checkout hook ran inside the containment, so the \
         hooks-path pin did not take effect"
    );

    // The negatives: neither surface wrote a ref.
    assert!(
        !filter_ref.exists(),
        "the hostile filter wrote a ref into the repository's Git directory"
    );
    assert!(
        !hook_ref.exists(),
        "the hostile hook wrote a ref into the repository's Git directory"
    );

    // The worktree is on the daemon's branch, and the branch still points
    // where the daemon put it. The child wrote no ref.
    let listed = run_git_output(&gitdir, &["worktree", "list", "--porcelain"]);
    assert!(
        listed.contains(&format!("branch refs/heads/{branch}")),
        "the worktree is not attached to the daemon's branch: {listed}"
    );
    assert_eq!(
        run_git_output(&gitdir, &["rev-parse", &format!("refs/heads/{branch}")]),
        base,
        "the child moved the daemon's branch"
    );
    let refs = run_git_output(
        &gitdir,
        &["for-each-ref", "--format=%(refname)", "refs/heads/"],
    );
    assert_eq!(
        refs.lines().collect::<Vec<_>>(),
        vec!["refs/heads/codegg/worktree/c008-native", "refs/heads/main"],
        "the child created or moved a branch: {refs}"
    );
}

// ── 4. The Git execution policy ───────────────────────────────────────────

/// C008 adds a **contained route** for one operation. It does not relax the
/// Git execution policy for anything.
///
/// Every other containment-requiring request keeps its classification, and
/// `git worktree add` keeps its classification too: the contained route is a
/// separate, explicitly named path, not a change to what the policy allows
/// on the ordinary process boundary.
#[test]
fn the_git_execution_policy_is_unchanged() {
    let still_gated = [
        vec!["git", "add", "README.md"],
        vec!["git", "merge", "feature"],
        vec!["git", "cherry-pick", "abc123"],
        vec!["git", "rebase", "main"],
        vec!["git", "stash"],
        vec!["git", "am", "patch"],
        vec!["git", "bisect", "start"],
        vec!["git", "push", "origin", "main"],
        vec!["git", "pull"],
        vec!["git", "worktree", "add", "/tmp/x", "feature"],
    ];
    for argv in still_gated {
        let argv: Vec<String> = argv.into_iter().map(str::to_string).collect();
        assert!(
            !matches!(
                egggit::process::classify_git_request(&argv),
                egggit::GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
            ),
            "{argv:?} became available on the ordinary process boundary"
        );
    }

    // The shapes that were already trusted keep their own classification.
    for argv in [
        vec!["git", "worktree", "list"],
        vec!["git", "commit", "-m", "message"],
    ] {
        let argv: Vec<String> = argv.into_iter().map(str::to_string).collect();
        assert!(
            matches!(
                egggit::process::classify_git_request(&argv),
                egggit::GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
            ),
            "{argv:?} changed classification"
        );
    }
}

/// The creator resolves the paths it names — including the two it creates
/// itself — before it launches anything.
///
/// The checkout and the administration directory are both created *after* the
/// grant is composed, so a resolution that demanded every component to exist
/// would fail before the child ever ran. That is the defect the hosted lane
/// caught, and this is its regression: the creator must reach the launch,
/// whatever this host can or cannot enforce.
///
/// The test does not care whether the launch then succeeds. A host that cannot
/// obtain the M004 tiers must fail at the **containment**, not at the path.
#[tokio::test]
async fn the_creator_reaches_the_launch_instead_of_failing_on_its_own_paths() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let gitdir = repository
        .join(".git")
        .canonicalize()
        .expect("canonical gitdir");
    let scratch = workspace.path().join("scratch");
    let managed_root = workspace.path().join("managed");
    std::fs::create_dir_all(&managed_root).expect("managed root");

    // The shape the service hands the creator: the parent exists and the
    // leaf does not, because the daemon creates the leaf.
    let checkout = managed_root.join("wt-regression");
    assert!(!checkout.exists());

    let creator = ContainedGitWorktreeCreator::new(git_binary(), scratch);
    let request = ContainedWorktreeRequest {
        repository_root: repository.canonicalize().expect("canonical repository"),
        repository_gitdir: gitdir.clone(),
        worktree_path: checkout.clone(),
        branch: "codegg/worktree/c008-regression".to_string(),
        base_commit: run_git_output(&repository, &["rev-parse", "HEAD"]),
        timeout: std::time::Duration::from_secs(60),
    };

    match creator.create(&request).await {
        Ok(outcome) => {
            // The host obtained the tiers: the worktree really exists.
            assert!(checkout.join(".git").is_file());
            assert!(outcome.administration_dir.starts_with(&gitdir));
        }
        Err(error) => {
            let message = error.to_string();
            assert!(
                !message.contains("could not be resolved"),
                "the creator failed to resolve a path it creates itself: {message}"
            );
        }
    }
}

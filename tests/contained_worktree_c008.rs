//! C008 — contained managed-worktree creation.
//!
//! ADR-0048 (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`)
//! grants exactly one operation shape: `git worktree add <path>
//! <existing-branch>` inside an OS-contained child whose writable grant is
//! the linked worktree's own administration directory and nothing else.
//!
//! These tests cover three different things, deliberately kept apart:
//!
//! * **The grant** is asserted structurally, on any host. The composition is
//!   the same whether or not this machine can enforce it.
//! * **The seam** is exercised with a test double that produces a real
//!   checkout, so the worktree service's own contract is tested without
//!   needing a qualified host.
//! * **The native proof** — the real contained child, a hostile repository
//!   configuration, and the obtained enforcement — runs only on a qualified
//!   Linux host, behind the established opt-in.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use codegg::security::sandbox::{SandboxConfig, SandboxMode};
use codegg_core::worktree_service::{
    ContainedWorktreeCreator, ContainedWorktreeOutcome, ContainedWorktreeRequest,
    CreateWorktreeRequest, ManagedWorktreeState, WorktreeHealth, WorktreeService,
    WorktreeServiceError,
};

/// The established opt-in for the native Linux M004/M005 proofs.
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

/// A repository with one commit, returning the base commit.
fn init_repository(root: &Path) -> String {
    std::fs::create_dir_all(root).expect("repository directory");
    run_git(root, &["init", "--quiet", "--initial-branch=main"]);
    run_git(root, &["config", "user.email", "test@example.com"]);
    run_git(root, &["config", "user.name", "Test"]);
    run_git(root, &["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join("README.md"), "hello\n").expect("write README");
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

// ── The grant ─────────────────────────────────────────────────────────────

/// The child's writable set is exactly the checkout, the daemon scratch and
/// the linked worktree's own administration directory — and the repository's
/// Git directory is readable and immutable.
#[test]
fn the_grant_names_only_the_administration_directory() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let gitdir = repository.join(".git");
    let checkout = workspace.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("checkout directory");
    let scratch = workspace.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let administration = gitdir.join("worktrees");
    std::fs::create_dir_all(&administration).expect("administration directory");

    let spec = codegg::git_contained_worktree::build_worktree_grant(
        &git_binary(),
        &gitdir,
        &repository,
        &checkout,
        &scratch,
        &administration,
        "feature",
    )
    .expect("the contained worktree grant");

    let gitdir = gitdir.canonicalize().expect("canonical gitdir");
    let repository = repository.canonicalize().expect("canonical repository");
    let checkout = checkout.canonicalize().expect("canonical checkout");
    let scratch = scratch.canonicalize().expect("canonical scratch");
    let administration = administration
        .canonicalize()
        .expect("canonical administration");

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
    for expected in [&checkout, &scratch, &administration] {
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
        !spec.write_paths.contains(&repository),
        "the source working tree was granted writable"
    );

    // Nothing under the Git directory is writable except the administration
    // directory. This is what makes branch creation by the child impossible
    // rather than merely unintended.
    for write in &spec.write_paths {
        if write.starts_with(&gitdir) {
            assert_eq!(
                write, &administration,
                "the child was granted {write:?} inside the repository's Git directory"
            );
        }
    }

    // The only `.git` the child may create is the checkout's own pointer.
    assert_eq!(
        spec.expected_created_git_pointers,
        vec![checkout.join(".git")],
        "the child may create a Git pointer beyond the checkout's own"
    );

    // Both tiers are requested.
    assert!(spec.linux_process_isolation);
    assert!(spec.linux_offline_network);

    // The argv is exactly one shape, and it never creates a branch.
    assert!(
        spec.args.windows(2).all(|pair| pair != ["-b", "feature"]),
        "the argv creates a branch: {:?}",
        spec.args
    );
    assert!(spec.args.contains(&"worktree".to_string()));
    assert!(spec.args.contains(&"add".to_string()));
    assert!(!spec.args.iter().any(|arg| arg == "-b" || arg == "-B"));
}

/// The exemption from the "hide a `.git` that does not exist yet" rule is
/// narrow: it names the top-level `.git` of a granted write root and nothing
/// else.
#[test]
fn a_git_pointer_beyond_a_write_root_is_refused() {
    let workspace = tempfile::tempdir().expect("workspace");
    let checkout = workspace.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("checkout directory");
    let elsewhere = workspace.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere directory");

    // A pointer that is not a write root's own `.git` is refused.
    let error = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(vec![checkout.display().to_string()])
        .with_expected_created_git_pointers(vec![elsewhere.join(".git").display().to_string()])
        .launch_spec(git_binary(), &["--version".to_string()], Some(&checkout))
        .expect_err("a pointer outside a write root must be refused");
    assert!(
        error
            .to_string()
            .contains("not the top level of a granted write root"),
        "unexpected refusal: {error}"
    );

    // A nested `.git` is refused: the child may only write the pointer of a
    // checkout it was granted, never one inside a directory it controls.
    std::fs::create_dir_all(checkout.join("nested")).expect("nested directory");
    let error = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(vec![checkout.display().to_string()])
        .with_expected_created_git_pointers(vec![checkout
            .join("nested")
            .join(".git")
            .display()
            .to_string()])
        .launch_spec(git_binary(), &["--version".to_string()], Some(&checkout))
        .expect_err("a nested Git pointer must be refused");
    assert!(
        error
            .to_string()
            .contains("not the top level of a granted write root"),
        "unexpected refusal: {error}"
    );
}

// ── The seam ──────────────────────────────────────────────────────────────

/// What the test double does when the worktree service asks it to create a
/// checkout.
#[derive(Clone)]
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
                // The branch is the daemon's: it is created before the child
                // runs, so the child attaches to an existing branch.
                run_git(
                    &request.repository_root,
                    &[
                        "branch",
                        request.branch.as_str(),
                        request.base_commit.as_str(),
                    ],
                );
                run_git(
                    &request.repository_gitdir,
                    &[
                        "--git-dir",
                        request.repository_gitdir.to_str().expect("gitdir"),
                        "worktree",
                        "add",
                        request.worktree_path.to_str().expect("worktree path"),
                        request.branch.as_str(),
                    ],
                );
                Ok(ContainedWorktreeOutcome {
                    administration_dir: request.repository_gitdir.join("worktrees").join(
                        request
                            .worktree_path
                            .file_name()
                            .expect("worktree name")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    backend: "test-double".into(),
                    abi: None,
                    network_enforced: true,
                })
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

/// A configured creator is authoritative: the worktree service uses it, the
/// checkout is real, and Git reports it on the recorded branch.
#[tokio::test]
async fn the_configured_creator_produces_the_checkout() {
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

    let calls = calls.lock().expect("creator calls");
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
}

/// A failing creator is final: the reservation is orphaned with a Git-error
/// health and no checkout is reported as created.
#[tokio::test]
async fn a_failing_creator_orphans_the_reservation() {
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
}

/// A checkout whose `.git` is a **directory** rather than a pointer file is
/// not accepted. This is the property that makes the grant's exemption safe
/// to grant: the child may create the pointer, but it may not plant a
/// repository of its own.
#[tokio::test]
async fn a_nested_repository_is_not_accepted_as_a_checkout() {
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
}

/// Without a configured creator the service still fails closed with the Git
/// policy's own typed refusal. There is no uncontained fallback.
#[tokio::test]
async fn a_service_without_a_creator_still_fails_closed() {
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

/// The compiled mount view is the actual enforcement, so the grant is
/// checked against it rather than against the configuration that produced
/// it.
///
/// This proof does not need a qualified host: it compiles the Bubblewrap
/// argument list and reads the binds out of it. What it shows is the shape
/// of the containment — the repository's Git directory bound read-only, the
/// administration directory bound writable **after** it (so it overlays),
/// and the checkout's `.git` pointer not hidden.
#[cfg(target_os = "linux")]
#[test]
fn the_compiled_mount_view_grants_only_the_administration_directory() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);
    let gitdir = repository.join(".git");
    let checkout = workspace.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("checkout directory");
    let scratch = workspace.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let administration = gitdir.join("worktrees");
    std::fs::create_dir_all(&administration).expect("administration directory");

    let spec = codegg::git_contained_worktree::build_worktree_grant(
        &git_binary(),
        &gitdir,
        &repository,
        &checkout,
        &scratch,
        &administration,
        "feature",
    )
    .expect("the contained worktree grant");

    let (args, _fds) =
        codegg::security::sandbox::linux_mount_view::compile(&spec, &gitdir, |_| Ok(256))
            .expect("compile the protected mount view");

    // The binds, in the order the child will apply them.
    let binds: Vec<(bool, String)> = args
        .windows(3)
        .filter(|window| window[0] == "--bind-fd" || window[0] == "--ro-bind-fd")
        .map(|window| (window[0] == "--bind-fd", window[2].clone()))
        .collect();

    let gitdir = gitdir
        .canonicalize()
        .expect("canonical gitdir")
        .display()
        .to_string();
    let administration = administration
        .canonicalize()
        .expect("canonical administration")
        .display()
        .to_string();
    let checkout = checkout
        .canonicalize()
        .expect("canonical checkout")
        .display()
        .to_string();

    let gitdir_bind = binds
        .iter()
        .position(|(_, destination)| *destination == gitdir)
        .expect("the Git directory is bound");
    let administration_bind = binds
        .iter()
        .position(|(_, destination)| *destination == administration)
        .expect("the administration directory is bound");

    // The Git directory is read-only, and the administration directory is
    // bound writable **after** it, so the writable bind overlays the
    // read-only one.
    assert!(
        !binds[gitdir_bind].0,
        "the repository's Git directory is bound writable"
    );
    assert!(
        binds[administration_bind].0,
        "the administration directory is bound read-only"
    );
    assert!(
        administration_bind > gitdir_bind,
        "the administration directory's writable bind must come after the Git directory's read-only bind"
    );

    // The checkout is writable, and its `.git` pointer is not hidden: the
    // child has to create that pointer, because Git refuses to create a
    // worktree in a non-empty directory.
    assert!(
        binds
            .iter()
            .any(|(writable, destination)| *writable && *destination == checkout),
        "the checkout is not bound writable"
    );
    let pointer = format!("{checkout}/.git");
    let hidden_pointer = args.windows(2).any(|window| {
        (window[0] == "--tmpfs" || window[0] == "--remount-ro" || window[0] == "--ro-bind-data")
            && window[1] == pointer
    });
    assert!(
        !hidden_pointer,
        "the checkout's '.git' pointer is hidden from the child: {args:?}"
    );

    // The mount view's own rule still holds everywhere else: a `.git` that
    // does not exist yet inside a writable root is hidden, so the child
    // cannot plant a repository of its own.
    let scratch_git = format!("{}/.git", scratch.display());
    let administration_git = format!("{administration}/.git");
    for hidden_path in [&scratch_git, &administration_git] {
        assert!(
            args.windows(2).any(|window| {
                (window[0] == "--tmpfs" || window[0] == "--remount-ro") && window[1] == *hidden_path
            }),
            "{hidden_path} should still be hidden from the child"
        );
    }
}

// ── The native proof ──────────────────────────────────────────────────────

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

/// The real contained child, on a qualified Linux host.
///
/// The repository's own configuration defines a smudge filter and a
/// `post-checkout` hook that each try to write **outside** the child's
/// grant. The filter also writes a marker **inside** the grant, so the test
/// can tell "the filter never ran" apart from "the filter ran and was
/// contained".
///
/// The claim is narrow and the test states it: the host is unaffected. The
/// checkout's *contents* are untrusted content, and this test does not claim
/// otherwise.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_contained_worktree_is_created_and_a_hostile_configuration_is_contained() {
    if std::env::var(NATIVE_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {NATIVE_OPT_IN}=1 to run the native contained-worktree proof");
        return;
    }
    if let Err(reason) = codegg::security::sandbox::landlock::probe_process_scopes() {
        panic!("the contained worktree proof requires qualified M004 process/IPC setup: {reason}");
    }

    let workspace = tempfile::tempdir().expect("workspace");
    let repository = workspace.path().join("repo");
    init_repository(&repository);

    // The hostile configuration. Both surfaces try to write outside the
    // grant; the filter also writes inside it as a positive control.
    let escaped_marker = workspace.path().join("escaped-marker");
    let filter_marker = repository.join("filter-ran");
    let hook_marker = workspace.path().join("hook-escaped-marker");
    let filter = format!(
        "sh -c 'echo ran > {} ; echo pwned > {} ; cat'",
        filter_marker.display(),
        escaped_marker.display()
    );
    let hook = format!("sh -c 'echo pwned > {}'", hook_marker.display());
    run_git(
        &repository,
        &["config", "filter.hostile.smudge", filter.as_str()],
    );
    run_git(&repository, &["config", "filter.hostile.required", "true"]);
    run_git(&repository, &["config", "core.hooksPath", hook.as_str()]);
    std::fs::write(repository.join(".gitattributes"), "*.txt filter=hostile\n")
        .expect("write attributes");

    let managed_root = tempfile::tempdir().expect("managed root");
    let scratch = tempfile::tempdir().expect("scratch");
    let service = WorktreeService::memory(
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    )
    .with_contained_creator(Arc::new(
        codegg::git_contained_worktree::ContainedGitWorktreeCreator::new(
            git_binary(),
            scratch.path().to_path_buf(),
        ),
    ));

    let (record, _lease) = service
        .create(&create_request(&repository))
        .await
        .expect("the contained child creates the checkout");

    assert_eq!(record.state, ManagedWorktreeState::Ready);
    assert_eq!(record.health, WorktreeHealth::Clean);
    assert!(record.path.join("README.md").is_file());
    assert!(record.path.join(".git").is_file());

    // The checkout's pointer names the administration directory the grant
    // named — not a repository the child controls.
    let pointer = std::fs::read_to_string(record.path.join(".git")).expect("pointer");
    assert!(
        pointer.starts_with("gitdir: "),
        "the checkout's '.git' is not a linked-worktree pointer: {pointer}"
    );

    // Neither hostile surface reached the host.
    assert!(
        !escaped_marker.exists(),
        "the hostile filter wrote outside the grant"
    );
    assert!(
        !hook_marker.exists(),
        "the hostile hook wrote outside the grant"
    );

    // The positive control: the filter ran inside the grant. If this is
    // absent, the negative assertions above prove nothing about containment
    // — they would only prove the filter never ran.
    assert!(
        filter_marker.is_file(),
        "the hostile filter never ran, so the containment result is vacuous"
    );
}

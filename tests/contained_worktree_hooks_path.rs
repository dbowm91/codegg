//! C009's chosen disposition — **defence in depth**: the contained worktree
//! child must not run the repository's own hooks.
//!
//! ADR-0048 grants the child a **shared** administration root, so the child can
//! write any linked worktree's `HEAD`. The path to that write is real: `git
//! worktree add` runs the repository's `post-checkout` hook, and that hook
//! would run inside the containment. The maintainer's chosen disposition is to
//! pin `core.hooksPath` for the child, so the repository's own hooks cannot run
//! at all.
//!
//! The claim is proved against a **control** — the production argv with the pin
//! pair removed — so it cannot be vacuous. It does **not** claim the capability
//! is closed: the shared root stays writable, and the sandbox remains the
//! security property.

// Composing the launch refuses off Linux, so the whole target is a Linux qualification.
#![cfg(target_os = "linux")]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use codegg::git_contained_worktree::{
    compose_worktree_child_launch, resolve_git_binary, WorktreeChildPaths, WorktreeChildStep,
    WorktreeLaunchTiers, WORKTREE_CHILD_HOOKS_PATH_PIN,
};
use codegg::managed_process::{
    EnvironmentPolicy, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxRequest,
};

// ── Fixtures ──────────────────────────────────────────────────────────────

fn git_binary() -> PathBuf {
    resolve_git_binary().expect("a pinned Git executable")
}

fn run_git(cwd: &Path, args: &[&str]) -> String {
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

/// A repository whose `core.hooksPath` points at a `post-checkout` hook that
/// records that it ran. Returns the repository root and the marker.
fn repository_with_a_hostile_hook(workspace: &Path) -> (PathBuf, PathBuf) {
    let repository = workspace.join("repository");
    std::fs::create_dir_all(&repository).expect("repository directory");
    run_git(&repository, &["init", "--quiet", "--initial-branch=main"]);
    run_git(&repository, &["config", "user.email", "test@example.com"]);
    run_git(&repository, &["config", "user.name", "Test"]);
    run_git(&repository, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repository.join("README.md"), "hello\n").expect("write README");
    run_git(&repository, &["add", "."]);
    run_git(&repository, &["commit", "--quiet", "-m", "init"]);

    let hooks = workspace.join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks directory");
    let marker = workspace.join("hook-ran");
    let hook = hooks.join("post-checkout");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\necho ran > {}\n", marker.display()),
    )
    .expect("write the hook");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make the hook executable");
    }
    run_git(
        &repository,
        &["config", "core.hooksPath", &hooks.display().to_string()],
    );

    (repository, marker)
}

/// The paths one contained worktree creation names, mirroring the production
/// composition. The checkout is created empty, because Git refuses to create a
/// worktree in a non-empty directory.
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

/// The production argv with the hooks-path pin pair removed, derived from the
/// production argv rather than written out separately, so it cannot drift from
/// what the child really runs.
fn without_the_pin(argv: &[OsString]) -> Vec<OsString> {
    let pin = WORKTREE_CHILD_HOOKS_PATH_PIN;
    let mut control: Vec<OsString> = Vec::with_capacity(argv.len());
    let mut index = 0;
    while index < argv.len() {
        let carries_the_pin = index + 1 < argv.len()
            && argv[index] == OsStr::new(pin[0])
            && argv[index + 1] == OsStr::new(pin[1]);
        if carries_the_pin {
            index += 2;
            continue;
        }
        control.push(argv[index].clone());
        index += 1;
    }
    control
}

/// Run the contained child's own launch, without the sandbox.
///
/// The sandbox is not what this target proves: the claim is about the argv the
/// child is given, and running it unsandboxed lets the control's hook run.
async fn run_the_child(argv: Vec<OsString>, cwd: PathBuf, environment: EnvironmentPolicy) {
    let mut request = ManagedProcessRequest::new(
        argv,
        cwd,
        ProcessProvenance::new("c009-hooks-path", "probe"),
    );
    request.environment_policy = environment;
    request.sandbox = SandboxRequest::Disabled;
    request.timeout = Some(Duration::from_secs(60));

    let result = ManagedProcessService::run(request)
        .await
        .expect("the child runs");
    assert!(
        result.exit_status.success(),
        "the child failed: {}",
        String::from_utf8_lossy(&result.stderr.head).trim()
    );
}

// ── The claim and its control ────────────────────────────────────────────

/// The contained child does not run the repository's `post-checkout` hook.
///
/// The control runs first, in the same repository and with the same command:
/// without the pin, the hook runs and records that it did. The claim runs next,
/// with the production argv: the hook does not run, and the worktree is still
/// created.
#[tokio::test]
async fn the_contained_child_does_not_run_the_repositorys_post_checkout_hook() {
    let workspace = tempfile::tempdir().expect("workspace");
    let (repository, marker) = repository_with_a_hostile_hook(workspace.path());
    let base = run_git(&repository, &["rev-parse", "HEAD"]);

    let scratch = workspace.path().join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch directory");

    // The control: the production argv with the pin pair removed. The hook runs.
    let control_checkout = workspace.path().join("control-worktree");
    std::fs::create_dir_all(&control_checkout).expect("control checkout");
    let control_paths = child_paths(
        &repository,
        &control_checkout,
        &scratch,
        "codegg/worktree/c009-control",
        &base,
    );
    let control_launch = compose_worktree_child_launch(
        &control_paths,
        WorktreeChildStep::Create,
        WorktreeLaunchTiers::PRODUCTION,
    )
    .expect("compose the control launch");
    let control_argv = without_the_pin(&control_launch.argv);
    assert_eq!(
        control_argv.len(),
        control_launch.argv.len() - 2,
        "the production argv does not carry the hooks-path pin at all"
    );
    run_the_child(control_argv, control_launch.cwd, control_launch.environment).await;
    assert!(
        marker.is_file(),
        "the control did not run the repository's post-checkout hook, so the claim \
         below would be vacuous: the hook never runs for an unrelated reason"
    );
    std::fs::remove_file(&marker).expect("remove the control's marker");

    // The claim: the production argv. The hook does not run.
    let claim_checkout = workspace.path().join("claim-worktree");
    std::fs::create_dir_all(&claim_checkout).expect("claim checkout");
    let claim_paths = child_paths(
        &repository,
        &claim_checkout,
        &scratch,
        "codegg/worktree/c009-claim",
        &base,
    );
    let claim_launch = compose_worktree_child_launch(
        &claim_paths,
        WorktreeChildStep::Create,
        WorktreeLaunchTiers::PRODUCTION,
    )
    .expect("compose the contained launch");
    // The pin is on the argv, ahead of the subcommand: a Git **global** option
    // only applies before it.
    assert_eq!(
        claim_launch.argv.get(1),
        Some(&OsString::from("-c")),
        "the production argv does not carry the hooks-path pin first: {:?}",
        claim_launch.argv
    );
    assert_eq!(
        claim_launch.argv.get(2),
        Some(&OsString::from("core.hooksPath=/dev/null")),
        "the production argv does not pin core.hooksPath: {:?}",
        claim_launch.argv
    );
    run_the_child(
        claim_launch.argv,
        claim_launch.cwd,
        claim_launch.environment,
    )
    .await;
    assert!(
        !marker.is_file(),
        "the repository's post-checkout hook ran inside the contained child, so the \
         hooks-path pin did not take effect"
    );

    // The operation itself still succeeded: the pin removes the hook, it does
    // not break the worktree creation.
    assert!(
        claim_checkout.join("README.md").is_file(),
        "the contained worktree was not created, so the pin cost the operation its result"
    );
}

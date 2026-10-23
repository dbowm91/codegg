//! Contained managed-worktree creation.
//!
//! ADR-0048 (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`)
//! grants exactly one operation shape: `git worktree add <path>
//! <existing-branch>` inside an OS-contained child. This module is that
//! operation.
//!
//! Three properties carry the whole contract:
//!
//! * **The branch is the daemon's.** The daemon creates it first, through the
//!   existing typed, policy-allowed `git branch` path. The child attaches to a
//!   branch that already exists, so it writes **no ref**.
//! * **The grant is the administration directory and nothing else.** The child
//!   may write the new checkout, the daemon scratch, and the linked worktree's
//!   own `worktrees/` directory inside the repository's Git directory. Every
//!   other path under that Git directory — `refs`, `objects`, `index`,
//!   `config`, `hooks`, `packed-refs` — is read-only to the child.
//! * **Containment is asserted, not requested.** The launch's *obtained*
//!   enforcement is read after the launch. A request is a hope; only the
//!   obtained fact is evidence.
//!
//! The child is still **untrusted code**: the repository's own configuration
//! may define filters, hooks, merge drivers, fsmonitor hooks and credential
//! helpers, and the sandbox — not the absence of that configuration — is what
//! keeps them from the host.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use codegg_core::worktree_service::{
    ContainedWorktreeCreator, ContainedWorktreeOutcome, ContainedWorktreeRequest,
    WorktreeServiceError,
};

use crate::git_mutations::GitMutationExecutor;
use crate::git_mutations_ops as git_ops;
use crate::managed_process::{
    EnvironmentPolicy, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxExecutionOutcome, SandboxRequest,
};
use crate::security::sandbox::{SandboxConfig, SandboxMode};

/// The daemon-pinned Git executable used by the contained child.
///
/// The binary is resolved by the daemon, not by the child and not by the
/// model, so the child cannot be pointed at a different Git.
pub fn resolve_git_binary() -> Option<PathBuf> {
    crate::git_publication::resolve_git_binary()
}

/// The daemon-owned scratch root the contained child may write to.
///
/// It is not a repository and it never becomes one: the mount view refuses a
/// `.git` entry inside a write root unless the launch explicitly expects a
/// pointer there.
pub fn default_worktree_scratch_root() -> PathBuf {
    crate::git_publication::default_publication_scratch_root()
        .parent()
        .map(|root| root.join("git-worktree"))
        .unwrap_or_else(|| PathBuf::from("git-worktree"))
}

/// The Linux enforcement tiers a contained worktree child is launched with.
///
/// The **grant** — the set of paths the child may write — is identical for
/// every tier. The tiers decide which enforcement dimensions the launch
/// obtains *on top of* that grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorktreeLaunchTiers {
    /// The M005 offline-network tier, which implies M004's process/IPC tier.
    pub offline_network: bool,
    /// The M004 process/IPC tier.
    pub process_isolation: bool,
}

impl WorktreeLaunchTiers {
    /// The production tiers: the M005 offline network and the M004
    /// process/IPC boundary it implies.
    pub const PRODUCTION: Self = Self {
        offline_network: true,
        process_isolation: true,
    };

    /// The filesystem grant alone.
    ///
    /// A host that cannot provide the M004 process/IPC boundary (Landlock
    /// ABI 6) can still enforce the **mount view**, which is the dimension
    /// this milestone's grant is about. The path set is identical; only the
    /// two dimensions the host cannot obtain are left out, and the launch
    /// reports which ones it actually obtained.
    pub const FILESYSTEM_ONLY: Self = Self {
        offline_network: false,
        process_isolation: false,
    };
}

/// Everything one contained worktree creation names.
///
/// The paths are resolved **before** the child runs, and they are the whole
/// grant: the child may write the checkout, the scratch and the linked
/// worktree's own administration directory, and nothing else.
#[derive(Clone, Debug)]
pub struct WorktreeChildPaths {
    /// The daemon-pinned Git executable.
    pub git_binary: PathBuf,
    /// The repository's Git directory.
    pub gitdir: PathBuf,
    /// The source working tree — readable, never writable.
    pub repository_root: PathBuf,
    /// The checkout the child creates.
    pub checkout: PathBuf,
    /// The daemon scratch root.
    pub scratch: PathBuf,
    /// `<common-gitdir>/worktrees/` — the grant's administration root.
    pub administration_root: PathBuf,
    /// The linked worktree's own administration directory.
    pub administration_dir: PathBuf,
    /// The daemon's branch, already created.
    pub branch: String,
    /// The base commit the checkout is written from.
    pub base_commit: String,
}

/// The contained operation, in two steps.
///
/// Step one creates the linked worktree **detached**. Step two attaches the
/// daemon's branch to it.
///
/// The split is not a preference — it is forced by what Git does. Measured on
/// Git 2.43.0 and 2.55.0: `git worktree add <path> <existing-branch>` locks
/// and rewrites the branch ref even when the value is unchanged, so the
/// attached form cannot run inside a grant that excludes `refs`. The detached
/// form writes no ref at all, and attaching the branch afterwards is a write
/// to the administration directory the grant already names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeChildStep {
    /// `git --git-dir=<gitdir> worktree add --detach <checkout> <commit>`
    Create,
    /// `git --git-dir=<administration-dir> symbolic-ref HEAD refs/heads/<branch>`
    AttachBranch,
}

/// Creates a managed worktree through a contained `git worktree add`.
pub struct ContainedGitWorktreeCreator {
    git_binary: PathBuf,
    scratch_root: PathBuf,
}

impl ContainedGitWorktreeCreator {
    pub fn new(git_binary: PathBuf, scratch_root: PathBuf) -> Self {
        Self {
            git_binary,
            scratch_root,
        }
    }

    /// The daemon-pinned Git executable the child runs.
    pub fn git_binary(&self) -> &Path {
        &self.git_binary
    }

    /// The daemon-owned scratch root.
    pub fn scratch_root(&self) -> &Path {
        &self.scratch_root
    }
}

/// Everything the contained child runs with: the argv, the working directory,
/// the environment and the sandbox description.
///
/// The composition is separated from the launch so that the **grant** can be
/// inspected, and the launch itself exercised, without a host that can obtain
/// every enforcement tier.
pub struct WorktreeChildLaunch {
    /// The argv, starting with the daemon-pinned Git executable.
    pub argv: Vec<std::ffi::OsString>,
    /// The child's working directory: the repository's Git directory. The
    /// child never sees the source working tree.
    pub cwd: PathBuf,
    /// The child's sanitized environment.
    pub environment: EnvironmentPolicy,
    /// The sandbox description — the grant.
    pub spec: crate::security::sandbox::SandboxLaunchSpec,
}

/// Compose the launch one step of the contained worktree child runs with.
///
/// This is the **one authorized operation** ADR-0048 grants, in its two
/// steps. The grant is identical for both: the child may write the checkout,
/// the daemon scratch and the linked worktree's own administration directory,
/// and nothing else.
pub fn compose_worktree_child_launch(
    paths: &WorktreeChildPaths,
    step: WorktreeChildStep,
    tiers: WorktreeLaunchTiers,
) -> Result<WorktreeChildLaunch, WorktreeServiceError> {
    let spec = build_worktree_grant(paths, step, tiers)?;
    let argv = std::iter::once(paths.git_binary.as_os_str().to_owned())
        .chain(spec.args.iter().map(std::ffi::OsString::from))
        .collect();
    Ok(WorktreeChildLaunch {
        argv,
        cwd: paths.gitdir.clone(),
        environment: worktree_child_environment(&paths.scratch),
        spec,
    })
}

/// The environment the contained worktree child runs with.
///
/// The child's Git configuration is the repository's own and nothing else: the
/// system and global configuration files are explicitly excluded, so a
/// host-level `core.hooksPath` or filter driver cannot reach the child.
pub fn worktree_child_environment(scratch: &Path) -> EnvironmentPolicy {
    EnvironmentPolicy::sanitized()
        .with_var("GIT_CONFIG_NOSYSTEM", "1")
        .with_var("GIT_CONFIG_GLOBAL", "/dev/null")
        .with_var("GIT_ATTR_NOSYSTEM", "1")
        .with_var("GIT_TERMINAL_PROMPT", "0")
        .with_var("LC_ALL", "C")
        .with_var("TMPDIR", scratch.display().to_string())
}

#[async_trait]
impl ContainedWorktreeCreator for ContainedGitWorktreeCreator {
    async fn create(
        &self,
        request: &ContainedWorktreeRequest,
    ) -> Result<ContainedWorktreeOutcome, WorktreeServiceError> {
        let branch = request.branch.trim();
        if branch.is_empty() {
            return Err(WorktreeServiceError::InvalidInput(
                "the contained worktree creation has no branch to attach to".into(),
            ));
        }
        if request.base_commit.trim().is_empty() {
            return Err(WorktreeServiceError::InvalidInput(
                "the contained worktree creation has no base commit".into(),
            ));
        }

        // ── The branch is the daemon's ─────────────────────────────────────
        //
        // It is created before the child runs, through the existing typed,
        // policy-allowed `git branch` path. The child therefore attaches to a
        // branch that already exists and writes no ref at all. If the branch
        // were absent, `git worktree add` would try to create it — and the
        // child's grant excludes `refs`, so the attempt would fail closed
        // rather than write a ref.
        let executor = GitMutationExecutor::new();
        let created = git_ops::branch_create(
            &executor,
            &request.repository_root,
            branch,
            Some(request.base_commit.as_str()),
            false,
        )
        .await
        .map_err(|error| {
            WorktreeServiceError::Git(format!(
                "the worktree branch '{branch}' could not be created: {error}"
            ))
        })?;
        if !created.success {
            return Err(WorktreeServiceError::Git(format!(
                "the worktree branch '{branch}' could not be created: {}",
                created.stderr.trim()
            )));
        }

        // ── The paths the grant names ──────────────────────────────────────
        let gitdir = canonical(&request.repository_gitdir)?;
        let scratch = prepare_scratch_root(&self.scratch_root)?;
        // Neither the checkout nor the administration directory exists yet —
        // the daemon creates the first and Git creates the second — so both
        // are resolved through their existing parent rather than through
        // `canonicalize`, which demands every component exist.
        let checkout = canonical_pending(&request.worktree_path)?;
        // A linked worktree's administration directory always lives inside
        // the repository's Git directory — `git worktree add` has no
        // separate-git-dir option — so the grant names it explicitly.
        let administration_root = canonical_pending(&gitdir.join("worktrees"))?;

        // The administration directory is created by Git on demand, but the
        // grant names it, so it has to exist before the launch.
        std::fs::create_dir_all(&administration_root).map_err(|error| {
            WorktreeServiceError::Git(format!(
                "the worktree administration directory could not be prepared: {error}"
            ))
        })?;
        let administration_root = canonical(&administration_root)?;
        let administration_dir = administration_root.join(
            checkout
                .file_name()
                .ok_or_else(|| {
                    WorktreeServiceError::InvalidInput(
                        "the worktree path has no directory name".into(),
                    )
                })?
                .to_string_lossy()
                .to_string(),
        );

        // Git refuses to create a worktree in a non-empty directory, and it
        // creates the checkout's `.git` pointer itself. The checkout therefore
        // has to exist and be empty before the child runs.
        //
        // `create_dir` and not `create_dir_all`: the leaf must not exist
        // already, and a path that does exist is exactly the collision this
        // refuses — including a symlink planted at the leaf, which
        // `create_dir_all` would silently follow to somewhere else.
        std::fs::create_dir(&checkout).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                WorktreeServiceError::Collision(checkout.display().to_string())
            } else {
                WorktreeServiceError::Storage(format!(
                    "the worktree checkout directory could not be prepared: {error}"
                ))
            }
        })?;
        let checkout = canonical(&checkout)?;
        if std::fs::read_dir(&checkout)
            .map_err(|error| {
                WorktreeServiceError::Storage(format!(
                    "the worktree checkout directory could not be inspected: {error}"
                ))
            })?
            .next()
            .is_some()
        {
            return Err(WorktreeServiceError::Collision(
                checkout.display().to_string(),
            ));
        }

        // ── The one authorized operation, in its two steps ────────────────
        //
        // Step one: `git --git-dir=<gitdir> worktree add --detach <checkout>
        // <commit>`. The child runs with the Git directory as its working
        // directory and never sees the source working tree at all: it needs
        // the objects, refs and index, not the project's files.
        //
        // Step two: `git --git-dir=<admin> symbolic-ref HEAD
        // refs/heads/<branch>`. The attached form of `worktree add` would
        // lock and rewrite the branch ref — measured, not assumed — so the
        // worktree is created detached and the daemon's branch is attached
        // afterwards, with Git's own plumbing, writing only the
        // administration directory the grant already names.
        let paths = WorktreeChildPaths {
            git_binary: self.git_binary.clone(),
            gitdir: gitdir.clone(),
            repository_root: request.repository_root.clone(),
            checkout: checkout.clone(),
            scratch: scratch.clone(),
            administration_root: administration_root.clone(),
            administration_dir: administration_dir.clone(),
            branch: branch.to_string(),
            base_commit: request.base_commit.clone(),
        };

        let result =
            run_worktree_child_step(&paths, WorktreeChildStep::Create, branch, request.timeout)
                .await?;
        let (backend, abi, network_enforced, guarantees) = match &result.sandbox {
            SandboxExecutionOutcome::Enforced {
                backend,
                abi,
                network,
                guarantees,
                ..
            } => (
                backend.to_string(),
                *abi,
                matches!(
                    network,
                    codegg_core::approval::NetworkEnforcement::Enforced { .. }
                ),
                guarantees.clone(),
            ),
            _ => {
                return Err(WorktreeServiceError::Git(
                    "the worktree child returned without obtained enforcement".into(),
                ))
            }
        };
        // The mount view is the dimension this milestone's grant is about, and
        // it must be **obtained**, not merely requested: the obtained scope
        // has to name the administration directory as a writable root. A
        // request is a hope; only the obtained fact is evidence.
        let obtained_writes = guarantees
            .iter()
            .find_map(|entry| entry.strip_prefix("writable grant roots: "))
            .unwrap_or_default();
        for required in [&checkout, &scratch, &administration_root] {
            if !obtained_writes.contains(&required.display().to_string()) {
                return Err(WorktreeServiceError::Git(format!(
                    "the worktree child did not obtain write access to {}",
                    required.display()
                )));
            }
        }
        if !network_enforced {
            return Err(WorktreeServiceError::Git(
                "the worktree child did not obtain the offline network boundary".into(),
            ));
        }
        if !result.exit_status.success() {
            return Err(WorktreeServiceError::Git(format!(
                "git worktree add exited with {}: {}",
                result.exit_status.code().unwrap_or(-1),
                String::from_utf8_lossy(&result.stderr.head).trim()
            )));
        }
        verify_pointer(&checkout, &administration_dir)?;

        // Step two: attach the daemon's branch. The branch was created by the
        // daemon before the child ran, and the child never writes a ref — the
        // attach only repoints the new worktree's own `HEAD` at that branch.
        let attach = run_worktree_child_step(
            &paths,
            WorktreeChildStep::AttachBranch,
            branch,
            request.timeout,
        )
        .await?;
        if !attach.exit_status.success() {
            return Err(WorktreeServiceError::Git(format!(
                "git symbolic-ref exited with {}: {}",
                attach.exit_status.code().unwrap_or(-1),
                String::from_utf8_lossy(&attach.stderr.head).trim()
            )));
        }
        // The attach is a contained launch too, and its obtained enforcement
        // is asserted the same way: the administration directory must be in
        // the scope the launch actually obtained.
        let attach_guarantees = match &attach.sandbox {
            SandboxExecutionOutcome::Enforced { guarantees, .. } => guarantees.clone(),
            _ => {
                return Err(WorktreeServiceError::Git(
                    "the worktree attach returned without obtained enforcement".into(),
                ))
            }
        };
        let attach_writes = attach_guarantees
            .iter()
            .find_map(|entry| entry.strip_prefix("writable grant roots: "))
            .unwrap_or_default();
        if !attach_writes.contains(&administration_root.display().to_string()) {
            return Err(WorktreeServiceError::Git(format!(
                "the worktree attach did not obtain write access to {}",
                administration_root.display()
            )));
        }

        Ok(ContainedWorktreeOutcome {
            administration_dir,
            backend,
            abi,
            network_enforced,
        })
    }
}

/// The child is launched through the canonical managed-process owner, with the
/// sandbox description as a **required** sandbox: there is no uncontained
/// route to fall back to.
async fn run_worktree_child_step(
    paths: &WorktreeChildPaths,
    step: WorktreeChildStep,
    branch: &str,
    timeout: std::time::Duration,
) -> Result<crate::managed_process::ManagedProcessResult, WorktreeServiceError> {
    let launch = compose_worktree_child_launch(paths, step, WorktreeLaunchTiers::PRODUCTION)?;
    let mut managed = ManagedProcessRequest::new(
        launch.argv,
        launch.cwd,
        ProcessProvenance::new("contained-git-worktree", branch),
    );
    managed.timeout = Some(timeout);
    managed.environment_policy = launch.environment;
    managed.sandbox = SandboxRequest::Required(launch.spec);
    ManagedProcessService::run(managed).await.map_err(|error| {
        WorktreeServiceError::Git(format!("the worktree child failed to run: {error}"))
    })
}

/// Verify that the checkout's `.git` is the **pointer file** the child's grant
/// authorized, naming the administration directory the grant named.
///
/// The mount view exempts exactly one path from its "hide a `.git` that does
/// not exist yet" rule: the checkout's own `.git`. The child could in
/// principle have created a *directory* there instead — a nested repository
/// the child controls. This check is what makes that a failure rather than a
/// success, and it is the reason the exemption is safe to grant.
fn verify_pointer(checkout: &Path, administration_dir: &Path) -> Result<(), WorktreeServiceError> {
    let pointer_path = checkout.join(".git");
    let metadata = std::fs::symlink_metadata(&pointer_path).map_err(|error| {
        WorktreeServiceError::Git(format!(
            "the created worktree has no '.git' pointer: {error}"
        ))
    })?;
    if !metadata.is_file() {
        return Err(WorktreeServiceError::Git(
            "the created worktree's '.git' is not a pointer file".into(),
        ));
    }
    let pointer = std::fs::read_to_string(&pointer_path).map_err(|error| {
        WorktreeServiceError::Git(format!(
            "the created worktree's '.git' pointer could not be read: {error}"
        ))
    })?;
    let named = pointer
        .lines()
        .find_map(|line| line.strip_prefix("gitdir: "))
        .map(str::trim)
        .ok_or_else(|| {
            WorktreeServiceError::Git(
                "the created worktree's '.git' is not a linked-worktree pointer".into(),
            )
        })?;
    let named = PathBuf::from(named);
    let named = if named.is_absolute() {
        named
    } else {
        checkout.join(named)
    };
    let named = named.canonicalize().map_err(|error| {
        WorktreeServiceError::Git(format!(
            "the created worktree's administration directory could not be resolved: {error}"
        ))
    })?;
    let expected = administration_dir.canonicalize().map_err(|error| {
        WorktreeServiceError::Git(format!(
            "the expected administration directory could not be resolved: {error}"
        ))
    })?;
    if named != expected {
        return Err(WorktreeServiceError::Git(format!(
            "the created worktree's '.git' names {} instead of the expected administration directory",
            named.display()
        )));
    }
    Ok(())
}

/// The launch description the contained child runs with.
///
/// Building the description is separated from launching it so the **grant**
/// can be asserted on a host that cannot obtain the M004/M005 tiers: the
/// composition is identical whether or not this machine can enforce it.
/// What differs between hosts is whether the launch obtains the enforcement,
/// and that is read from the launch's own result.
///
/// The grant is exactly ADR-0048 clause 3: the child may write the new
/// checkout, the daemon scratch, and the linked worktree's own
/// administration directory. Every other path under the repository's Git
/// directory — `refs`, `objects`, `index`, `config`, `hooks`,
/// `packed-refs` — is read-only to it.
pub fn build_worktree_grant(
    paths: &WorktreeChildPaths,
    step: WorktreeChildStep,
    tiers: WorktreeLaunchTiers,
) -> Result<crate::security::sandbox::SandboxLaunchSpec, WorktreeServiceError> {
    let args = worktree_child_args(paths, step);
    // The write roots are **per step**, and the narrower grant is always the
    // better one. The creation step writes the checkout's files and the
    // administration directory. The attach step writes only the
    // administration directory — and that matters, because the protected
    // mount view follows the checkout's `.git` pointer and protects the
    // administration directory it names. Granting the checkout as a write
    // root in the attach step would let the mount view narrow the grant
    // under the child, silently, at the moment the child needs to write
    // there.
    let (write_roots, expects_pointer) = match step {
        WorktreeChildStep::Create => (
            vec![
                paths.checkout.clone(),
                paths.scratch.clone(),
                paths.administration_root.clone(),
            ],
            true,
        ),
        WorktreeChildStep::AttachBranch => (
            vec![paths.scratch.clone(), paths.administration_root.clone()],
            false,
        ),
    };
    let mut config = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(
            write_roots
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        )
        // The repository's Git directory is visible and immutable, except for
        // the administration directory named above.
        .with_read_only_paths(vec![paths.gitdir.display().to_string()]);
    if expects_pointer {
        // The child creates the checkout's `.git` **pointer file**, which
        // names the administration directory. It is not repository metadata
        // the child controls.
        config = config.with_expected_created_git_pointers(vec![paths
            .checkout
            .join(".git")
            .display()
            .to_string()]);
    }
    if tiers.offline_network {
        config = config.with_linux_offline_network(true);
    } else if tiers.process_isolation {
        config = config.with_linux_process_isolation(true);
    } else {
        config = config.with_linux_protected_mount_view(true);
    }
    let spec = config
        .launch_spec(&paths.git_binary, &args, Some(&paths.gitdir))
        .map_err(|error| {
            WorktreeServiceError::Git(format!(
                "the worktree child could not be contained: {error}"
            ))
        })?;
    assert_grant(&spec, paths, &write_roots, expects_pointer, tiers)?;
    Ok(spec)
}

/// The argv one step of the contained child runs.
///
/// Both shapes are fixed by the daemon. The child cannot choose a different
/// Git, a different path, or a different branch: the argv *is* the authority.
fn worktree_child_args(paths: &WorktreeChildPaths, step: WorktreeChildStep) -> Vec<String> {
    match step {
        WorktreeChildStep::Create => vec![
            format!("--git-dir={}", paths.gitdir.display()),
            "worktree".to_string(),
            "add".to_string(),
            "--detach".to_string(),
            paths.checkout.display().to_string(),
            paths.base_commit.clone(),
        ],
        WorktreeChildStep::AttachBranch => vec![
            format!("--git-dir={}", paths.administration_dir.display()),
            "symbolic-ref".to_string(),
            "HEAD".to_string(),
            format!("refs/heads/{}", paths.branch),
        ],
    }
}

/// The structural claim ADR-0048 clause 3 makes, asserted against the launch
/// description the child will actually run with.
///
/// The child may write exactly three things: the new checkout, the daemon
/// scratch, and the linked worktree's own administration directory. Every
/// other path under the repository's Git directory — `refs`, `objects`,
/// `index`, `config`, `hooks`, `packed-refs` — is read-only to it.
///
/// This is checked against the **built** description rather than against the
/// configuration that was requested, because a request is a hope and the
/// description is what the child runs with.
fn assert_grant(
    spec: &crate::security::sandbox::SandboxLaunchSpec,
    paths: &WorktreeChildPaths,
    expected_writes: &[PathBuf],
    expects_pointer: bool,
    tiers: WorktreeLaunchTiers,
) -> Result<(), WorktreeServiceError> {
    let fail = |reason: &str| WorktreeServiceError::Git(reason.to_string());
    let gitdir = &paths.gitdir;
    let checkout = &paths.checkout;
    let administration_root = &paths.administration_root;

    let writes = &spec.write_paths;
    for required in expected_writes {
        if !writes.iter().any(|write| write == required) {
            return Err(fail(&format!(
                "the worktree child was not granted write access to {}",
                required.display()
            )));
        }
    }
    // The device nodes are writable under every profile — they hold no
    // persistent state — so they are not part of the grant. Everything else
    // is.
    let devices = device_write_allowances();
    let granted = writes
        .iter()
        .filter(|write| !devices.contains(write))
        .collect::<Vec<_>>();
    if granted.len() != expected_writes.len() {
        return Err(fail(&format!(
            "the worktree child was granted {} writable roots, expected exactly {}",
            granted.len(),
            expected_writes.len()
        )));
    }
    if writes.iter().any(|write| write == gitdir) {
        return Err(fail(
            "the repository's Git directory was granted to the worktree child as writable",
        ));
    }
    // Nothing under the Git directory is writable except the administration
    // directory itself. This is the property that makes branch creation by
    // the child impossible rather than merely unintended.
    for write in writes {
        if write.starts_with(gitdir) && write != administration_root {
            return Err(fail(&format!(
                "the worktree child was granted write access to {} inside the repository's Git directory",
                write.display()
            )));
        }
    }
    // The source working tree is not the child's to write. The child needs
    // the objects, the refs and the index; it does not need the project's
    // files.
    if writes.contains(&paths.repository_root) {
        return Err(fail(
            "the source working tree was granted to the worktree child as writable",
        ));
    }
    if !spec.read_paths.iter().any(|read| read == gitdir) {
        return Err(fail(
            "the repository's Git directory was not granted to the worktree child as readable",
        ));
    }
    // The pointer exemption is a property of the step that creates the
    // checkout. The attach step creates nothing, so it asks for no exemption.
    let expected_pointer = checkout.join(".git");
    if expects_pointer {
        if spec.expected_created_git_pointers.len() != 1
            || spec.expected_created_git_pointers[0] != expected_pointer
        {
            return Err(fail(
                "the worktree child's expected Git pointer is not the checkout's own '.git'",
            ));
        }
    } else if !spec.expected_created_git_pointers.is_empty() {
        return Err(fail(
            "the worktree child was granted a Git pointer exemption it does not need",
        ));
    }
    if tiers.process_isolation && !spec.linux_process_isolation {
        return Err(fail(
            "the worktree child was not launched with the M004 process/IPC tier",
        ));
    }
    if tiers.offline_network && !spec.linux_offline_network {
        return Err(fail(
            "the worktree child was not launched with the M005 offline network",
        ));
    }
    if tiers.process_isolation && !spec.linux_protected_mount_view {
        return Err(fail(
            "the worktree child was not launched with the protected mount view",
        ));
    }
    Ok(())
}

/// The device nodes the sandbox makes writable under every profile.
fn device_write_allowances() -> Vec<PathBuf> {
    crate::security::sandbox::policy::device_write_allowances()
        .into_iter()
        .filter(|path| path.exists())
        .filter_map(|path| path.canonicalize().ok())
        .collect()
}

fn canonical(path: &Path) -> Result<PathBuf, WorktreeServiceError> {
    path.canonicalize().map_err(|error| {
        WorktreeServiceError::Git(format!(
            "the worktree path {} could not be resolved: {error}",
            path.display()
        ))
    })
}

/// Resolve a path whose **final component does not exist yet**.
///
/// `std::fs::canonicalize` fails when any component is missing, but two of
/// the paths this module names are created *after* the grant is composed: the
/// checkout (the daemon creates it, because Git refuses to create a worktree
/// in a non-empty directory) and the administration directory (Git creates it
/// on demand, and a repository that has never had a linked worktree has no
/// `worktrees` directory at all).
///
/// So the path is resolved through its nearest **existing** ancestor, and the
/// components that do not exist yet are re-joined as names. That still
/// resolves every symlink that could redirect the path — the part that
/// matters — without demanding the leaf be there first.
fn canonical_pending(path: &Path) -> Result<PathBuf, WorktreeServiceError> {
    let mut pending: Vec<std::ffi::OsString> = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        match current.canonicalize() {
            Ok(resolved) => {
                let mut resolved = resolved;
                for part in pending.iter().rev() {
                    resolved.push(part);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = current.file_name() else {
                    return Err(unresolved(path, &error));
                };
                let Some(parent) = current.parent() else {
                    return Err(unresolved(path, &error));
                };
                pending.push(name.to_os_string());
                current = parent.to_path_buf();
            }
            Err(error) => return Err(unresolved(path, &error)),
        }
    }
}

fn unresolved(path: &Path, error: &std::io::Error) -> WorktreeServiceError {
    WorktreeServiceError::Git(format!(
        "the worktree path {} could not be resolved: {error}",
        path.display()
    ))
}

/// Prepare the daemon-owned scratch root the contained child may write to.
///
/// The directory is created with **explicit owner-only permissions** rather
/// than the ambient umask. A scratch root another local user could write to
/// would be a place to plant files the child trusts, so the permissions are
/// set rather than inherited — the same reason the managed-agent scratch
/// subsystem refuses a group- or other-writable cache root.
fn prepare_scratch_root(root: &Path) -> Result<PathBuf, WorktreeServiceError> {
    std::fs::create_dir_all(root).map_err(|error| {
        WorktreeServiceError::Storage(format!(
            "the worktree scratch root could not be prepared: {error}"
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(root)
            .map_err(|error| {
                WorktreeServiceError::Storage(format!(
                    "the worktree scratch root could not be inspected: {error}"
                ))
            })?
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(root, permissions).map_err(|error| {
            WorktreeServiceError::Storage(format!(
                "the worktree scratch root could not be made private: {error}"
            ))
        })?;
    }
    canonical(root)
}

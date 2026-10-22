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
        let checkout = canonical(&request.worktree_path)?;
        let scratch = prepare_scratch_root(&self.scratch_root)?;
        // A linked worktree's administration directory always lives inside
        // the repository's Git directory — `git worktree add` has no
        // separate-git-dir option — so the grant names it explicitly.
        let administration_root = canonical(&gitdir.join("worktrees"))?;
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

        // The administration directory is created by Git on demand, but the
        // grant names it, so it has to exist before the launch.
        std::fs::create_dir_all(&administration_root).map_err(|error| {
            WorktreeServiceError::Git(format!(
                "the worktree administration directory could not be prepared: {error}"
            ))
        })?;

        // Git refuses to create a worktree in a non-empty directory, and it
        // creates the checkout's `.git` pointer itself. The checkout therefore
        // has to exist and be empty before the child runs.
        std::fs::create_dir_all(&checkout).map_err(|error| {
            WorktreeServiceError::Storage(format!(
                "the worktree checkout directory could not be prepared: {error}"
            ))
        })?;
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

        // ── The one authorized argv shape ──────────────────────────────────
        //
        // `git --git-dir=<gitdir> worktree add <checkout> <existing-branch>`.
        //
        // The child runs with the Git directory as its working directory and
        // never sees the source working tree at all: it needs the objects,
        // refs and index, not the project's files.
        let spec = build_worktree_grant(
            &self.git_binary,
            &gitdir,
            &request.repository_root,
            &checkout,
            &scratch,
            &administration_root,
            branch,
        )?;

        let command = std::iter::once(spec.target.as_os_str().to_owned())
            .chain(spec.args.iter().map(std::ffi::OsString::from))
            .collect();
        let mut managed = ManagedProcessRequest::new(
            command,
            gitdir.clone(),
            ProcessProvenance::new("contained-git-worktree", branch),
        );
        managed.timeout = Some(request.timeout);
        managed.environment_policy = EnvironmentPolicy::sanitized()
            .with_var("GIT_CONFIG_NOSYSTEM", "1")
            .with_var("GIT_CONFIG_GLOBAL", "/dev/null")
            .with_var("GIT_ATTR_NOSYSTEM", "1")
            .with_var("GIT_TERMINAL_PROMPT", "0")
            .with_var("LC_ALL", "C")
            .with_var("TMPDIR", scratch.display().to_string());
        managed.sandbox = SandboxRequest::Required(spec);

        let result = ManagedProcessService::run(managed).await.map_err(|error| {
            WorktreeServiceError::Git(format!("the worktree child failed to run: {error}"))
        })?;
        let (backend, abi, network_enforced) = match &result.sandbox {
            SandboxExecutionOutcome::Enforced {
                backend,
                abi,
                network,
                ..
            } => (
                backend.to_string(),
                *abi,
                matches!(
                    network,
                    codegg_core::approval::NetworkEnforcement::Enforced { .. }
                ),
            ),
            _ => {
                return Err(WorktreeServiceError::Git(
                    "the worktree child returned without obtained enforcement".into(),
                ))
            }
        };
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

        Ok(ContainedWorktreeOutcome {
            administration_dir,
            backend,
            abi,
            network_enforced,
        })
    }
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
    git_binary: &Path,
    gitdir: &Path,
    repository_root: &Path,
    checkout: &Path,
    scratch: &Path,
    administration_root: &Path,
    branch: &str,
) -> Result<crate::security::sandbox::SandboxLaunchSpec, WorktreeServiceError> {
    let args = vec![
        format!("--git-dir={}", gitdir.display()),
        "worktree".to_string(),
        "add".to_string(),
        checkout.display().to_string(),
        branch.to_string(),
    ];
    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(vec![
            checkout.display().to_string(),
            scratch.display().to_string(),
            administration_root.display().to_string(),
        ])
        // The repository's Git directory is visible and immutable, except for
        // the administration directory named above.
        .with_read_only_paths(vec![gitdir.display().to_string()])
        // The child creates the checkout's `.git` **pointer file**, which
        // names the administration directory. It is not repository metadata
        // the child controls.
        .with_expected_created_git_pointers(vec![checkout.join(".git").display().to_string()])
        .with_linux_offline_network(true)
        .launch_spec(git_binary, &args, Some(gitdir))
        .map_err(|error| {
            WorktreeServiceError::Git(format!(
                "the worktree child could not be contained: {error}"
            ))
        })?;
    assert_grant(
        &spec,
        gitdir,
        repository_root,
        checkout,
        scratch,
        administration_root,
    )?;
    Ok(spec)
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
    gitdir: &Path,
    repository_root: &Path,
    checkout: &Path,
    scratch: &Path,
    administration_root: &Path,
) -> Result<(), WorktreeServiceError> {
    let fail = |reason: &str| WorktreeServiceError::Git(reason.to_string());

    let writes = &spec.write_paths;
    for required in [checkout, scratch, administration_root] {
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
    if granted.len() != 3 {
        return Err(fail(&format!(
            "the worktree child was granted {} writable roots, expected exactly 3",
            granted.len()
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
    if writes.iter().any(|write| write == repository_root) {
        return Err(fail(
            "the source working tree was granted to the worktree child as writable",
        ));
    }
    if !spec.read_paths.iter().any(|read| read == gitdir) {
        return Err(fail(
            "the repository's Git directory was not granted to the worktree child as readable",
        ));
    }
    let expected_pointer = checkout.join(".git");
    if spec.expected_created_git_pointers.len() != 1
        || spec.expected_created_git_pointers[0] != expected_pointer
    {
        return Err(fail(
            "the worktree child's expected Git pointer is not the checkout's own '.git'",
        ));
    }
    if !spec.linux_process_isolation {
        return Err(fail(
            "the worktree child was not launched with the M004 process/IPC tier",
        ));
    }
    if !spec.linux_offline_network {
        return Err(fail(
            "the worktree child was not launched with the M005 offline network",
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

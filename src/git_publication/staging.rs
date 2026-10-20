//! The staging step of a C007 publication.
//!
//! Staging is the half of the publication that runs Git: it produces the
//! **private index** (and the private object database that goes with it) for
//! a set of repository-relative paths, without touching the repository.
//!
//! It is a trait because the two halves of the vertical have different
//! evidence requirements:
//!
//! * the **contained** implementation below is the production one — the
//!   daemon-pinned Git executable, launched through the managed-process
//!   owner with the M004 process/IPC tier and the M005 offline-network
//!   candidate, so the repository cannot influence what the child executes
//!   and the repository's Git metadata is read-only to it;
//! * a test may substitute its own implementation to drive the publication
//!   half — the compare-and-swap, the lock, the atomic replacement, and the
//!   crash recovery — on a host that cannot provide the M004/M005 tier.
//!
//! The trait boundary is where the trust boundary is: everything above it
//! (the journal, the compare-and-swap, the atomic publication) is the same
//! code in both cases.

use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;

use crate::managed_process::{
    EnvironmentPolicy, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxExecutionOutcome, SandboxRequest,
};
use crate::security::sandbox::{SandboxConfig, SandboxMode};

/// What the staging step is asked to produce.
#[derive(Debug, Clone)]
pub struct GitIndexStagingRequest {
    /// The private gitdir the child stages into. It holds the private object
    /// database and the private index.
    pub private_gitdir: PathBuf,
    /// The private index path. The child must leave the repository's own
    /// index byte-identical.
    pub private_index: PathBuf,
    /// The repository root, which the child reads the worktree from.
    pub repository_root: PathBuf,
    /// The repository's real gitdir. It is **read-only** to the child.
    pub repository_gitdir: PathBuf,
    /// The repository-relative paths to stage.
    pub paths: Vec<String>,
    /// The publication identity, used for the child's provenance.
    pub intent_id: String,
    /// The daemon-owned scratch root the child may write to.
    pub scratch_root: PathBuf,
    /// The bound on the child's runtime.
    pub timeout: Duration,
}

/// Produce a private index for a set of paths.
#[async_trait]
pub trait GitIndexStaging: Send + Sync {
    /// Stage the requested paths into the private index.
    ///
    /// The implementation must leave the repository's index and object
    /// database byte-identical. The publisher verifies that independently
    /// and refuses the publication if it does not hold.
    ///
    /// The returned boundary is the network boundary this staging step
    /// **obtained**. A step that does not run a contained child reports
    /// `NotReported`; a step that runs one reports what the launch observed.
    /// The publisher copies it into the receipt verbatim, so a receipt can
    /// never state a boundary the staging did not obtain.
    async fn stage(
        &self,
        request: &GitIndexStagingRequest,
    ) -> Result<crate::git_publication::ObtainedNetworkBoundary, String>;
}

/// The production staging step.
///
/// The child is the daemon-pinned Git executable, launched through
/// [`ManagedProcessService`] with the M004 process/IPC tier and the M005
/// offline-network candidate.
#[derive(Debug, Clone)]
pub struct ContainedGitIndexStaging {
    git_binary: PathBuf,
}

impl ContainedGitIndexStaging {
    /// Build the contained staging step around a pinned Git executable.
    pub fn new(git_binary: PathBuf) -> Self {
        Self { git_binary }
    }

    /// The pinned Git executable.
    pub fn git_binary(&self) -> &std::path::Path {
        &self.git_binary
    }
}

#[async_trait]
impl GitIndexStaging for ContainedGitIndexStaging {
    async fn stage(
        &self,
        request: &GitIndexStagingRequest,
    ) -> Result<crate::git_publication::ObtainedNetworkBoundary, String> {
        let root = &request.repository_root;
        let gitdir = &request.repository_gitdir;
        let mut args = vec![
            format!("--git-dir={}", request.private_gitdir.display()),
            format!("--work-tree={}", root.display()),
            // Literal pathspecs: a magic pathspec such as `:(glob)**` would
            // otherwise widen the staged set beyond the paths the caller
            // authorized.
            "--literal-pathspecs".to_string(),
            "add".to_string(),
            "--".to_string(),
        ];
        args.extend(request.paths.iter().cloned());

        let scratch = request
            .scratch_root
            .canonicalize()
            .map_err(|error| format!("the publication scratch is unavailable: {error}"))?;
        let spec = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_allowed_paths(vec![
                root.to_string_lossy().into_owned(),
                scratch.to_string_lossy().into_owned(),
            ])
            .with_linux_offline_network(true)
            .launch_spec(&self.git_binary, &args, Some(root))
            .map_err(|error| format!("the staging child could not be contained: {error}"))?;

        // The containment is the security property, so it is asserted rather
        // than assumed: M004's process/IPC tier, M005's offline network, and
        // a read-only view of the repository's Git metadata.
        if !spec.linux_process_isolation {
            return Err("the staging child was not launched with the M004 process/IPC tier".into());
        }
        if !spec.linux_offline_network {
            return Err("the staging child was not launched with the M005 offline network".into());
        }
        if !spec
            .protected_readonly_paths
            .iter()
            .any(|path| path == gitdir)
        {
            return Err(
                "the repository's Git metadata is not read-only to the staging child".into(),
            );
        }

        let command = std::iter::once(spec.target.as_os_str().to_owned())
            .chain(spec.args.iter().map(std::ffi::OsString::from))
            .collect();
        let mut managed = ManagedProcessRequest::new(
            command,
            root.to_path_buf(),
            ProcessProvenance::new("linux-git-index-publication", &request.intent_id),
        );
        managed.timeout = Some(request.timeout);
        managed.environment_policy = EnvironmentPolicy::sanitized()
            .with_var("GIT_CONFIG_NOSYSTEM", "1")
            .with_var("GIT_CONFIG_GLOBAL", "/dev/null")
            .with_var("GIT_ATTR_NOSYSTEM", "1")
            .with_var("GIT_TERMINAL_PROMPT", "0")
            .with_var("LC_ALL", "C");
        managed.sandbox = SandboxRequest::Required(spec);

        let result = ManagedProcessService::run(managed)
            .await
            .map_err(|error| format!("the staging child failed to run: {error}"))?;
        if !matches!(result.sandbox, SandboxExecutionOutcome::Enforced { .. }) {
            return Err("the staging child returned without obtained enforcement".into());
        }
        if !result.exit_status.success() {
            return Err(format!(
                "git add exited with {}: {}",
                result.exit_status.code().unwrap_or(-1),
                String::from_utf8_lossy(&result.stderr.head).trim()
            ));
        }
        // The boundary is read off the launch's own obtained fact, so the
        // receipt states what the child actually ran with.
        Ok(crate::git_publication::ObtainedNetworkBoundary::from_launch(&result.sandbox))
    }
}

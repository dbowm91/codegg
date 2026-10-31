//! M007-H (work package C) — the daemon's own git revert.
//!
//! `/revert` used to be a frontend that spawned `git checkout HEAD -- <path>`
//! itself: an unfenced subprocess, run in the TUI, with no writer permit and
//! no record of who wrote what. The write now lives here.
//!
//! Four properties are kept. **Containment**: every path is validated
//! against the checkout the daemon resolved from its own durable binding, so
//! a caller cannot name a tree the daemon does not own. **The typed
//! operation**: the write is `GitOperation::Checkout`, executed by the
//! canonical `GitMutationExecutor`. **The writer fence**: the write is
//! contained by the C003 checkout writer fence and, when the checkout is
//! shared, serialized by that checkout's writer permit — the same fence the
//! model's typed git tool takes; the fence's scope is the acting session's
//! own run, resolved from durable state. **The record**: the revert is
//! authorized as a `git.write` operation and audited as a file mutation.

use std::path::PathBuf;
use std::sync::Arc;

use codegg_git::{GitOperation, RepoPath, RepoRoot};
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::git::{GitRevertRequestDto, GitRevertResultDto};

use crate::core::writer_fence::DaemonWriterFenceFactory;
use crate::tool::writer_fence::{
    NativeEffectClass, WriterFenceFactory, WriterFenceOutcome, WriterFencePermit,
    WriterFenceRequest, WriterScopeRequest,
};

/// The daemon refuses the revert when the checkout cannot be resolved.
const REFUSAL_REVERT_UNAVAILABLE: &str = "git_revert_unavailable";
/// The daemon refuses the revert when the request names no path.
const REFUSAL_REVERT_NO_PATH: &str = "git_revert_no_path";
/// The daemon refuses the revert when a path leaves the checkout.
const REFUSAL_REVERT_OUTSIDE_CHECKOUT: &str = "git_revert_outside_checkout";
/// The daemon refuses the revert when the writer fence refuses it.
const REFUSAL_REVERT_FENCED: &str = "git_revert_fenced";
/// The daemon refuses the revert when git itself refuses the operation.
const REFUSAL_REVERT_FAILED: &str = "git_revert_failed";

/// Serve the git family.
///
/// The read half is served by the read-only git fact layer; the write half
/// is this module's own revert.
pub(crate) async fn handle_git_family_request(
    daemon: &crate::core::daemon::CoreDaemon,
    request: CoreRequest,
    authority: &codegg_core::transport_auth::RequestAuthorityContext,
) -> Result<CoreResponse, crate::error::AppError> {
    match request {
        CoreRequest::GitRevert { request } => handle_git_revert(daemon, request, authority).await,
        other => daemon.handle_git_request(other).await,
    }
}

/// The daemon's `/revert`.
async fn handle_git_revert(
    daemon: &crate::core::daemon::CoreDaemon,
    request: GitRevertRequestDto,
    authority: &codegg_core::transport_auth::RequestAuthorityContext,
) -> Result<CoreResponse, crate::error::AppError> {
    if request.paths.is_empty() {
        return Ok(revert_error(
            REFUSAL_REVERT_NO_PATH,
            "the revert names no path to restore",
        ));
    }

    // The checkout comes from the daemon's own durable binding. The caller
    // names a project, never a directory.
    let root = match daemon
        .resolve_project_repository_root(&request.project_id)
        .await
    {
        Ok(root) => root,
        Err(_) => {
            return Ok(revert_error(
                REFUSAL_REVERT_UNAVAILABLE,
                "the project's checkout could not be resolved",
            ))
        }
    };
    let repo_root = match git_root(&root) {
        Ok(root) => root,
        Err(message) => return Ok(revert_error(REFUSAL_REVERT_UNAVAILABLE, &message)),
    };

    // Containment: every path is validated against the checkout before the
    // fence is consulted, so an escaping path never reaches git at all.
    let mut repo_paths: Vec<RepoPath> = Vec::with_capacity(request.paths.len());
    for path in &request.paths {
        match RepoPath::new(&repo_root, path) {
            Ok(repo_path) => repo_paths.push(repo_path),
            Err(_) => {
                return Ok(revert_error(
                    REFUSAL_REVERT_OUTSIDE_CHECKOUT,
                    &format!("{path} is not inside the checkout"),
                ))
            }
        }
    }

    let targets: Vec<PathBuf> = repo_paths
        .iter()
        .map(|path| repo_root.as_path().join(path.as_str()))
        .collect();

    // The fence's scope is the acting session's own run, resolved from
    // durable state. A session with no run holds no managed checkout, so
    // there is no fence to take — the same shape the model's typed git
    // tool has when its execution carries no fence.
    let permit = match acquire_revert_writer_permit(daemon, &request, &targets, authority).await {
        Ok(permit) => permit,
        Err(message) => return Ok(revert_error(REFUSAL_REVERT_FENCED, &message)),
    };

    let operation = GitOperation::Checkout {
        target: Some("HEAD".to_string()),
        paths: Some(repo_paths.clone()),
        create: false,
        force: false,
    };

    let executor = crate::git_mutations::GitMutationExecutor::new()
        .with_env_policy(crate::git_mutations::GitEnvPolicy::default())
        .with_timeout(std::time::Duration::from_secs(30));
    let result = executor.execute(&operation, repo_root.as_path()).await;

    let (outcome, failure) = match &result {
        Ok(result) => {
            let outcome = match result.outcome {
                codegg_git::MutationOutcome::NoOp => WriterFenceOutcome::NoEffect,
                _ => WriterFenceOutcome::Settled,
            };
            (outcome, None)
        }
        Err(error) => (
            WriterFenceOutcome::Unproven(error.to_string()),
            Some(error.to_string()),
        ),
    };
    settle(permit, outcome).await;

    if let Some(message) = failure {
        return Ok(revert_error(REFUSAL_REVERT_FAILED, &message));
    }

    let result = result.expect("the failure was returned above");
    let paths: Vec<String> = repo_paths
        .iter()
        .map(|path| path.as_str().to_owned())
        .collect();

    Ok(CoreResponse::GitRevert {
        result: GitRevertResultDto {
            project_id: request.project_id,
            workspace_id: request.workspace_id,
            paths,
            outcome: result.outcome.label().to_string(),
        },
    })
}

/// The repository root the checkout is written in.
///
/// The durable binding names the repository's own root; a binding that names
/// a directory inside one is walked up to the repository, exactly as the
/// frontend's own `git` invocation did.
fn git_root(root: &PathBuf) -> Result<RepoRoot, String> {
    if root.as_os_str().is_empty() {
        return Err("the project's checkout could not be resolved".to_string());
    }
    let repo_root = match codegg_core::worktree::find_git_root(root) {
        Some(found) => found,
        None => return Err("not a git repository".to_string()),
    };
    RepoRoot::new(repo_root).map_err(|error| format!("the checkout is unusable: {error}"))
}

/// Take the checkout's writer permit for the revert.
///
/// The permit is taken before the first effect and settled after it. A
/// refusal is a refusal: the daemon does not write without the permit.
async fn acquire_revert_writer_permit(
    daemon: &crate::core::daemon::CoreDaemon,
    request: &GitRevertRequestDto,
    targets: &[PathBuf],
    authority: &codegg_core::transport_auth::RequestAuthorityContext,
) -> Result<Option<WriterFencePermit>, String> {
    let Some(session_id) = request.session_id.as_deref() else {
        return Ok(None);
    };
    let Some(permit_service) = daemon.deps.writer_permit_service.clone() else {
        return Ok(None);
    };
    let Some(run_id) = revert_run_id(daemon, session_id).await else {
        return Ok(None);
    };
    let factory = DaemonWriterFenceFactory::with_team_state(
        permit_service,
        Arc::clone(&daemon.worktree_service),
        Arc::clone(&daemon.deps.agent_run_store),
        Some(Arc::clone(&daemon.deps.run_group_service)),
        daemon
            .pool
            .as_ref()
            .map(|pool| Arc::new(codegg_core::team::TeamStore::new(pool.clone()))),
    );
    let fence = factory
        .fence_for(WriterScopeRequest {
            agent_run_id: Some(run_id),
            workspace_root: targets.first().cloned().unwrap_or_default(),
            attempt_id: None,
            cancellation: None,
        })
        .await
        .map_err(|error| format!("the checkout's writer permit is unavailable: {error}"))?;
    let Some(fence) = fence else {
        return Ok(None);
    };
    let request = WriterFenceRequest {
        effect_class: NativeEffectClass::GitMutation,
        targets: targets.to_vec(),
        idempotency_key: crate::tool::writer_fence::writer_fence_key("git_revert", targets, None),
        granted_roots: Vec::new(),
        // The actor is the transport-bound principal of the request that
        // reached the daemon. It is never synthesized here.
        principal: Some(authority.principal().clone()),
    };
    fence
        .begin(request)
        .await
        .map_err(|error| format!("the checkout refused the write: {error}"))
}

/// The session's own run, when it has one.
///
/// The run is the authority for the checkout the session holds. A session
/// with no run holds no managed checkout.
async fn revert_run_id(
    daemon: &crate::core::daemon::CoreDaemon,
    session_id: &str,
) -> Option<String> {
    let runs = daemon
        .deps
        .agent_run_store
        .list_by_session(session_id)
        .await
        .ok()?;
    let record = runs
        .into_iter()
        .filter(|record| record.worktree_id.is_some())
        .next_back()?;
    Some(record.run_id.as_str().to_owned())
}

/// Settle the permit with the evidence the effect produced.
async fn settle(permit: Option<WriterFencePermit>, outcome: WriterFenceOutcome) {
    let Some(permit) = permit else {
        return;
    };
    if let Err(error) = permit.settle(outcome).await {
        tracing::warn!(error = %error, "the revert's writer permit could not be settled");
    }
}

/// A typed refusal, in the shape the frontend renders.
fn revert_error(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

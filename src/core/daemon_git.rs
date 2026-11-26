//! Daemon-owned Git revert and diff operations.
//!
//! Revert validates paths against the durable checkout binding, executes typed
//! `GitOperation::Checkout`, and uses the writer fence (plus a permit for shared
//! checkouts). It records `git.write` authorization and the file mutation. Diff
//! reads the daemon-bound checkout without mutation or a permit.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use codegg_git::{GitOperation, RepoPath, RepoRoot};
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::git::{
    GitDiffFileDto, GitDiffRequestDto, GitDiffResultDto, GitRevertRequestDto, GitRevertResultDto,
};

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

/// The daemon refuses the diff when the checkout cannot be resolved.
const REFUSAL_DIFF_UNAVAILABLE: &str = "git_diff_unavailable";
/// The daemon refuses the diff when a path leaves the checkout.
const REFUSAL_DIFF_OUTSIDE_CHECKOUT: &str = "git_diff_outside_checkout";
/// The daemon refuses the diff when git itself refuses the read.
const REFUSAL_DIFF_FAILED: &str = "git_diff_failed";

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
        CoreRequest::GitDiff { request } => handle_git_diff(daemon, request).await,
        other => daemon.handle_git_request(other).await,
    }
}

/// The daemon's `/diff` read (M007-I, work package A).
///
/// The two branches stay mutually exclusive, exactly as the frontend's own
/// read was. The `/diff <path>` branch reads **one** file's bytes at `HEAD`
/// and **one** file's bytes from the working tree; the bare `/diff` branch
/// reads the **one** whole-tree diff and no file. Neither branch writes, so
/// neither takes a writer permit nor a fence.
///
/// The bytes come from the **read-only** fact layer — the same
/// `GitEnvPolicy` the frontend's own path used — so the daemon reads the
/// same tree the same way. The typed mutating crate is not involved.
async fn handle_git_diff(
    daemon: &crate::core::daemon::CoreDaemon,
    request: GitDiffRequestDto,
) -> Result<CoreResponse, crate::error::AppError> {
    // The checkout comes from the daemon's own durable binding. The caller
    // names a project, never a directory.
    let root = match daemon
        .resolve_project_repository_root(&request.project_id)
        .await
    {
        Ok(root) => root,
        Err(_) => {
            return Ok(diff_error(
                REFUSAL_DIFF_UNAVAILABLE,
                "the project's checkout could not be resolved",
            ))
        }
    };
    let git_root = match codegg_core::worktree::find_git_root(&root) {
        Some(found) => found,
        None => {
            return Ok(diff_error(
                REFUSAL_DIFF_UNAVAILABLE,
                "the project's checkout is not a git repository",
            ))
        }
    };

    let Some(path) = request
        .path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    else {
        // The bare branch: one `git diff`, no file read.
        let argv = vec!["git".to_string(), "diff".to_string()];
        let output = match run_git_read(&argv, &git_root).await {
            Ok(output) => output,
            Err(message) => return Ok(diff_error(REFUSAL_DIFF_FAILED, &message)),
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Ok(diff_error(
                REFUSAL_DIFF_FAILED,
                &format!("git diff failed: {}", stderr.trim()),
            ));
        }
        let diff_text = String::from_utf8_lossy(&output.stdout).to_string();
        let (files, truncated) = split_unified_diff(&diff_text);
        return Ok(CoreResponse::GitDiff {
            result: GitDiffResultDto {
                project_id: request.project_id,
                workspace_id: request.workspace_id,
                path: None,
                old_content: String::new(),
                new_content: diff_text,
                title: "Repository Diff".to_string(),
                files,
                truncated,
            },
        });
    };

    // Containment: the path is resolved against the checkout the daemon
    // owns, so a caller cannot name a tree the daemon does not hold.
    let relative = match repo_relative_path(path) {
        Ok(relative) => relative,
        Err(message) => return Ok(diff_error(REFUSAL_DIFF_OUTSIDE_CHECKOUT, &message)),
    };

    // The `HEAD` side: one `git show HEAD:<path>`.
    let show_argv = vec![
        "git".to_string(),
        "show".to_string(),
        format!("HEAD:{relative}"),
    ];
    let old_content = match run_git_read(&show_argv, &git_root).await {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        // A file that is not in `HEAD` has no old side. The frontend's own
        // read produced the same empty string in that case.
        Ok(_) => String::new(),
        Err(message) => return Ok(diff_error(REFUSAL_DIFF_FAILED, &message)),
    };

    // The working-tree side: one file read.
    let new_content = std::fs::read_to_string(git_root.join(&relative)).unwrap_or_default();

    Ok(CoreResponse::GitDiff {
        result: GitDiffResultDto {
            project_id: request.project_id,
            workspace_id: request.workspace_id,
            path: Some(relative),
            old_content,
            new_content,
            title: format!("Diff: {path}"),
            files: Vec::new(),
            truncated: false,
        },
    })
}

/// M003 aggregate bound: the most files one bare diff answer carries.
const MAX_DIFF_FILES: usize = 200;
/// M003 aggregate bound: the most patch bytes one file may carry.
const MAX_DIFF_FILE_BYTES: usize = 256 * 1024;
/// M003 aggregate bound: the most patch bytes the whole answer may carry.
const MAX_DIFF_TOTAL_BYTES: usize = 1024 * 1024;

/// Split one unified working-tree patch into per-file sections.
///
/// The bare `/`diff` branch already runs exactly one `git diff`; this is a
/// parse of that one answer, not an N+1 read. A section starts at git's own
/// `diff --git` header, so no path is guessed and a file the parser cannot
/// name is still shown under its header text rather than dropped.
///
/// Bounds are explicit because the answer travels over the wire: file count,
/// per-file bytes and total bytes, each reported through `truncated` so the
/// reader is never shown a silently partial list.
fn split_unified_diff(patch: &str) -> (Vec<GitDiffFileDto>, bool) {
    /// Cut at a UTF-8 boundary and report that the section was shortened.
    fn cut(text: &str, max: usize) -> (String, bool) {
        if text.len() <= max {
            return (text.to_string(), false);
        }
        let mut end = max;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        (text[..end].to_string(), true)
    }

    /// The repository-relative path of a `diff --git` header.
    ///
    /// The `b/` side is the path after the change, which is what a reader
    /// navigates to. Both plain and quoted headers are accepted; a header the
    /// parser cannot name keeps its own text.
    fn header_path(line: &str) -> String {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if let Some(token) = tokens.iter().rev().find(|token| token.starts_with("b/")) {
            return token.trim_start_matches("b/").trim_matches('"').to_string();
        }
        line.trim().to_string()
    }

    fn flush(
        files: &mut Vec<GitDiffFileDto>,
        current: &mut Option<(String, String)>,
        total: &mut usize,
        aggregate_truncated: &mut bool,
    ) {
        let Some((path, body)) = current.take() else {
            return;
        };
        if body.trim().is_empty() {
            return;
        }
        let (patch, file_truncated) = cut(&body, MAX_DIFF_FILE_BYTES);
        if *total + patch.len() > MAX_DIFF_TOTAL_BYTES {
            *aggregate_truncated = true;
            return;
        }
        *total += patch.len();
        let binary = patch.contains("Binary files ")
            || patch.contains("GIT binary patch")
            || patch.contains("Binary file ");
        files.push(GitDiffFileDto {
            path,
            patch,
            binary,
            truncated: file_truncated,
        });
    }

    let mut files: Vec<GitDiffFileDto> = Vec::new();
    let mut current: Option<(String, String)> = None;
    let mut total = 0usize;
    let mut truncated = false;

    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            flush(&mut files, &mut current, &mut total, &mut truncated);
            if files.len() >= MAX_DIFF_FILES {
                truncated = true;
                break;
            }
            current = Some((header_path(line), line.to_string()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
        }
    }
    flush(&mut files, &mut current, &mut total, &mut truncated);
    (files, truncated)
}

/// The repository-relative form of a caller-supplied path.
///
/// The path is normalized against the checkout and refused when it would
/// leave it, so the daemon never reads a tree it does not own.
fn repo_relative_path(path: &str) -> Result<String, String> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(format!("{path} is not inside the checkout"));
    }
    let mut parts: Vec<String> = Vec::new();
    for component in candidate.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            std::path::Component::CurDir => {}
            // A `..` or a root component would leave the checkout.
            _ => return Err(format!("{path} is not inside the checkout")),
        }
    }
    if parts.is_empty() {
        return Err(format!("{path} is not inside the checkout"));
    }
    Ok(parts.join("/"))
}

/// Run one read-only git invocation under the read-only fact layer.
///
/// The read is a **read**: the policy is the same one the frontend's own
/// read used, so the object-reading allowlist and `GIT_OPTIONAL_LOCKS=0`
/// apply unchanged.
async fn run_git_read(argv: &[String], git_root: &Path) -> Result<std::process::Output, String> {
    let argv = argv.to_vec();
    let git_root = git_root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        crate::git_mutations::GitEnvPolicy::default()
            .apply_sync(&argv, &git_root)
            .map_err(|outcome| outcome.to_string())?
            .output()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("the git read could not be run: {error}"))?
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
fn git_root(root: &Path) -> Result<RepoRoot, String> {
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
        .rfind(|record| record.worktree_id.is_some())?;
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

/// A typed refusal for the diff read, in the shape the frontend renders.
fn diff_error(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod aggregate_diff_tests {
    use super::split_unified_diff;

    const TWO_FILES: &str = "\
diff --git a/src/a.rs b/src/a.rs
index 1111111..2222222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
-old a
+new a
diff --git a/src/b.rs b/src/b.rs
index 3333333..4444444 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
-old b
+new b
";

    /// M003: one git answer, two file sections, no second read.
    #[test]
    fn splits_one_patch_into_named_file_sections() {
        let (files, truncated) = split_unified_diff(TWO_FILES);
        assert!(!truncated);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(files[1].path, "src/b.rs");
        assert!(files[0].patch.contains("-old a"));
        assert!(files[1].patch.contains("+new b"));
        assert!(!files[0].binary);
    }

    /// A binary section is announced, never decoded as text.
    #[test]
    fn marks_a_binary_section() {
        let patch = "\
diff --git a/img.png b/img.png
index 1111111..2222222 100644
Binary files a/img.png and b/img.png differ
";
        let (files, truncated) = split_unified_diff(patch);
        assert!(!truncated);
        assert_eq!(files.len(), 1);
        assert!(files[0].binary);
    }

    /// An empty or preamble-only answer is not a file list.
    #[test]
    fn empty_patch_has_no_files() {
        let (files, truncated) = split_unified_diff("");
        assert!(files.is_empty());
        assert!(!truncated);
    }
}

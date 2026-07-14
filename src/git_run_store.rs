//! Persist Git mutating operations (and read-side events, when relevant) into
//! the `RunStore` for audit, replay, and the TUI "rerun" surface.
//!
//! This module mirrors the pattern used by `src/test_runner/runner.rs`
//! (`persist_to_run_store`) and `src/python_script/tool.rs`. The run is
//! tagged with `RunKind::GitMutation` for any operation that mutates the
//! repo, or `RunKind::GitRead` when reading (the latter is opt-in; reads
//! that go through `GitExecutionService` are not currently persisted
//! because they already surface structured payloads through the existing
//! `format_structured_result` formatter).
//!
//! Phase F: Phase F also persists recovery runs via the same `kind =
//! RunKind::GitMutation` path but a distinct `backend.detail` of the
//! form `"recover:<continue|abort|skip>"`. This lets operators
//! distinguish initial mutations from recovery actions in metrics
//! dashboards without changing the run-kind surface.

use std::path::Path;
use std::sync::Arc;

use codegg_core::run_store::{
    ActualBackend, ArtifactInput, ArtifactKind, BackendRecord, PlannedBackend, RiskRecord,
    RunCompletion, RunDraft, RunHandle, RunId, RunKind, RunOwnership, RunStatus, RunStore,
};

use crate::git_mutations::MutationResult;

/// Persist a completed `MutationResult` to the `RunStore`.
///
/// Writes:
/// * stdout artifact (model-unsafe)
/// * stderr artifact (model-unsafe)
/// * state delta JSON (model-safe)
/// * summary projection as model-safe StructuredJson
///
/// Returns the new `RunId` (`RunId`) on success, `None` if the store is
/// missing or persisting fails. Logging on failure is non-fatal — the
/// mutation itself has already succeeded.
pub async fn persist_mutation(
    store: &Option<Arc<dyn RunStore>>,
    result: &MutationResult,
    workdir: &Path,
    repo_root: &Path,
    backend_family: &str,
    backend_detail: Option<String>,
) -> Option<RunId> {
    let store = match store.as_ref() {
        Some(s) => s,
        None => return None,
    };

    let argv = codegg_git::render_argv(&result.operation);
    let command = argv.join(" ");
    let started_at = chrono::Utc::now();
    let status = if result.success {
        RunStatus::Complete
    } else {
        RunStatus::Failed
    };

    let draft = RunDraft {
        kind: RunKind::GitMutation,
        invocation: codegg_core::run_store::RunInvocation {
            command,
            argv: Some(argv),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: workdir.to_path_buf(),
        cwd: workdir.to_path_buf(),
        backend: BackendRecord {
            family: backend_family.to_string(),
            detail: backend_detail,
        },
        risk: RiskRecord {
            level: "high".to_string(),
            has_subprocess: true,
            has_git_mutation: true,
            has_destructive_mutation: false,
        },
        planned_backend: Some(PlannedBackend::Git),
        actual_backend: Some(ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };

    let handle = match begin_run_safe(store, draft).await {
        Some(h) => h,
        None => return None,
    };

    if !result.stdout.is_empty() {
        let _ = store
            .write_artifact(
                &handle,
                ArtifactInput {
                    kind: ArtifactKind::Stdout,
                    data: result.stdout.as_bytes().to_vec(),
                    mime_type: "text/plain".to_string(),
                    safe_for_model: false,
                },
            )
            .await;
    }
    if !result.stderr.is_empty() {
        let _ = store
            .write_artifact(
                &handle,
                ArtifactInput {
                    kind: ArtifactKind::Stderr,
                    data: result.stderr.as_bytes().to_vec(),
                    mime_type: "text/plain".to_string(),
                    safe_for_model: false,
                },
            )
            .await;
    }
    if let Ok(delta_json) = serde_json::to_vec_pretty(&result.delta) {
        let _ = store
            .write_artifact(
                &handle,
                ArtifactInput {
                    kind: ArtifactKind::StructuredJson,
                    data: delta_json,
                    mime_type: "application/json".to_string(),
                    safe_for_model: true,
                },
            )
            .await;
    }
    let summary = crate::git_mutation_projector::project_mutation(result);
    if !summary.is_empty() {
        let _ = store
            .write_artifact(
                &handle,
                ArtifactInput {
                    kind: ArtifactKind::StructuredJson,
                    data: summary.as_bytes().to_vec(),
                    mime_type: "application/json".to_string(),
                    safe_for_model: true,
                },
            )
            .await;
    }

    let completion = RunCompletion {
        status,
        completed_at: chrono::Utc::now(),
        permissions: Vec::new(),
        sandbox: None,
        projection: None,
        changes: result
            .delta
            .paths_staged
            .iter()
            .chain(result.delta.paths_unstaged.iter())
            .map(|p| codegg_core::run_store::ChangedPathRecord {
                path: std::path::PathBuf::from(p),
                kind: "mutated".to_string(),
            })
            .collect(),
        rerun: Some(codegg_core::run_store::RerunDescriptor {
            argv: Some(codegg_git::render_argv(&result.operation)),
            script_source_ref: None,
            backend_family: backend_family.to_string(),
            cwd: workdir.to_path_buf(),
            workspace_root: repo_root.to_path_buf(),
            mode: Some("git_mutation".to_string()),
            config_profile: None,
            parent_run_id: None,
        }),
        actual_backend: Some(ActualBackend::Git),
        fallback: None,
    };

    let _ = started_at; // not used at the moment; RunStore captures timing itself

    match store.complete_run(handle, completion).await {
        Ok(manifest) => Some(manifest.run_id),
        Err(e) => {
            tracing::warn!("git mutations: failed to complete RunStore run: {e}");
            None
        }
    }
}

async fn begin_run_safe(store: &Arc<dyn RunStore>, draft: RunDraft) -> Option<RunHandle> {
    match store.begin_run(draft).await {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::warn!("git mutations: failed to begin RunStore run: {e}");
            None
        }
    }
}

/// Persist a recovery operation (`continue`/`abort`/`skip`) as a RunStore run.
///
/// Same provenance as [`persist_mutation`] but tagged in `backend.detail` so
/// dashboards can distinguish recovery actions from initial mutations.
pub async fn persist_recovery(
    store: &Option<Arc<dyn RunStore>>,
    result: &MutationResult,
    workdir: &Path,
    repo_root: &Path,
    action: &str,
) -> Option<RunId> {
    let detail = format!("recover:{action}");
    // Delegate to the standard mutation persist path, which already
    // wires RunKind::GitMutation + DelegatedBackend. We override the
    // backend_detail so the run row is grep-able.
    persist_mutation(
        store,
        result,
        workdir,
        repo_root,
        "git_native_recovery",
        Some(detail),
    )
    .await
}

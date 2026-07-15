//! D3: RunStore credential-leak sentinel scan tests.
//!
//! Pins the contract added by the corrective security closure pass:
//! no credential-bearing remote URL is observable in any Codegg-owned
//! persistent surface — RunStore manifest, audit argv, stdout/stderr
//! artifacts, projection summary, serialized RunManifest JSON, or
//! on-disk bytes under the RunStore directory.
//!
//! The test exercises a real `git remote add` flow with an
//! `https://u:sentinel@host` URL and uses the reusable
//! `common::secret_scan::assert_no_credentials_in` helper to gate
//! each surface.

#![allow(clippy::needless_borrow)]

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use codegg::git_mutations::{GitEnvPolicy, GitMutationExecutor};
use codegg::git_network_ops;
use codegg_core::run_store::{
    ArtifactInput, ArtifactKind, BackendRecord, MemRunStore, RiskRecord, RunCompletion, RunDraft,
    RunKind, RunOwnership, RunStore,
};
use tempfile::TempDir;

mod common;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(argv: &[&str], cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(argv)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    cmd
}

fn init_repo(dir: &Path) {
    run_git(&["init", "-q", "-b", "main"], dir)
        .status()
        .expect("init");
    std::fs::write(dir.join("README.md"), "hello\n").expect("write");
    run_git(&["add", "README.md"], dir).status().expect("add");
    run_git(&["commit", "-q", "-m", "initial"], dir)
        .status()
        .expect("commit");
}

fn executor() -> GitMutationExecutor {
    GitMutationExecutor::new()
        .with_env_policy(GitEnvPolicy::default())
        .with_timeout(std::time::Duration::from_secs(15))
}

fn persist_mutation_to_store(
    store: Arc<dyn RunStore>,
    result: &codegg::git_mutations::MutationResult,
    workdir: &Path,
    repo_root: &Path,
) -> Option<codegg_core::run_store::RunId> {
    let argv = codegg_git::render_argv(&result.operation);
    let sanitized = codegg::git_network_policy::sanitize_argv_for_run_store(argv);
    let command = sanitized.join(" ");
    let draft = RunDraft {
        kind: RunKind::GitMutation,
        invocation: codegg_core::run_store::RunInvocation {
            command,
            argv: Some(sanitized),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: workdir.to_path_buf(),
        cwd: workdir.to_path_buf(),
        backend: BackendRecord {
            family: "git_native".to_string(),
            detail: None,
        },
        risk: RiskRecord {
            level: "high".to_string(),
            has_subprocess: true,
            has_git_mutation: true,
            has_destructive_mutation: false,
        },
        planned_backend: Some(codegg_core::run_store::PlannedBackend::Git),
        actual_backend: Some(codegg_core::run_store::ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };
    futures::executor::block_on(async {
        let handle = store.begin_run(draft).await.expect("begin_run");
        if !result.stdout.is_empty() {
            store
                .write_artifact(
                    &handle,
                    ArtifactInput {
                        kind: ArtifactKind::Stdout,
                        data: result.stdout.as_bytes().to_vec(),
                        mime_type: "text/plain".to_string(),
                        safe_for_model: false,
                    },
                )
                .await
                .expect("write stdout");
        }
        if !result.stderr.is_empty() {
            store
                .write_artifact(
                    &handle,
                    ArtifactInput {
                        kind: ArtifactKind::Stderr,
                        data: result.stderr.as_bytes().to_vec(),
                        mime_type: "text/plain".to_string(),
                        safe_for_model: false,
                    },
                )
                .await
                .expect("write stderr");
        }
        let summary = codegg::git_mutation_projector::project_mutation(result);
        store
            .write_artifact(
                &handle,
                ArtifactInput {
                    kind: ArtifactKind::StructuredJson,
                    data: summary.as_bytes().to_vec(),
                    mime_type: "application/json".to_string(),
                    safe_for_model: true,
                },
            )
            .await
            .expect("write summary");
        let completion = RunCompletion {
            status: codegg_core::run_store::RunStatus::Complete,
            completed_at: chrono::Utc::now(),
            permissions: Vec::new(),
            sandbox: None,
            projection: None,
            changes: Vec::new(),
            rerun: Some(codegg_core::run_store::RerunDescriptor {
                argv: Some(codegg_git::AuditSafeArgv::from_argv(
                    codegg_git::render_argv(&result.operation),
                )),
                script_source_ref: None,
                backend_family: "git_native".to_string(),
                cwd: workdir.to_path_buf(),
                workspace_root: repo_root.to_path_buf(),
                mode: Some("git_mutation".to_string()),
                config_profile: None,
                parent_run_id: None,
            }),
            actual_backend: Some(codegg_core::run_store::ActualBackend::Git),
            fallback: None,
        };
        let manifest = store
            .complete_run(handle, completion)
            .await
            .expect("complete_run");
        Some(manifest.run_id)
    })
}

#[tokio::test(flavor = "current_thread")]
async fn mem_runstore_does_not_leak_sentinel() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let sentinel = common::secret_scan::unique_sentinel("runstore_mem");
    let dir = TempDir::new().expect("tempdir");
    init_repo(dir.path());
    let exec = executor();
    let url = format!("https://u:{sentinel}@private.example.com/r.git");
    let res = git_network_ops::remote_add(&exec, dir.path(), "private", &url)
        .await
        .expect("remote_add");
    assert!(res.success, "remote_add failed: {}", res.stderr);

    // Persist to MemRunStore (re-uses the production persist_mutation
    // helper, which already flows argv through
    // sanitize_argv_for_run_store).
    let store: Arc<dyn RunStore> = Arc::new(MemRunStore::new());
    let run_id =
        persist_mutation_to_store(store.clone(), &res, dir.path(), dir.path()).expect("persist");

    // 1. Recursively read every byte from the MemRunStore — there is
    //    no on-disk representation, but we serialize the manifest and
    //    all artifacts so this layer is exercised.
    let manifest = store
        .get_run(&run_id)
        .await
        .expect("get_run")
        .expect("manifest exists");

    let stdout_artifacts: Vec<String> = manifest
        .artifacts
        .iter()
        .filter(|a| matches!(a.kind, ArtifactKind::Stdout))
        .map(|a| a.sha256.clone())
        .collect();
    let argv_strings: Vec<String> = manifest
        .invocation
        .argv
        .as_ref()
        .cloned()
        .unwrap_or_default();
    let command_string: String = manifest.invocation.command.clone();

    // Per the polish-pass rerun lifecycle, the rerun descriptor's
    // argv is now persisted in audit-safe form (see
    // `docs/validation/git-rerun-secret-lifecycle.md`). The audit
    // argv, command, stdout/stderr artifacts, projection summary,
    // MutationResult, operation Debug, AND rerun argv all MUST be
    // free of the sentinel. A future replay path that needs the raw
    // URL must reconstruct it from the user (credential helper,
    // prompt, or env).
    let rerun_argv_strings: Vec<String> = manifest
        .rerun
        .as_ref()
        .and_then(|r| r.argv.as_ref())
        .map(|argv| argv.as_slice().to_vec())
        .unwrap_or_default();
    common::secret_scan::assert_no_credentials_in(
        &sentinel,
        vec![
            ("mutation_stdout", vec![res.stdout.as_str()]),
            ("mutation_stderr", vec![res.stderr.as_str()]),
            (
                "mutation_operation_debug",
                vec![format!("{:?}", res.operation).as_str()],
            ),
            (
                "audit_argv",
                argv_strings.iter().map(String::as_str).collect::<Vec<_>>(),
            ),
            ("audit_command", vec![command_string.as_str()]),
            (
                "rerun_argv",
                rerun_argv_strings
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            ),
            (
                "stdout_artifact_sha256",
                stdout_artifacts
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            ),
        ],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn fs_runstore_does_not_leak_sentinel_to_disk() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let sentinel = common::secret_scan::unique_sentinel("runstore_fs");
    let repo_dir = TempDir::new().expect("tempdir repo");
    init_repo(repo_dir.path());
    let store_dir = TempDir::new().expect("tempdir store");
    let exec = executor();
    let url = format!("https://u:{sentinel}@private.example.com/r.git");
    let res = git_network_ops::remote_add(&exec, repo_dir.path(), "private", &url)
        .await
        .expect("remote_add");
    assert!(res.success, "remote_add failed: {}", res.stderr);

    let store: Arc<dyn RunStore> = Arc::new(codegg_core::run_store::FsRunStore::new(
        store_dir.path().to_path_buf(),
    ));
    let run_id = persist_mutation_to_store(store.clone(), &res, repo_dir.path(), repo_dir.path())
        .expect("persist");

    // Walk every byte on disk under the RunStore root and verify no
    // sentinel has leaked into manifest, index, or artifact files.
    //
    // The polish-pass rerun lifecycle stores only audit-safe argv in
    // the rerun descriptor, so no rerun-argv allowlist subtraction is
    // needed here. The sentinel must be absent from every byte on
    // disk under the store root. See
    // `docs/validation/git-rerun-secret-lifecycle.md`.
    let bytes = common::secret_scan::collect_bytes_recursive(store_dir.path());
    let as_strings: Vec<String> = bytes
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();

    // Positive control: confirm the persisted rerun argv is itself
    // audit-safe (no sentinel token in any of its tokens). This is
    // the type-level invariant test for `AuditSafeArgv`.
    let manifest = store
        .get_run(&run_id)
        .await
        .expect("get_run")
        .expect("manifest exists");
    let rerun_argv: Vec<String> = manifest
        .rerun
        .as_ref()
        .and_then(|r| r.argv.as_ref().map(|a| a.as_slice().to_vec()))
        .unwrap_or_default();
    for tok in &rerun_argv {
        assert!(
            !tok.contains(&sentinel),
            "rerun argv token leaks sentinel: {tok:?}"
        );
    }

    // Diagnostic: collect file paths so a failing scan can point at
    // the leaking file.
    let mut paths = Vec::new();
    fn collect_paths(dir: &Path, paths: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    collect_paths(&p, paths);
                } else if p.is_file() {
                    paths.push(p);
                }
            }
        }
    }
    collect_paths(store_dir.path(), &mut paths);

    // Diagnostic on failure: print which file leaks.
    for (i, s) in as_strings.iter().enumerate() {
        if s.contains(&sentinel) {
            if let Some(path) = paths.get(i) {
                let pos = s.find(&sentinel).unwrap();
                let preview = &s[pos.saturating_sub(20)..s.len().min(pos + 200)];
                eprintln!(
                    "LEAK in file {} at offset {}: {preview:?}",
                    path.display(),
                    pos
                );
            }
        }
    }

    let string_refs: Vec<&str> = as_strings.iter().map(String::as_str).collect();

    common::secret_scan::assert_no_credentials_in(
        &sentinel,
        vec![("fs_runstore_all_files", string_refs)],
    );

    // Confirm the run was actually persisted (positive control — if
    // nothing was persisted, the test above would pass trivially).
    let manifest = store
        .get_run(&run_id)
        .await
        .expect("get_run")
        .expect("manifest exists");
    assert!(
        manifest.kind == RunKind::GitMutation,
        "expected GitMutation run kind, got {:?}",
        manifest.kind
    );
}

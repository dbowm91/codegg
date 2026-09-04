//! Daemon-side validation and reconstruction for historical run reruns.
//!
//! The first supported vertical is the scheduler-owned supervised test run.
//! This module deliberately accepts only the small, credential-free durable
//! specification that the test runner can reconstruct. Other run kinds fail
//! closed until they have an explicit secret, Git, and worktree contract.

use std::path::{Path, PathBuf};

use codegg_core::jobs::{
    IdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob, ResourceRequest,
    RetryPolicy,
};
use codegg_core::run_store::{RunId, RunManifest, RunStatus};
use codegg_core::workspace::WorkspaceId;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RerunError {
    #[error("run kind '{0:?}' is not rerunnable")]
    UnsupportedKind(codegg_core::run_store::RunKind),
    #[error("run status '{0:?}' is not rerunnable")]
    IneligibleStatus(RunStatus),
    #[error("run has no reconstructable rerun specification")]
    MissingSpecification,
    #[error("rerun requires current credential reacquisition")]
    SecretReacquisitionRequired,
    #[error("historical run authority no longer matches the current session")]
    AuthorityChanged,
    #[error("historical workspace identity does not match the requested workspace")]
    WorkspaceChanged,
    #[error("historical rerun working directory is unavailable or outside the workspace")]
    InvalidBase,
}

impl RerunError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedKind(_) | Self::MissingSpecification => "ineligible_missing_spec",
            Self::IneligibleStatus(_) => "ineligible_status",
            Self::SecretReacquisitionRequired => "ineligible_secret_reacquisition_required",
            Self::AuthorityChanged => "ineligible_authority_changed",
            Self::WorkspaceChanged | Self::InvalidBase => "ineligible_missing_or_invalid_base",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRerun {
    pub parent_run_id: RunId,
    pub session_id: Option<String>,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub scope: Option<String>,
}

/// Validate the historical record against current workspace and session
/// authority. This function does not read or reconstruct credentials.
pub fn validate(
    manifest: &RunManifest,
    workspace_root: &Path,
    requested_session_id: Option<&str>,
) -> Result<ValidatedRerun, RerunError> {
    if !matches!(manifest.kind, codegg_core::run_store::RunKind::Test) {
        return Err(RerunError::UnsupportedKind(manifest.kind.clone()));
    }
    if !matches!(
        manifest.status,
        RunStatus::Complete | RunStatus::Failed | RunStatus::TimedOut
    ) {
        return Err(RerunError::IneligibleStatus(manifest.status.clone()));
    }
    let Some(descriptor) = manifest.rerun.as_ref() else {
        return Err(RerunError::MissingSpecification);
    };
    if descriptor.backend_family != "test_runner" {
        return Err(RerunError::MissingSpecification);
    }
    let Some(argv) = descriptor.argv.as_ref().map(|argv| argv.as_slice()) else {
        return Err(RerunError::MissingSpecification);
    };
    if argv.is_empty() {
        return Err(RerunError::MissingSpecification);
    }
    // AuditSafeArgv intentionally records redacted URL credentials. A test
    // rerun cannot silently turn that marker back into a secret.
    if argv.iter().any(|arg| arg.contains("://redacted@")) {
        return Err(RerunError::SecretReacquisitionRequired);
    }
    if let (Some(historical), Some(current)) =
        (manifest.session_id.as_deref(), requested_session_id)
    {
        if historical != current {
            return Err(RerunError::AuthorityChanged);
        }
    }

    let canonical_workspace =
        std::fs::canonicalize(workspace_root).map_err(|_| RerunError::InvalidBase)?;
    let historical_workspace = std::fs::canonicalize(&manifest.workspace_root)
        .map_err(|_| RerunError::WorkspaceChanged)?;
    if historical_workspace != canonical_workspace {
        return Err(RerunError::WorkspaceChanged);
    }
    let cwd = std::fs::canonicalize(&descriptor.cwd).map_err(|_| RerunError::InvalidBase)?;
    if !cwd.starts_with(&canonical_workspace) {
        return Err(RerunError::InvalidBase);
    }

    Ok(ValidatedRerun {
        parent_run_id: manifest.run_id.clone(),
        session_id: requested_session_id
            .map(str::to_owned)
            .or_else(|| manifest.session_id.clone()),
        argv: argv.to_vec(),
        cwd,
        scope: descriptor.mode.clone(),
    })
}

pub fn to_job(spec: ValidatedRerun, workspace_id: WorkspaceId) -> NewJob {
    let command = spec.argv.join(" ");
    NewJob {
        workspace_id,
        session_id: spec.session_id,
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Test {
            command,
            argv: spec.argv,
            cwd: Some(spec.cwd.to_string_lossy().into_owned()),
            scope: spec.scope,
            parent_run_id: Some(spec.parent_run_id),
        },
        resource_request: ResourceRequest::for_kind(JobKind::Test),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: Vec::new(),
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: Some("rerun".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use codegg_core::run_store::{
        BackendRecord, RerunDescriptor, RiskRecord, RunInvocation, RunKind, RunOwnership,
    };

    fn manifest(root: &Path) -> RunManifest {
        RunManifest {
            schema_version: 1,
            run_id: RunId::new_unchecked("parent-run"),
            session_id: Some("session-1".into()),
            parent_run_id: None,
            kind: RunKind::Test,
            invocation: RunInvocation {
                command: "cargo test".into(),
                argv: Some(vec!["cargo".into(), "test".into()]),
                script_hash: None,
            },
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            status: RunStatus::Complete,
            workspace_root: root.to_path_buf(),
            cwd: root.to_path_buf(),
            backend: BackendRecord {
                family: "test_runner".into(),
                detail: None,
            },
            risk: RiskRecord {
                level: "low".into(),
                has_subprocess: true,
                has_git_mutation: false,
                has_destructive_mutation: false,
            },
            permissions: Vec::new(),
            sandbox: None,
            artifacts: Vec::new(),
            projection: None,
            changes: Vec::new(),
            rerun: Some(RerunDescriptor {
                argv: Some(codegg_git::AuditSafeArgv::from_argv(vec![
                    "cargo".into(),
                    "test".into(),
                ])),
                script_source_ref: None,
                backend_family: "test_runner".into(),
                cwd: root.to_path_buf(),
                workspace_root: root.to_path_buf(),
                mode: Some("auto-rust".into()),
                config_profile: None,
                parent_run_id: None,
            }),
            planned_backend: None,
            actual_backend: None,
            fallback: None,
            ownership: RunOwnership::DelegatedBackend,
            asset_provenance: None,
        }
    }

    #[test]
    fn rerun_error_codes_are_stable_and_actionable() {
        assert_eq!(
            RerunError::SecretReacquisitionRequired.code(),
            "ineligible_secret_reacquisition_required"
        );
        assert_eq!(
            RerunError::AuthorityChanged.code(),
            "ineligible_authority_changed"
        );
    }

    #[test]
    fn eligible_test_rerun_reconstructs_safe_child_job() {
        let root = std::env::current_dir().unwrap();
        let spec = validate(&manifest(&root), &root, Some("session-1")).unwrap();
        let job = to_job(spec, WorkspaceId::new_unchecked("workspace-1"));
        assert_eq!(job.kind, JobKind::Test);
        assert_eq!(job.relation_kind.as_deref(), Some("rerun"));
        assert_eq!(job.idempotency, IdempotencyClass::SafeRepeat);
        match job.payload {
            JobPayload::Test {
                parent_run_id: Some(parent),
                argv,
                ..
            } => {
                assert_eq!(parent.as_str(), "parent-run");
                assert_eq!(argv, vec!["cargo", "test"]);
            }
            other => panic!("unexpected rerun payload: {other:?}"),
        }
    }

    #[test]
    fn rerun_rejects_incomplete_or_secret_dependent_history() {
        let root = std::env::current_dir().unwrap();
        let mut incomplete = manifest(&root);
        incomplete.status = RunStatus::Cancelled;
        assert_eq!(
            validate(&incomplete, &root, Some("session-1"))
                .unwrap_err()
                .code(),
            "ineligible_status"
        );

        let mut secret = manifest(&root);
        secret.rerun.as_mut().unwrap().argv = Some(codegg_git::AuditSafeArgv::from_argv(vec![
            "git".into(),
            "https://redacted@example.test/repo".into(),
        ]));
        assert_eq!(
            validate(&secret, &root, Some("session-1"))
                .unwrap_err()
                .code(),
            "ineligible_secret_reacquisition_required"
        );
    }
}
